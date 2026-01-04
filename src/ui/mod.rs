use anyhow::Error;
use std::io::{self, Write};
use unicode_width::UnicodeWidthChar;

use crate::core::{ActionPlan, Finding, Report, RiskLevel};

#[derive(Debug, Clone)]
pub struct UiConfig {
    pub color: bool,
    pub stdin_is_tty: bool,
    pub stdout_is_tty: bool,
    pub stderr_is_tty: bool,
    pub max_table_rows: usize,
    pub quiet: bool,
    pub verbose: bool,
}

pub fn eprintln_error(err: &Error) {
    let mut stderr = io::stderr().lock();
    let _ = writeln!(stderr, "エラー:");
    let _ = writeln!(stderr, "  {err}");

    let mut causes = err.chain().skip(1).peekable();
    if causes.peek().is_some() {
        let _ = writeln!(stderr, "原因:");
        for cause in causes {
            let _ = writeln!(stderr, "  - {cause}");
        }
    }

    let _ = writeln!(stderr, "次に:");
    let _ = writeln!(
        stderr,
        "  - 詳細を見るには `--verbose` を付けて再実行してください"
    );
    let _ = writeln!(
        stderr,
        "  - 利用可能なコマンド/オプションは `macdiet --help` を参照してください"
    );
}

pub fn print_doctor(report: &Report, cfg: &UiConfig, top_n: usize) {
    if cfg.quiet {
        return;
    }

    let has_unobserved_note = report
        .summary
        .notes
        .iter()
        .any(|n| n.starts_with("未観測:"));
    let unobserved_display = if report.summary.unobserved_bytes == 0 && has_unobserved_note {
        "不明".to_string()
    } else {
        format_bytes(report.summary.unobserved_bytes)
    };
    let unobserved_approx = if report.summary.unobserved_bytes > 0 && has_unobserved_note {
        "≈"
    } else {
        ""
    };

    let mut out = io::stdout().lock();
    let _ = writeln!(
        out,
        "概要: 推定合計={}  未観測{}={}",
        format_bytes(report.summary.estimated_total_bytes),
        unobserved_approx,
        unobserved_display
    );
    for note in prioritize_notes(&report.summary.notes) {
        let _ = writeln!(out, "- {note}");
    }

    let total_findings = report.findings.len();
    let rows = cfg.max_table_rows.min(top_n).min(total_findings).max(0);

    let _ = writeln!(out);
    if total_findings > rows {
        let _ = writeln!(out, "上位の所見（{rows}件表示 / 全{total_findings}件）:");
    } else {
        let _ = writeln!(out, "上位の所見（{rows}件表示）:");
    }
    print_findings_table(&mut out, &report.findings, rows, cfg.color);

    use std::collections::HashSet;
    let shown_finding_ids: HashSet<&str> = report
        .findings
        .iter()
        .take(rows)
        .map(|f| f.id.as_str())
        .collect();

    let mut actions: Vec<&ActionPlan> = report
        .actions
        .iter()
        .filter(|a| {
            a.related_findings.is_empty()
                || a.related_findings
                    .iter()
                    .any(|f| shown_finding_ids.contains(f.as_str()))
        })
        .collect();
    if actions.is_empty() {
        actions = report.actions.iter().collect();
    }
    actions.sort_by_key(|a| (a.risk_level, std::cmp::Reverse(a.estimated_reclaimed_bytes)));

    if !actions.is_empty() {
        let _ = writeln!(out);
        let show_actions = actions.len().min(cfg.max_table_rows.max(1));
        if actions.len() > show_actions {
            let _ = writeln!(
                out,
                "推奨アクション（{show_actions}件表示 / 全{}件）:",
                actions.len()
            );
        } else {
            let _ = writeln!(out, "推奨アクション（{show_actions}件表示）:");
        }
        for action in actions.iter().take(show_actions) {
            let risk = format_risk(action.risk_level, cfg.color);
            let _ = writeln!(
                out,
                "- {} [{}]（推定: {}）",
                action.title,
                risk,
                format_bytes(action.estimated_reclaimed_bytes)
            );
        }
        if actions.len() > show_actions {
            let _ = writeln!(out, "- ...（残り{}件）", actions.len() - show_actions);
        }
    }

    let snapshots: Vec<&Finding> = report
        .findings
        .iter()
        .filter(|f| is_snapshot_finding_type(&f.finding_type))
        .collect();
    if !snapshots.is_empty() {
        use std::collections::HashMap;
        let actions_by_id: HashMap<&str, &ActionPlan> =
            report.actions.iter().map(|a| (a.id.as_str(), a)).collect();

        let _ = writeln!(out);
        let _ = writeln!(out, "スナップショット:");
        for finding in snapshots {
            let risk = format_risk(finding.risk_level, cfg.color);
            let _ = writeln!(out, "- {} [{}]", finding.title, risk);
            if finding.finding_type.ends_with("_UNOBSERVED") {
                if let Some(reason) = unobserved_reason(&finding.evidence) {
                    let _ = writeln!(out, "  - 理由: {reason}");
                }
            }
            if !finding.recommended_actions.is_empty() {
                let _ = writeln!(out, "  - 次の手順:");
                for r in &finding.recommended_actions {
                    if let Some(action) = actions_by_id.get(r.id.as_str()) {
                        let risk = format_risk(action.risk_level, cfg.color);
                        let _ = writeln!(
                            out,
                            "    - {} [{}]（{}） id={}",
                            action.title,
                            risk,
                            action_kind_label(&action.kind),
                            action.id
                        );
                    }
                }
            }
        }
    }
}

pub fn print_fix_plan(actions: &[ActionPlan], cfg: &UiConfig, max_risk: RiskLevel) {
    if cfg.quiet {
        return;
    }

    let mut out = io::stdout().lock();
    let estimated_total: u64 = actions
        .iter()
        .filter(|a| matches!(a.kind, crate::core::ActionKind::TrashMove { .. }))
        .map(|a| a.estimated_reclaimed_bytes)
        .sum();
    let preview_action_count = actions
        .iter()
        .filter(|a| a.risk_level > RiskLevel::R1)
        .count();
    let preview_total = if preview_action_count > 0 {
        estimate_preview_reclaim(actions)
    } else {
        0
    };

    let _ = writeln!(out, "掃除プラン（max_risk={max_risk}）");
    if preview_action_count > 0 {
        let _ = writeln!(
            out,
            "推定削減（TRASH_MOVE）: {}",
            format_bytes(estimated_total)
        );
        let _ = writeln!(
            out,
            "参考：削減見込み（R2+プレビュー）: {}",
            format_bytes(preview_total)
        );
        let _ = writeln!(
            out,
            "注意: R2+ は既定でプレビューのみです。CLIで実行できるのは R1/TRASH_MOVE と、許可リストされた RUN_CMD のみです。"
        );
    } else {
        let _ = writeln!(out, "推定削減: {}", format_bytes(estimated_total));
    }

    if actions.is_empty() {
        let _ = writeln!(out, "実行可能なアクションがありません。");
        return;
    }

    let _ = writeln!(out, "\nアクション:");
    for action in actions {
        let risk = format_risk(action.risk_level, cfg.color);
        let _ = writeln!(
            out,
            "- {} [{}]（推定: {} / {}）",
            action.title,
            risk,
            format_bytes(action.estimated_reclaimed_bytes),
            action_kind_label(&action.kind)
        );
        let _ = writeln!(out, "  - id: {}", action.id);
        if !action.related_findings.is_empty() {
            let _ = writeln!(out, "  - 対象: {}", action.related_findings.join(","));
        }

        match &action.kind {
            crate::core::ActionKind::TrashMove { paths }
            | crate::core::ActionKind::Delete { paths } => {
                if !paths.is_empty() {
                    let show_all = cfg.verbose || paths.len() <= 1;
                    let show_n = if show_all { paths.len() } else { 1 };
                    for p in paths.iter().take(show_n) {
                        let _ = writeln!(out, "  - パス: {p}");
                    }
                    if !show_all {
                        let _ = writeln!(out, "  - ...（残り{}件）", paths.len().saturating_sub(1));
                    }
                }
            }
            crate::core::ActionKind::RunCmd { cmd, args } => {
                let _ = writeln!(out, "  - コマンド: {}", format_cmdline(cmd, args));
            }
            crate::core::ActionKind::OpenInFinder { path } => {
                let _ = writeln!(out, "  - Finderで開く: {path}");
            }
            crate::core::ActionKind::ShowInstructions { markdown } => {
                let first = first_non_empty_line(markdown);
                if first.is_empty() {
                    let _ = writeln!(out, "  - 手順: {} 文字", markdown.len());
                } else {
                    let _ = writeln!(out, "  - 手順: {first}");
                }
                if !cfg.verbose && action.risk_level >= RiskLevel::R2 {
                    if let Some(note) = first_highlight_line(markdown, first) {
                        let _ = writeln!(out, "  - 注記: {note}");
                    }
                }
                if cfg.verbose {
                    write_instruction_excerpt(&mut out, markdown, 10);
                }
            }
        }

        if !action.notes.is_empty() {
            let show_all = cfg.verbose || action.notes.len() <= 2;
            let show_n = if show_all { action.notes.len() } else { 2 };
            for note in action.notes.iter().take(show_n) {
                let _ = writeln!(out, "  - 注記: {note}");
            }
            if !show_all {
                let _ = writeln!(
                    out,
                    "  - ...（残り{}件の注記）",
                    action.notes.len().saturating_sub(show_n)
                );
            }
        }
    }
}

pub fn print_fix_candidates(actions: &[ActionPlan], cfg: &UiConfig, max_risk: RiskLevel) {
    if cfg.quiet {
        return;
    }

    let mut out = io::stdout().lock();
    let _ = writeln!(out, "掃除（対話選択） max_risk={max_risk}");

    if actions.is_empty() {
        let _ = writeln!(out, "実行可能なアクションがありません。");
        return;
    }

    let _ = writeln!(out, "\n候補:");
    for (idx, action) in actions.iter().enumerate() {
        let n = idx + 1;
        let risk = format_risk(action.risk_level, cfg.color);
        let _ = writeln!(
            out,
            "[{n}] {} [{}]（推定: {} / {}） id={}",
            action.title,
            risk,
            format_bytes(action.estimated_reclaimed_bytes),
            action_kind_label(&action.kind),
            action.id
        );
        if !action.related_findings.is_empty() {
            let _ = writeln!(out, "  - 対象: {}", action.related_findings.join(","));
        }

        match &action.kind {
            crate::core::ActionKind::TrashMove { paths }
            | crate::core::ActionKind::Delete { paths } => {
                if !paths.is_empty() {
                    let show_all = cfg.verbose || paths.len() <= 1;
                    let show_n = if show_all { paths.len() } else { 1 };
                    for p in paths.iter().take(show_n) {
                        let _ = writeln!(out, "  - パス: {p}");
                    }
                    if !show_all {
                        let _ = writeln!(out, "  - ...（残り{}件）", paths.len().saturating_sub(1));
                    }
                }
            }
            crate::core::ActionKind::RunCmd { cmd, args } => {
                let _ = writeln!(out, "  - コマンド: {}", format_cmdline(cmd, args));
            }
            crate::core::ActionKind::OpenInFinder { path } => {
                let _ = writeln!(out, "  - Finderで開く: {path}");
            }
            crate::core::ActionKind::ShowInstructions { markdown } => {
                let first = first_non_empty_line(markdown);
                if first.is_empty() {
                    let _ = writeln!(out, "  - 手順: {} 文字", markdown.len());
                } else {
                    let _ = writeln!(out, "  - 手順: {first}");
                }
                if action.risk_level >= RiskLevel::R2 {
                    if let Some(note) = first_highlight_line(markdown, first) {
                        let _ = writeln!(out, "  - 注記: {note}");
                    }
                }
                if cfg.verbose {
                    write_instruction_excerpt(&mut out, markdown, 6);
                }
            }
        }

        if !action.notes.is_empty() {
            let show_all = cfg.verbose || action.notes.len() <= 1;
            let show_n = if show_all { action.notes.len() } else { 1 };
            for note in action.notes.iter().take(show_n) {
                let _ = writeln!(out, "  - 注記: {note}");
            }
            if !show_all {
                let _ = writeln!(
                    out,
                    "  - ...（残り{}件の注記）",
                    action.notes.len().saturating_sub(show_n)
                );
            }
        }
    }
}

pub fn print_snapshots_status(report: &Report, cfg: &UiConfig) {
    if cfg.quiet {
        return;
    }

    let mut out = io::stdout().lock();
    let _ = writeln!(out, "スナップショットの状態:");
    for note in &report.summary.notes {
        let _ = writeln!(out, "- {note}");
    }

    use std::collections::HashMap;
    let actions_by_id: HashMap<&str, &ActionPlan> =
        report.actions.iter().map(|a| (a.id.as_str(), a)).collect();

    for finding in &report.findings {
        let risk = format_risk(finding.risk_level, cfg.color);
        let _ = writeln!(
            out,
            "- {} [{}]（推定: {}）",
            finding.title,
            risk,
            format_bytes(finding.estimated_bytes)
        );
        if finding.finding_type.ends_with("_UNOBSERVED") {
            if let Some(reason) = unobserved_reason(&finding.evidence) {
                let _ = writeln!(out, "  - 理由: {reason}");
            }
        }

        if !finding.recommended_actions.is_empty() {
            let _ = writeln!(out, "  - 次の手順:");
            for r in &finding.recommended_actions {
                if let Some(action) = actions_by_id.get(r.id.as_str()) {
                    let risk = format_risk(action.risk_level, cfg.color);
                    let _ = writeln!(
                        out,
                        "    - {} [{}]（{}） id={}",
                        action.title,
                        risk,
                        action_kind_label(&action.kind),
                        action.id
                    );
                    if matches!(
                        action.kind,
                        crate::core::ActionKind::ShowInstructions { .. }
                    ) && !cfg.verbose
                    {
                        if let crate::core::ActionKind::ShowInstructions { markdown } = &action.kind
                        {
                            let first = first_non_empty_line(markdown);
                            if !first.is_empty() {
                                let _ = writeln!(out, "      {first}");
                            }
                        }
                    }
                }
            }
        }

        if cfg.verbose {
            for ev in &finding.evidence {
                let _ = writeln!(out, "  - 根拠({:?}): {}", ev.kind, ev.value);
            }
        }
    }
}

fn action_kind_label(kind: &crate::core::ActionKind) -> &'static str {
    match kind {
        crate::core::ActionKind::TrashMove { .. } => "ゴミ箱へ移動（TRASH_MOVE）",
        crate::core::ActionKind::Delete { .. } => "削除（DELETE）",
        crate::core::ActionKind::RunCmd { .. } => "コマンド実行（RUN_CMD）",
        crate::core::ActionKind::OpenInFinder { .. } => "Finderで開く（OPEN_IN_FINDER）",
        crate::core::ActionKind::ShowInstructions { .. } => "手順表示（SHOW_INSTRUCTIONS）",
    }
}

fn estimate_preview_reclaim(actions: &[ActionPlan]) -> u64 {
    use std::collections::HashMap;

    let mut by_finding: HashMap<&str, u64> = HashMap::new();
    for action in actions {
        if action.risk_level <= RiskLevel::R1 {
            continue;
        }
        if action.related_findings.is_empty() {
            continue;
        }
        let bytes = action.estimated_reclaimed_bytes;
        for finding_id in &action.related_findings {
            let entry = by_finding.entry(finding_id.as_str()).or_insert(0);
            *entry = (*entry).max(bytes);
        }
    }
    by_finding.values().sum()
}

fn first_non_empty_line(markdown: &str) -> &str {
    for line in markdown.lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    ""
}

fn first_highlight_line(markdown: &str, skip: &str) -> Option<String> {
    let skip = skip.trim();
    for line in markdown.lines() {
        let line = normalize_instruction_line(line);
        if line.is_empty() || line == skip {
            continue;
        }
        if is_instruction_highlight(line) {
            return Some(line.to_string());
        }
    }
    None
}

fn normalize_instruction_line(line: &str) -> &str {
    line.trim()
        .trim_start_matches("- ")
        .trim_start_matches("* ")
        .trim_start_matches("• ")
}

fn is_instruction_highlight(line: &str) -> bool {
    line.contains("注意")
        || line.contains("慎重")
        || line.contains("影響")
        || line.contains("R2")
        || line.contains("R3")
}

fn format_cmdline(cmd: &str, args: &[String]) -> String {
    let mut out = String::from(cmd);
    for arg in args {
        out.push(' ');
        out.push_str(arg);
    }
    out
}

fn write_instruction_excerpt(out: &mut dyn Write, markdown: &str, max_lines: usize) {
    let _ = writeln!(out, "  - 手順:");

    let mut iter = markdown
        .lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.trim().is_empty());

    for _ in 0..max_lines.max(1) {
        let Some(line) = iter.next() else {
            return;
        };
        let _ = writeln!(out, "    {line}");
    }
    if iter.next().is_some() {
        let _ = writeln!(out, "    ...");
    }
}

fn prioritize_notes<'a>(notes: &'a [String]) -> Vec<&'a String> {
    let mut system_data = Vec::new();
    let mut unobserved = Vec::new();
    let mut other = Vec::new();

    for note in notes {
        if note.contains("System Data") || note.contains("システムデータ") {
            system_data.push(note);
        } else if note.starts_with("未観測:")
            || note.starts_with("未観測推定:")
            || note.starts_with("ヒント:")
        {
            unobserved.push(note);
        } else {
            other.push(note);
        }
    }

    system_data.extend(unobserved);
    system_data.extend(other);
    system_data
}

fn is_snapshot_finding_type(finding_type: &str) -> bool {
    finding_type.starts_with("TM_LOCAL_SNAPSHOTS") || finding_type.starts_with("APFS_SNAPSHOTS")
}

fn unobserved_reason(evidence: &[crate::core::Evidence]) -> Option<String> {
    for ev in evidence {
        if ev.kind != crate::core::EvidenceKind::Stat {
            continue;
        }
        let s = ev.value.trim();
        if s.is_empty() {
            continue;
        }
        return Some(truncate_middle(s, 180));
    }
    None
}

fn truncate_middle(s: &str, max_chars: usize) -> String {
    let len = s.chars().count();
    if len <= max_chars {
        return s.to_string();
    }

    let keep = max_chars.saturating_sub(3);
    let left = keep / 2;
    let right = keep.saturating_sub(left);

    let prefix: String = s.chars().take(left).collect();
    let suffix: String = s
        .chars()
        .rev()
        .take(right)
        .collect::<String>()
        .chars()
        .rev()
        .collect();

    format!("{prefix}...{suffix}")
}

fn print_findings_table(out: &mut dyn Write, findings: &[Finding], rows: usize, color: bool) {
    let label_size = "サイズ";
    let label_risk = "リスク";
    let label_conf = "確度";
    let label_title = "タイトル";

    let bytes_w = findings
        .iter()
        .take(rows)
        .map(|f| visible_width_ansi(&format_bytes(f.estimated_bytes)))
        .max()
        .unwrap_or(0)
        .max(visible_width_ansi(label_size));
    let risk_w = visible_width_ansi(label_risk).max(2);
    let conf_w = visible_width_ansi(label_conf).max(4);
    let title_w = visible_width_ansi(label_title).max(5);

    let _ = writeln!(
        out,
        "{}  {}  {}  {}",
        pad_end_display(label_size, bytes_w),
        pad_end_display(label_risk, risk_w),
        pad_start_display(label_conf, conf_w),
        label_title
    );
    let _ = writeln!(
        out,
        "{}  {}  {}  {}",
        "-".repeat(bytes_w),
        "-".repeat(risk_w),
        "-".repeat(conf_w),
        "-".repeat(title_w)
    );

    for finding in findings.iter().take(rows) {
        let size = pad_end_display(&format_bytes(finding.estimated_bytes), bytes_w);
        let risk = pad_end_ansi(&format_risk(finding.risk_level, color), risk_w);
        let conf = pad_start_display(&format!("{:.2}", finding.confidence), conf_w);
        let _ = writeln!(out, "{size}  {risk}  {conf}  {}", finding.title);
    }
}

fn risk_label(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::R0 => "R0",
        RiskLevel::R1 => "R1",
        RiskLevel::R2 => "R2",
        RiskLevel::R3 => "R3",
    }
}

fn format_risk(risk: RiskLevel, color: bool) -> String {
    let s = risk_label(risk);
    if !color {
        return s.to_string();
    }

    let code = match risk {
        RiskLevel::R0 => "90",
        RiskLevel::R1 => "32",
        RiskLevel::R2 => "33",
        RiskLevel::R3 => "31",
    };
    format!("\x1b[{code}m{s}\x1b[0m")
}

fn pad_end_ansi(s: &str, width: usize) -> String {
    let w = visible_width_ansi(s);
    if w >= width {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(width - w))
}

fn pad_end_display(s: &str, width: usize) -> String {
    let w = visible_width_ansi(s);
    if w >= width {
        return s.to_string();
    }
    format!("{s}{}", " ".repeat(width - w))
}

fn pad_start_display(s: &str, width: usize) -> String {
    let w = visible_width_ansi(s);
    if w >= width {
        return s.to_string();
    }
    format!("{}{}", " ".repeat(width - w), s)
}

fn visible_width_ansi(s: &str) -> usize {
    let mut width: usize = 0;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if chars.peek() == Some(&'[') {
                let _ = chars.next();
                while let Some(ch2) = chars.next() {
                    if ch2 == 'm' {
                        break;
                    }
                }
                continue;
            }
        }
        width = width.saturating_add(UnicodeWidthChar::width(ch).unwrap_or(0));
    }
    width
}

pub fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    let b = bytes as f64;
    if b < KB {
        return format!("{bytes} B");
    }
    if b < MB {
        return format!("{:.1} KiB", b / KB);
    }
    if b < GB {
        return format!("{:.1} MiB", b / MB);
    }
    if b < TB {
        return format!("{:.1} GiB", b / GB);
    }
    format!("{:.1} TiB", b / TB)
}
