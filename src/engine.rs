use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::core::{OsInfo, Report, ReportSummary};
use crate::platform;
use crate::rules::{RuleContext, RuleOutput};

#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub timeout: Duration,
    pub privacy_mask_home: bool,
    pub include_evidence: bool,
    pub show_progress: bool,
}

#[derive(Clone)]
pub struct Engine {
    opts: EngineOptions,
    home_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub scope: Option<String>,
    pub deep: bool,
    pub max_depth: usize,
    pub top_dirs: usize,
    pub exclude: Vec<String>,
    pub show_progress: bool,
}

impl Engine {
    pub fn new(opts: EngineOptions) -> Result<Self> {
        let home_dir = crate::platform::effective_home_dir()?;
        Ok(Self { opts, home_dir })
    }

    pub fn timeout(&self) -> Duration {
        self.opts.timeout
    }

    pub fn home_dir(&self) -> &std::path::Path {
        &self.home_dir
    }

    pub fn doctor(&self) -> Result<Report> {
        let deadline = Instant::now() + self.opts.timeout;
        let ctx = RuleContext {
            home_dir: self.home_dir.clone(),
            timeout: std::cmp::min(self.opts.timeout, Duration::from_secs(8)),
            deadline: Some(deadline),
            privacy_mask_home: self.opts.privacy_mask_home,
        };
        use std::io::IsTerminal;
        let progress_enabled = self.opts.show_progress && std::io::stderr().is_terminal();
        let pb = if progress_enabled {
            let pb = indicatif::ProgressBar::new_spinner();
            pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());
            pb.set_message("所見を収集中...");
            pb.enable_steady_tick(Duration::from_millis(120));
            Some(pb)
        } else {
            None
        };

        let mut outputs = crate::rules::doctor_rules(&ctx);
        outputs.extend(crate::rules::snapshots_rules(&ctx));

        if let Some(pb) = pb {
            pb.finish_and_clear();
        }
        Ok(self.report_from_outputs(
            outputs,
            vec![
                "System Data は、他カテゴリに属さない Apple/サードパーティのファイルをまとめた一般カテゴリです（Appleの定義に従う）。"
                    .to_string(),
                "中身は雑多で変動するため、macdiet は開発者環境で頻出の原因を推定し、原因カテゴリへ再分類して提示します。"
                    .to_string(),
            ],
        ))
    }

    pub fn snapshots_status(&self) -> Result<Report> {
        let ctx = RuleContext {
            home_dir: self.home_dir.clone(),
            timeout: self.opts.timeout,
            deadline: None,
            privacy_mask_home: self.opts.privacy_mask_home,
        };
        let outputs = crate::rules::snapshots_rules(&ctx);
        Ok(self.report_from_outputs(outputs, vec![
            "ローカルスナップショットは容量が必要な場合などに自動削除されることがあります（Apple の説明に従う）。".to_string(),
            "APFS スナップショットは Disk Utility で閲覧/削除できます（ツールはまずGUI導線を提示する）。".to_string(),
            "CLI からの thin/delete は R3（強い同意と慎重な運用が必要）。".to_string(),
        ]))
    }

    pub fn report(&self) -> Result<Report> {
        self.doctor()
    }

    pub fn scan(&self, req: ScanRequest) -> Result<Report> {
        if !req.deep {
            return self.doctor();
        }

        use std::io::IsTerminal;
        let progress_enabled = req.show_progress && std::io::stderr().is_terminal();

        let roots = self.resolve_scan_roots(req.scope.as_deref());
        let mut findings = Vec::new();
        let mut notes = vec![
            format!(
                "スキャン: deep=true max_depth={} top_dirs={}",
                req.max_depth, req.top_dirs
            ),
            format!("スキャン: excludes={:?}", req.exclude),
        ];

        for root in roots {
            if !root.exists() {
                notes.push(format!(
                    "スキャン: スコープが存在しません: {}",
                    root.display()
                ));
                continue;
            }

            let pb = if progress_enabled {
                let pb = indicatif::ProgressBar::new_spinner();
                pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());
                pb.set_message(format!(
                    "スキャン中 {}",
                    mask_home(&root, &self.home_dir, true)
                ));
                pb.enable_steady_tick(Duration::from_millis(120));
                Some(pb)
            } else {
                None
            };

            let result =
                crate::scan::top_directories(&root, req.max_depth, req.top_dirs, &req.exclude)
                    .with_context(|| format!("スキャン: {}", root.display()))?;

            if let Some(pb) = pb {
                pb.finish_and_clear();
            }

            notes.push(format!(
                "スキャン: {} files={} errors={}",
                mask_home(&result.root, &self.home_dir, true),
                result.file_count,
                result.error_count
            ));

            for entry in result.entries {
                let masked = mask_home(&entry.path, &self.home_dir, true);
                let id = format!("scan-top:{masked}");
                findings.push(crate::core::Finding {
                    id,
                    finding_type: "SCAN_TOP_DIR".to_string(),
                    title: format!("上位ディレクトリ: {masked}"),
                    estimated_bytes: entry.bytes,
                    confidence: if result.error_count == 0 { 0.9 } else { 0.5 },
                    risk_level: crate::core::RiskLevel::R0,
                    evidence: vec![
                        crate::core::Evidence::path(masked, true),
                        crate::core::Evidence::stat(format!(
                            "root={} max_depth={}",
                            mask_home(&result.root, &self.home_dir, true),
                            req.max_depth
                        )),
                    ],
                    recommended_actions: vec![],
                });
            }
        }

        findings.sort_by_key(|f| std::cmp::Reverse(f.estimated_bytes));

        Ok(self.report_from_outputs(
            findings
                .into_iter()
                .map(|finding| RuleOutput {
                    finding,
                    actions: vec![],
                })
                .collect(),
            notes,
        ))
    }

    fn report_from_outputs(&self, mut outputs: Vec<RuleOutput>, mut notes: Vec<String>) -> Report {
        outputs.sort_by_key(|o| std::cmp::Reverse(o.finding.estimated_bytes));

        let mut findings = Vec::new();
        let mut actions = Vec::new();
        for output in outputs {
            findings.push(output.finding);
            actions.extend(output.actions);
        }

        let estimated_total_bytes = findings.iter().map(|f| f.estimated_bytes).sum();
        let unobserved_errors = collect_unobserved_errors(&findings, &notes);
        let has_unobserved_findings = findings
            .iter()
            .any(|f| f.finding_type.ends_with("_UNOBSERVED"));
        let unobserved_bytes_estimate = estimate_unobserved_bytes(&findings);
        if unobserved_errors > 0 || has_unobserved_findings {
            if unobserved_errors > 0 {
                notes.push(format!(
                    "未観測: 一部の領域を走査できませんでした（errors={}）。結果は下限推定です。",
                    unobserved_errors
                ));
                if unobserved_bytes_estimate > 0 {
                    notes.push(
                        "未観測推定: unobserved_bytes は走査エラー数からの概算です（参考値）。"
                            .to_string(),
                    );
                }
            } else {
                notes.push("未観測: 一部の領域を観測できませんでした（権限/外部コマンド）。結果は下限推定です。".to_string());
            }
            notes.push("ヒント: macdiet を実行しているターミナルに Full Disk Access を許可すると精度が上がります（システム設定 → プライバシーとセキュリティ → フルディスクアクセス）。".to_string());
        }

        let generated_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string());

        let os: OsInfo =
            platform::os_info(std::cmp::min(self.opts.timeout, Duration::from_secs(2)));

        notes.sort();
        notes.dedup();

        Report {
            schema_version: "1.0".to_string(),
            tool_version: env!("CARGO_PKG_VERSION").to_string(),
            os,
            generated_at,
            summary: ReportSummary {
                estimated_total_bytes,
                unobserved_bytes: unobserved_bytes_estimate,
                notes,
            },
            findings,
            actions,
        }
    }

    fn resolve_scan_roots(&self, scope: Option<&str>) -> Vec<PathBuf> {
        let scope = scope.unwrap_or("dev").trim();
        match scope {
            "dev" => vec![
                self.home_dir.join("Library/Developer"),
                self.home_dir.join("Library/Caches/Homebrew"),
                self.home_dir.join(".cargo"),
                self.home_dir.join(".gradle"),
                self.home_dir.join(".npm"),
                self.home_dir.join(".pnpm-store"),
                self.home_dir.join("Library/pnpm/store"),
            ],
            "userlib" => vec![self.home_dir.join("Library")],
            "all-readable" => vec![self.home_dir.clone()],
            other => {
                let p = PathBuf::from(other);
                if p.is_absolute() {
                    vec![p]
                } else {
                    vec![self.home_dir.join(p)]
                }
            }
        }
    }
}

fn mask_home(path: &std::path::Path, home_dir: &std::path::Path, mask_home: bool) -> String {
    if !mask_home {
        return path.display().to_string();
    }

    let Ok(stripped) = path.strip_prefix(home_dir) else {
        return path.display().to_string();
    };
    let stripped = stripped.display().to_string();
    if stripped.is_empty() {
        "~".to_string()
    } else {
        format!("~/{stripped}")
    }
}

fn collect_unobserved_errors(findings: &[crate::core::Finding], notes: &[String]) -> u64 {
    let mut total: u64 = 0;
    for f in findings {
        for ev in &f.evidence {
            if ev.kind != crate::core::EvidenceKind::Stat {
                continue;
            }
            if let Some(n) = parse_errors_count(&ev.value) {
                total = total.saturating_add(n);
            }
        }
    }
    for note in notes {
        if let Some(n) = parse_errors_count(note) {
            total = total.saturating_add(n);
        }
    }
    total
}

fn estimate_unobserved_bytes(findings: &[crate::core::Finding]) -> u64 {
    let mut total: u64 = 0;
    for f in findings {
        if f.estimated_bytes == 0 {
            continue;
        }
        for ev in &f.evidence {
            if ev.kind != crate::core::EvidenceKind::Stat {
                continue;
            }
            let Some((files, errors)) = parse_files_errors(&ev.value) else {
                continue;
            };
            if files == 0 || errors == 0 {
                continue;
            }
            let avg = (f.estimated_bytes.saturating_add(files.saturating_sub(1))) / files;
            total = total.saturating_add(avg.saturating_mul(errors));
        }
    }
    total
}

fn parse_errors_count(s: &str) -> Option<u64> {
    let idx = s.find("errors=")?;
    let rest = &s[idx + "errors=".len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}

fn parse_files_count(s: &str) -> Option<u64> {
    let idx = s.find("files=")?;
    let rest = &s[idx + "files=".len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse::<u64>().ok()
}

fn parse_files_errors(s: &str) -> Option<(u64, u64)> {
    Some((parse_files_count(s)?, parse_errors_count(s)?))
}
