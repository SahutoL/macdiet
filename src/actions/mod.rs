use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Result, anyhow};

use crate::core::{ActionKind, ActionPlan, RiskLevel};
use crate::platform::CommandOutput;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AllowlistedRunCmdSpec {
    pub confirm_token: &'static str,
    pub final_confirm_token: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AllowlistedRunCmdOutcome {
    Ok,
    OkWithWarnings(String),
    Error(String),
}

pub fn allowlisted_run_cmd(action: &ActionPlan) -> Option<AllowlistedRunCmdSpec> {
    let ActionKind::RunCmd { cmd, args } = &action.kind else {
        return None;
    };

    if action.id == "coresimulator-simctl-delete-unavailable"
        && action.risk_level == RiskLevel::R2
        && cmd == "xcrun"
        && args
            .iter()
            .map(String::as_str)
            .eq(["simctl", "delete", "unavailable"])
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "unavailable",
            final_confirm_token: "run",
        });
    }

    if action.id == "docker-builder-prune"
        && action.risk_level == RiskLevel::R2
        && cmd == "docker"
        && args.iter().map(String::as_str).eq(["builder", "prune"])
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "builder-prune",
            final_confirm_token: "run",
        });
    }

    if action.id == "docker-system-prune"
        && action.risk_level == RiskLevel::R2
        && cmd == "docker"
        && args.iter().map(String::as_str).eq(["system", "prune"])
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "system-prune",
            final_confirm_token: "run",
        });
    }

    if action.id == "docker-storage-df"
        && action.risk_level == RiskLevel::R2
        && cmd == "docker"
        && args.iter().map(String::as_str).eq(["system", "df"])
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "df",
            final_confirm_token: "run",
        });
    }

    if action.id == "homebrew-cache-cleanup"
        && action.risk_level == RiskLevel::R1
        && cmd == "brew"
        && args.iter().map(String::as_str).eq(["cleanup"])
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "cleanup",
            final_confirm_token: "run",
        });
    }

    if action.id == "npm-cache-cleanup"
        && action.risk_level == RiskLevel::R1
        && cmd == "npm"
        && args
            .iter()
            .map(String::as_str)
            .eq(["cache", "clean", "--force"])
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "npm",
            final_confirm_token: "run",
        });
    }

    if action.id == "yarn-cache-cleanup"
        && action.risk_level == RiskLevel::R1
        && cmd == "yarn"
        && args.iter().map(String::as_str).eq(["cache", "clean"])
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "yarn",
            final_confirm_token: "run",
        });
    }

    if action.id == "pnpm-store-prune"
        && action.risk_level == RiskLevel::R1
        && cmd == "pnpm"
        && args.iter().map(String::as_str).eq(["store", "prune"])
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "pnpm",
            final_confirm_token: "run",
        });
    }

    if action.id == "homebrew-cellar-permissions-chmod"
        && action.risk_level == RiskLevel::R2
        && cmd == "chmod"
        && args.len() >= 3
        && args[0] == "-R"
        && args[1] == "u+rwX"
        && args[2..]
            .iter()
            .all(|p| p.starts_with("/opt/homebrew/Cellar/") || p.starts_with("/usr/local/Cellar/"))
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "chmod",
            final_confirm_token: "run",
        });
    }

    if action.id == "homebrew-cellar-permissions-chown"
        && action.risk_level == RiskLevel::R3
        && cmd == "chown"
        && args.len() >= 3
        && args[0] == "-R"
        && is_safe_posix_owner(&args[1])
        && args[2..]
            .iter()
            .all(|p| p.starts_with("/opt/homebrew/Cellar/") || p.starts_with("/usr/local/Cellar/"))
    {
        return Some(AllowlistedRunCmdSpec {
            confirm_token: "chown",
            final_confirm_token: "run",
        });
    }

    None
}

pub fn evaluate_allowlisted_run_cmd_output(
    action: &ActionPlan,
    output: &CommandOutput,
) -> AllowlistedRunCmdOutcome {
    if allowlisted_run_cmd(action).is_none() {
        return AllowlistedRunCmdOutcome::Error(format!(
            "許可リスト外の RUN_CMD です（action_id={}）",
            action.id
        ));
    }

    match action.id.as_str() {
        "docker-builder-prune" | "docker-system-prune" => {
            if output.exit_code == 0 {
                return AllowlistedRunCmdOutcome::Ok;
            }

            if output.stderr.contains("Cannot connect to the Docker daemon")
                || output.stderr.contains("Is the docker daemon running")
            {
                return AllowlistedRunCmdOutcome::Error(
                    "Docker daemon に接続できません。Docker Desktop を起動し、`docker system df` が動作することを確認してから再試行してください。"
                        .to_string(),
                );
            }

            if output.stderr.contains("permission denied") || output.stderr.contains("Permission denied")
            {
                return AllowlistedRunCmdOutcome::Error(
                    "Docker の実行が権限不足で失敗しました。Docker Desktop の設定・権限（グループ/ソケット）を確認してから再試行してください。"
                        .to_string(),
                );
            }

            AllowlistedRunCmdOutcome::Error(format!(
                "コマンドが失敗しました（exit_code={}）",
                output.exit_code
            ))
        }
        "homebrew-cache-cleanup" => {
            if output.exit_code == 0 {
                return AllowlistedRunCmdOutcome::Ok;
            }

            if output.exit_code == 1 && !brew_output_has_hard_error(output) {
                return AllowlistedRunCmdOutcome::OkWithWarnings(
                    "`brew cleanup` は警告があると exit_code=1 を返すことがあります。ログの stderr（Warning）を確認してください。"
                        .to_string(),
                );
            }

            if output
                .stderr
                .contains("Running Homebrew as root is extremely dangerous")
            {
                return AllowlistedRunCmdOutcome::Error(
                    "`brew cleanup` は root では実行できません（Homebrew の安全制約）。`sudo` で macdiet を起動している場合は、`sudo` を外して再試行してください。".to_string(),
                );
            }

            let error_line = brew_first_error_line(output);
            let permission_paths = brew_permission_fix_paths(output);
            let looks_like_permission_issue = output.stderr.contains("Fix your permissions on:")
                || output.stderr.contains("Permission denied");

            let mut msg = format!(
                "`brew cleanup` が失敗しました（exit_code={}）",
                output.exit_code
            );
            if looks_like_permission_issue {
                msg.push_str("（権限不足の可能性）");
            }
            if let Some(line) = error_line {
                msg.push_str(&format!(": {line}"));
            }
            if !permission_paths.is_empty() {
                msg.push_str(&format!(
                    " 修正が必要なパス: {}",
                    permission_paths.join(", ")
                ));
            }
            msg.push_str(" ヒント: `brew doctor` を実行し、表示される指示に従って所有者/権限を修正してください。");

            AllowlistedRunCmdOutcome::Error(msg)
        }
        "homebrew-cellar-permissions-chmod" => {
            if output.exit_code == 0 {
                AllowlistedRunCmdOutcome::Ok
            } else if output.stderr.contains("Operation not permitted")
                || output.stderr.contains("Permission denied")
            {
                AllowlistedRunCmdOutcome::Error(
                    "`chmod` が失敗しました。対象が root 所有などの可能性があります。必要なら Fix 画面で最大リスクを R3 にしてから「所有者修復（chown）」を試してください（または `sudo macdiet ui` で起動してください）。".to_string(),
                )
            } else {
                AllowlistedRunCmdOutcome::Error(format!(
                    "コマンドが失敗しました（exit_code={}）",
                    output.exit_code
                ))
            }
        }
        "homebrew-cellar-permissions-chown" => {
            if output.exit_code == 0 {
                AllowlistedRunCmdOutcome::Ok
            } else if output.stderr.contains("Operation not permitted")
                || output.stderr.contains("Permission denied")
            {
                AllowlistedRunCmdOutcome::Error(
                    "`chown` が失敗しました。多くの場合 `sudo` が必要です（ツールは自動で sudo しません）。`sudo macdiet ui` で起動してから再試行してください。".to_string(),
                )
            } else {
                AllowlistedRunCmdOutcome::Error(format!(
                    "コマンドが失敗しました（exit_code={}）",
                    output.exit_code
                ))
            }
        }
        _ => {
            if output.exit_code == 0 {
                AllowlistedRunCmdOutcome::Ok
            } else {
                AllowlistedRunCmdOutcome::Error(format!(
                    "コマンドが失敗しました（exit_code={}）",
                    output.exit_code
                ))
            }
        }
    }
}

pub fn suggest_allowlisted_run_cmd_repair_action(
    action: &ActionPlan,
    output: &CommandOutput,
) -> Option<ActionPlan> {
    suggest_allowlisted_run_cmd_repair_actions(action, output)
        .into_iter()
        .next()
}

fn brew_output_has_hard_error(output: &CommandOutput) -> bool {
    for s in [&output.stdout, &output.stderr] {
        if s.contains("Error:") {
            return true;
        }
        for line in s.lines() {
            let line = line.trim_start();
            if line.starts_with("fatal:") {
                return true;
            }
        }
    }
    false
}

fn brew_first_error_line(output: &CommandOutput) -> Option<String> {
    for s in [&output.stderr, &output.stdout] {
        for line in s.lines() {
            let line = line.trim_start();
            if line.starts_with("Error:") {
                return Some(line.trim().to_string());
            }
        }
    }
    None
}

fn brew_permission_fix_paths(output: &CommandOutput) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let mut in_section = false;

    for line in output.stderr.lines() {
        if !in_section && line.contains("Fix your permissions on:") {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if line.starts_with(' ') || line.starts_with('\t') || trimmed.starts_with('/') {
            out.push(trimmed.to_string());
        } else {
            break;
        }
    }

    out
}

pub fn suggest_allowlisted_run_cmd_repair_actions(
    action: &ActionPlan,
    output: &CommandOutput,
) -> Vec<ActionPlan> {
    if action.id != "homebrew-cache-cleanup" {
        return vec![];
    }
    if output.exit_code == 0 {
        return vec![];
    }

    let paths = brew_permission_fix_paths(output);
    if paths.is_empty() {
        return vec![];
    }
    if !paths
        .iter()
        .all(|p| p.starts_with("/opt/homebrew/Cellar/") || p.starts_with("/usr/local/Cellar/"))
    {
        return vec![];
    }

    let mut out = Vec::<ActionPlan>::new();

    let mut chmod_args = vec!["-R".to_string(), "u+rwX".to_string()];
    chmod_args.extend(paths.iter().cloned());
    out.push(ActionPlan {
        id: "homebrew-cellar-permissions-chmod".to_string(),
        title: "Homebrew Cellar の権限を修復（`chmod -R u+rwX`）（R2）".to_string(),
        risk_level: RiskLevel::R2,
        estimated_reclaimed_bytes: 0,
        related_findings: action.related_findings.clone(),
        kind: ActionKind::RunCmd {
            cmd: "chmod".to_string(),
            args: chmod_args,
        },
        notes: vec![
            "目的: Homebrew の権限エラー（Could not cleanup old kegs 等）を解消するため、対象パスをユーザー書き込み可能にします。"
                .to_string(),
            "注: 所有者が root などの場合は、この操作だけでは解決せず `sudo` が必要になることがあります。".to_string(),
            "次: `brew cleanup` をもう一度実行してください。".to_string(),
        ],
    });

    let looks_like_ownership_issue = output.stderr.contains("Permission denied @ apply2files")
        || output.stderr.contains("Operation not permitted");
    if looks_like_ownership_issue {
        if let Some(owner) = detect_preferred_owner_for_repair() {
            let mut chown_args = vec!["-R".to_string(), owner.clone()];
            chown_args.extend(paths.iter().cloned());
            out.push(ActionPlan {
                id: "homebrew-cellar-permissions-chown".to_string(),
                title: format!(
                    "Homebrew Cellar の所有者を修復（`sudo chown -R {owner}`）（R3）"
                ),
                risk_level: RiskLevel::R3,
                estimated_reclaimed_bytes: 0,
                related_findings: action.related_findings.clone(),
                kind: ActionKind::RunCmd {
                    cmd: "chown".to_string(),
                    args: chown_args,
                },
                notes: vec![
                    "目的: 所有者が root 等になっている場合に、削除/整理できる状態へ戻します。"
                        .to_string(),
                    "注: これは R3（権限が必要）です。ツールは自動で sudo しないため、実行するにはユーザーが明示的に `sudo macdiet ui` で起動してください。".to_string(),
                    "次: `brew cleanup` をもう一度実行してください。".to_string(),
                ],
            });
        }
    }

    out
}

fn detect_preferred_owner_for_repair() -> Option<String> {
    let mut candidates = Vec::<String>::new();
    for key in ["SUDO_USER", "USER", "LOGNAME"] {
        if let Ok(v) = std::env::var(key) {
            let v = v.trim();
            if !v.is_empty() {
                candidates.push(v.to_string());
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        if let Some(name) = std::path::Path::new(&home)
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::trim)
        {
            if !name.is_empty() {
                candidates.push(name.to_string());
            }
        }
    }
    let mut root_fallback = None::<String>;
    for owner in candidates {
        if !is_safe_posix_owner(&owner) {
            continue;
        }
        if owner == "root" {
            root_fallback = Some(owner);
            continue;
        }
        return Some(owner);
    }
    root_fallback
}

fn is_safe_posix_owner(owner: &str) -> bool {
    if owner.is_empty() {
        return false;
    }
    if owner.starts_with('-') {
        return false;
    }
    if owner.contains(':') {
        return false;
    }
    owner.chars().all(|c| {
        c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
    })
}

pub fn run_allowlisted_cmd(
    action: &ActionPlan,
    timeout: Duration,
) -> Result<crate::platform::CommandOutput> {
    let Some(_spec) = allowlisted_run_cmd(action) else {
        return Err(anyhow!(
            "v0.1 では、この RUN_CMD は実行許可リストに含まれていません（action_id={}）",
            action.id
        ));
    };
    let ActionKind::RunCmd { cmd, args } = &action.kind else {
        return Err(anyhow!(
            "アクション種別が RUN_CMD ではありません（action_id={}）",
            action.id
        ));
    };

    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    match action.id.as_str() {
        "homebrew-cache-cleanup"
        | "coresimulator-simctl-delete-unavailable"
        | "docker-builder-prune"
        | "docker-storage-df"
        | "docker-system-prune"
        | "npm-cache-cleanup"
        | "yarn-cache-cleanup"
        | "pnpm-store-prune" => crate::platform::run_command_invoking_user(cmd, &args_ref, timeout),
        _ => crate::platform::run_command(cmd, &args_ref, timeout),
    }
}

#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    pub moved: Vec<TrashMoveRecord>,
    pub skipped_missing: Vec<PathBuf>,
    pub errors: Vec<TrashMoveError>,
}

#[derive(Debug, Clone)]
pub struct TrashMoveRecord {
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TrashMoveError {
    pub path: PathBuf,
    pub error: String,
}

pub fn validate_actions(actions: &[ActionPlan], home_dir: &Path) -> Result<()> {
    for action in actions {
        validate_action(action, home_dir)?;
    }
    Ok(())
}

pub fn apply_trash_moves(actions: &[ActionPlan], home_dir: &Path) -> Result<ApplyOutcome> {
    validate_actions(actions, home_dir)?;

    let mut moved = Vec::new();
    let mut skipped_missing = Vec::new();
    let mut errors = Vec::new();

    for action in actions {
        let ActionKind::TrashMove { paths } = &action.kind else {
            continue;
        };
        for path in paths {
            let src = validate_trash_target(path, home_dir)?;
            if !src.exists() {
                skipped_missing.push(src);
                continue;
            }
            match move_to_trash(&src, home_dir) {
                Ok(dest) => moved.push(TrashMoveRecord {
                    from: src,
                    to: dest,
                }),
                Err(err) => errors.push(TrashMoveError {
                    path: src,
                    error: err.to_string(),
                }),
            }
        }
    }

    Ok(ApplyOutcome {
        moved,
        skipped_missing,
        errors,
    })
}

fn validate_action(action: &ActionPlan, home_dir: &Path) -> Result<()> {
    match &action.kind {
        ActionKind::TrashMove { paths } => {
            for p in paths {
                validate_trash_target(p, home_dir)?;
            }
            Ok(())
        }
        ActionKind::Delete { .. } => Err(anyhow!(
            "v0.1 では DELETE は許可されていません（TRASH_MOVE を使用してください）"
        )),
        ActionKind::RunCmd { .. } => Ok(()),
        ActionKind::OpenInFinder { .. } => Ok(()),
        ActionKind::ShowInstructions { .. } => Ok(()),
    }
}

fn validate_trash_target(path: &str, home_dir: &Path) -> Result<PathBuf> {
    let expanded = expand_tilde(path, home_dir);
    if !expanded.is_absolute() {
        return Err(anyhow!(
            "パスは絶対パス（または ~/ から始まる形式）で指定してください: {path}"
        ));
    }
    if expanded == Path::new("/") {
        return Err(anyhow!("ルートパスに対する操作は拒否します: {path}"));
    }
    if !expanded.starts_with(home_dir) {
        return Err(anyhow!("パスは home 配下である必要があります: {path}"));
    }

    let allowed = allowed_trash_targets(home_dir);
    if !allowed.iter().any(|p| p == &expanded) {
        let prefixes = allowed_trash_target_prefixes(home_dir);
        let allowed_by_prefix = prefixes
            .iter()
            .any(|p| expanded.starts_with(p) && &expanded != p);
        if !allowed_by_prefix {
            return Err(anyhow!(
                "TRASH_MOVE の許可リストに含まれていないパスです: {path}"
            ));
        }
    }

    Ok(expanded)
}

fn allowed_trash_targets(home_dir: &Path) -> Vec<PathBuf> {
    vec![
        home_dir.join("Library/Developer/Xcode/DerivedData"),
        home_dir.join("Library/Developer/Shared/Documentation/DocSets"),
        home_dir.join("Library/Developer/Xcode/iOS Device Logs"),
        home_dir.join("Library/Caches/Homebrew"),
        home_dir.join(".cargo/registry"),
        home_dir.join(".cargo/git"),
        home_dir.join(".gradle/caches"),
        home_dir.join(".npm"),
        home_dir.join("Library/Caches/Yarn"),
        home_dir.join("Library/pnpm/store"),
        home_dir.join(".pnpm-store"),
    ]
}

fn allowed_trash_target_prefixes(home_dir: &Path) -> Vec<PathBuf> {
    vec![
        home_dir.join("Library/Developer/Xcode/Archives"),
        home_dir.join("Library/Developer/Xcode/iOS DeviceSupport"),
        home_dir.join("Library/Developer/CoreSimulator/Devices"),
    ]
}

fn expand_tilde(path: &str, home_dir: &Path) -> PathBuf {
    let path = path.trim();
    if path == "~" {
        return home_dir.to_path_buf();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir.join(rest);
    }
    PathBuf::from(path)
}

fn move_to_trash(src: &Path, home_dir: &Path) -> Result<PathBuf> {
    let trash_dir = home_dir.join(".Trash");
    std::fs::create_dir_all(&trash_dir)
        .map_err(|e| anyhow!("~/.Trash の作成に失敗しました: {e}"))?;

    let file_name = src.file_name().ok_or_else(|| {
        anyhow!(
            "ファイル名のないパスはゴミ箱へ移動できません: {}",
            src.display()
        )
    })?;

    let mut dest = trash_dir.join(file_name);
    if dest.exists() {
        dest = unique_dest(&trash_dir, file_name)?;
    }

    std::fs::rename(src, &dest).map_err(|e| {
        anyhow!(
            "ゴミ箱へ移動できませんでした: {} -> {}: {e}",
            src.display(),
            dest.display()
        )
    })?;

    Ok(dest)
}

fn unique_dest(trash_dir: &Path, file_name: &std::ffi::OsStr) -> Result<PathBuf> {
    let base = file_name.to_string_lossy();
    for i in 1..=1000u32 {
        let candidate = trash_dir.join(format!("{base}.macdiet-{i}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(anyhow!(
        "ゴミ箱内で一意な移動先を決定できませんでした: {base}"
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::core::{ActionKind, ActionPlan, RiskLevel};

    #[test]
    fn validate_trash_move_allows_whitelisted_path() {
        let home = PathBuf::from("/Users/test");
        let action = ActionPlan {
            id: "a".to_string(),
            title: "t".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec!["~/Library/Developer/Xcode/DerivedData".to_string()],
            },
            notes: vec![],
        };
        validate_actions(&[action], &home).expect("should validate");
    }

    #[test]
    fn validate_trash_move_allows_xcode_docsets() {
        let home = PathBuf::from("/Users/test");
        let action = ActionPlan {
            id: "a".to_string(),
            title: "t".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec!["~/Library/Developer/Shared/Documentation/DocSets".to_string()],
            },
            notes: vec![],
        };
        validate_actions(&[action], &home).expect("should validate");
    }

    #[test]
    fn validate_trash_move_allows_xcode_archives_subdir() {
        let home = PathBuf::from("/Users/test");
        let action = ActionPlan {
            id: "a".to_string(),
            title: "t".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec![
                    "~/Library/Developer/Xcode/Archives/2026-01-01/MyApp.xcarchive".to_string(),
                ],
            },
            notes: vec![],
        };
        validate_actions(&[action], &home).expect("should validate");
    }

    #[test]
    fn validate_trash_move_blocks_xcode_archives_root_dir() {
        let home = PathBuf::from("/Users/test");
        let action = ActionPlan {
            id: "a".to_string(),
            title: "t".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec!["~/Library/Developer/Xcode/Archives".to_string()],
            },
            notes: vec![],
        };
        assert!(validate_actions(&[action], &home).is_err());
    }

    #[test]
    fn validate_trash_move_blocks_outside_home() {
        let home = PathBuf::from("/Users/test");
        let action = ActionPlan {
            id: "a".to_string(),
            title: "t".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec!["/etc".to_string()],
            },
            notes: vec![],
        };
        assert!(validate_actions(&[action], &home).is_err());
    }

    #[test]
    fn validate_trash_move_blocks_unwhitelisted_under_home() {
        let home = PathBuf::from("/Users/test");
        let action = ActionPlan {
            id: "a".to_string(),
            title: "t".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec!["~/Downloads".to_string()],
            },
            notes: vec![],
        };
        assert!(validate_actions(&[action], &home).is_err());
    }

    #[test]
    fn apply_trash_moves_moves_directory_into_trash() {
        static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

        let temp = std::env::temp_dir();
        let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
        let uniq = format!("macdiet-test-{}-{seq}", std::process::id());
        let home = temp.join(uniq);
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".Trash")).unwrap();

        let src = home.join("Library/Developer/Xcode/DerivedData");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("file.txt"), b"hello").unwrap();

        let action = ActionPlan {
            id: "a".to_string(),
            title: "t".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec!["~/Library/Developer/Xcode/DerivedData".to_string()],
            },
            notes: vec![],
        };

        let outcome = apply_trash_moves(&[action], &home).expect("apply");
        assert_eq!(outcome.skipped_missing.len(), 0);
        assert_eq!(outcome.moved.len(), 1);
        assert!(!src.exists());
        assert!(outcome.moved[0].to.starts_with(home.join(".Trash")));

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn allowlisted_run_cmd_accepts_simctl_delete_unavailable() {
        let action = ActionPlan {
            id: "coresimulator-simctl-delete-unavailable".to_string(),
            title: "Delete unavailable simulators".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "xcrun".to_string(),
                args: vec![
                    "simctl".to_string(),
                    "delete".to_string(),
                    "unavailable".to_string(),
                ],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "unavailable");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_accepts_docker_builder_prune() {
        let action = ActionPlan {
            id: "docker-builder-prune".to_string(),
            title: "docker builder prune".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "docker".to_string(),
                args: vec!["builder".to_string(), "prune".to_string()],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "builder-prune");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_accepts_docker_system_prune() {
        let action = ActionPlan {
            id: "docker-system-prune".to_string(),
            title: "docker system prune".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "docker".to_string(),
                args: vec!["system".to_string(), "prune".to_string()],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "system-prune");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_accepts_docker_system_df() {
        let action = ActionPlan {
            id: "docker-storage-df".to_string(),
            title: "docker system df".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "docker".to_string(),
                args: vec!["system".to_string(), "df".to_string()],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "df");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_rejects_non_matching_command() {
        let action = ActionPlan {
            id: "coresimulator-simctl-delete-unavailable".to_string(),
            title: "Delete unavailable simulators".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "sh".to_string(),
                args: vec!["-c".to_string(), "echo nope".to_string()],
            },
            notes: vec![],
        };

        assert!(allowlisted_run_cmd(&action).is_none());
    }

    #[test]
    fn allowlisted_run_cmd_accepts_brew_cleanup() {
        let action = ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "brew cleanup".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "cleanup");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_accepts_npm_cache_cleanup() {
        let action = ActionPlan {
            id: "npm-cache-cleanup".to_string(),
            title: "npm cache clean --force".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "npm".to_string(),
                args: vec![
                    "cache".to_string(),
                    "clean".to_string(),
                    "--force".to_string(),
                ],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "npm");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_accepts_yarn_cache_cleanup() {
        let action = ActionPlan {
            id: "yarn-cache-cleanup".to_string(),
            title: "yarn cache clean".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "yarn".to_string(),
                args: vec!["cache".to_string(), "clean".to_string()],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "yarn");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_accepts_pnpm_store_prune() {
        let action = ActionPlan {
            id: "pnpm-store-prune".to_string(),
            title: "pnpm store prune".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "pnpm".to_string(),
                args: vec!["store".to_string(), "prune".to_string()],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "pnpm");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_accepts_homebrew_cellar_permissions_chmod() {
        let action = ActionPlan {
            id: "homebrew-cellar-permissions-chmod".to_string(),
            title: "chmod cellar".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "chmod".to_string(),
                args: vec![
                    "-R".to_string(),
                    "u+rwX".to_string(),
                    "/opt/homebrew/Cellar/python@3.13/3.13.2".to_string(),
                ],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "chmod");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn allowlisted_run_cmd_accepts_homebrew_cellar_permissions_chown() {
        let action = ActionPlan {
            id: "homebrew-cellar-permissions-chown".to_string(),
            title: "chown cellar".to_string(),
            risk_level: RiskLevel::R3,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "chown".to_string(),
                args: vec![
                    "-R".to_string(),
                    "testuser".to_string(),
                    "/opt/homebrew/Cellar/python@3.13/3.13.2".to_string(),
                ],
            },
            notes: vec![],
        };

        let spec = allowlisted_run_cmd(&action).expect("allowlisted");
        assert_eq!(spec.confirm_token, "chown");
        assert_eq!(spec.final_confirm_token, "run");
    }

    #[test]
    fn evaluate_allowlisted_run_cmd_output_treats_brew_cleanup_exit_1_as_warning_without_error() {
        let action = ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "brew cleanup".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![],
        };
        let out = CommandOutput {
            exit_code: 1,
            stdout:
                "Removing: ...\n==> This operation has freed approximately 1.0MB of disk space.\n"
                    .to_string(),
            stderr: "Warning: Skipping foo: most recent version 1.2.3 not installed\n".to_string(),
        };

        match evaluate_allowlisted_run_cmd_output(&action, &out) {
            AllowlistedRunCmdOutcome::OkWithWarnings(_) => {}
            other => panic!("expected OkWithWarnings, got: {other:?}"),
        }
    }

    #[test]
    fn evaluate_allowlisted_run_cmd_output_treats_brew_cleanup_exit_1_with_error_as_error() {
        let action = ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "brew cleanup".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![],
        };
        let out = CommandOutput {
            exit_code: 1,
            stdout: "Removing: ...\n==> This operation has freed approximately 1.0MB of disk space.\n"
                .to_string(),
            stderr: "Warning: Skipping foo: most recent version 1.2.3 not installed\nError: Could not cleanup old kegs! Fix your permissions on:\n  /opt/homebrew/Cellar/python@3.13/3.13.2\n"
                .to_string(),
        };

        match evaluate_allowlisted_run_cmd_output(&action, &out) {
            AllowlistedRunCmdOutcome::Error(msg) => {
                assert!(msg.contains("Fix your permissions on:"), "msg={msg}");
                assert!(
                    msg.contains("/opt/homebrew/Cellar/python@3.13/3.13.2"),
                    "msg={msg}"
                );
                assert!(msg.contains("brew doctor"), "msg={msg}");
            }
            other => panic!("expected Error, got: {other:?}"),
        }
    }

    #[test]
    fn suggest_allowlisted_run_cmd_repair_action_suggests_chmod_for_brew_permissions_error() {
        let action = ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "brew cleanup".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["homebrew-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![],
        };
        let out = CommandOutput {
            exit_code: 1,
            stdout: "".to_string(),
            stderr: "Error: Could not cleanup old kegs! Fix your permissions on:\n  /opt/homebrew/Cellar/python@3.13/3.13.2\n".to_string(),
        };

        let repair = suggest_allowlisted_run_cmd_repair_action(&action, &out).expect("repair");
        assert_eq!(repair.id, "homebrew-cellar-permissions-chmod");
        assert_eq!(repair.risk_level, RiskLevel::R2);
        match &repair.kind {
            ActionKind::RunCmd { cmd, args } => {
                assert_eq!(cmd, "chmod");
                assert!(
                    args.iter()
                        .map(String::as_str)
                        .any(|s| s == "/opt/homebrew/Cellar/python@3.13/3.13.2")
                );
            }
            other => panic!("expected RunCmd, got: {other:?}"),
        }
    }

    #[test]
    fn suggest_allowlisted_run_cmd_repair_actions_suggests_chown_when_permission_denied() {
        let action = ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "brew cleanup".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["homebrew-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![],
        };
        let out = CommandOutput {
            exit_code: 1,
            stdout: "".to_string(),
            stderr: "Warning: Permission denied @ apply2files - /opt/homebrew/Cellar/python@3.13/3.13.2/foo\nError: Could not cleanup old kegs! Fix your permissions on:\n  /opt/homebrew/Cellar/python@3.13/3.13.2\n".to_string(),
        };

        let repairs = suggest_allowlisted_run_cmd_repair_actions(&action, &out);
        assert!(
            repairs.iter().any(|a| a.id == "homebrew-cellar-permissions-chmod"),
            "repairs={repairs:?}"
        );
        assert!(
            repairs.iter().any(|a| a.id == "homebrew-cellar-permissions-chown"),
            "repairs={repairs:?}"
        );
    }
}
