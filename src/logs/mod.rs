use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::actions::ApplyOutcome;
use crate::core::{ActionKind, ActionPlan, RiskLevel};

const MAX_CMD_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize)]
struct FixApplyLog {
    schema_version: &'static str,
    tool_version: String,
    command: &'static str,
    started_at: String,
    finished_at: String,
    max_risk: String,
    status: String,
    rollback_hint: String,
    actions: Vec<FixApplyAction>,
    outcome: FixApplyOutcome,
}

#[derive(Debug, Serialize)]
struct FixApplyAction {
    id: String,
    title: String,
    risk_level: String,
    kind: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    paths: Vec<String>,
    rollback_possible: bool,
}

#[derive(Debug, Serialize)]
struct FixApplyOutcome {
    moved: Vec<FixApplyMoved>,
    skipped_missing: Vec<String>,
    errors: Vec<FixApplyError>,
}

#[derive(Debug, Serialize)]
struct FixApplyMoved {
    from: String,
    to: String,
}

#[derive(Debug, Serialize)]
struct FixApplyError {
    path: String,
    error: String,
}

#[derive(Debug, Serialize)]
struct CommandAttemptLog {
    cmd: String,
    args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    stdout: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct SnapshotsThinLog {
    schema_version: &'static str,
    tool_version: String,
    command: &'static str,
    started_at: String,
    finished_at: String,
    status: String,
    bytes: u64,
    urgency: u8,
    attempt: CommandAttemptLog,
}

#[derive(Debug, Serialize)]
struct SnapshotsDeleteLog {
    schema_version: &'static str,
    tool_version: String,
    command: &'static str,
    started_at: String,
    finished_at: String,
    status: String,
    requested_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_uuid: Option<String>,
    list_attempt: CommandAttemptLog,
    #[serde(skip_serializing_if = "Option::is_none")]
    delete_attempt: Option<CommandAttemptLog>,
}

#[derive(Debug, Serialize)]
struct FixRunCmdLog {
    schema_version: &'static str,
    tool_version: String,
    command: &'static str,
    started_at: String,
    finished_at: String,
    status: String,
    action_id: String,
    action_title: String,
    risk_level: String,
    attempt: CommandAttemptLog,
}

pub fn logs_dir(home_dir: &Path) -> PathBuf {
    home_dir.join(".config/macdiet/logs")
}

pub fn write_fix_apply_log(
    home_dir: &Path,
    started_at: OffsetDateTime,
    finished_at: OffsetDateTime,
    max_risk: RiskLevel,
    actions: &[ActionPlan],
    outcome: &ApplyOutcome,
) -> Result<PathBuf> {
    let dir = logs_dir(home_dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("ログディレクトリの作成に失敗しました: {}", dir.display()))?;

    let pid = std::process::id();
    let ts = finished_at.unix_timestamp_nanos();
    let file_name = format!("fix-apply-{pid}-{ts}.json");
    let path = dir.join(file_name);

    let status = if outcome.errors.is_empty() {
        "ok".to_string()
    } else {
        "partial_error".to_string()
    };

    let actions: Vec<FixApplyAction> = actions
        .iter()
        .map(|a| FixApplyAction {
            id: a.id.clone(),
            title: a.title.clone(),
            risk_level: a.risk_level.to_string(),
            kind: match &a.kind {
                ActionKind::TrashMove { .. } => "TRASH_MOVE".to_string(),
                ActionKind::Delete { .. } => "DELETE".to_string(),
                ActionKind::RunCmd { .. } => "RUN_CMD".to_string(),
                ActionKind::OpenInFinder { .. } => "OPEN_IN_FINDER".to_string(),
                ActionKind::ShowInstructions { .. } => "SHOW_INSTRUCTIONS".to_string(),
            },
            paths: match &a.kind {
                ActionKind::TrashMove { paths } => paths.clone(),
                _ => vec![],
            },
            rollback_possible: matches!(a.kind, ActionKind::TrashMove { .. }),
        })
        .collect();

    let moved = outcome
        .moved
        .iter()
        .map(|m| FixApplyMoved {
            from: mask_home(&m.from, home_dir),
            to: mask_home(&m.to, home_dir),
        })
        .collect();

    let skipped_missing = outcome
        .skipped_missing
        .iter()
        .map(|p| mask_home(p, home_dir))
        .collect();

    let errors = outcome
        .errors
        .iter()
        .map(|e| FixApplyError {
            path: mask_home(&e.path, home_dir),
            error: e.error.clone(),
        })
        .collect();

    let log = FixApplyLog {
        schema_version: "1.0",
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        command: "fix",
        started_at: started_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        finished_at: finished_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        max_risk: max_risk.to_string(),
        status,
        rollback_hint: "TRASH_MOVE is recoverable via ~/.Trash (move back if needed).".to_string(),
        actions,
        outcome: FixApplyOutcome {
            moved,
            skipped_missing,
            errors,
        },
    };

    let buf = serde_json::to_vec_pretty(&log).context("ログ(JSON)のシリアライズに失敗しました")?;
    std::fs::write(&path, buf)
        .with_context(|| format!("ログの書き込みに失敗しました: {}", path.display()))?;
    Ok(path)
}

pub fn write_snapshots_thin_log(
    home_dir: &Path,
    started_at: OffsetDateTime,
    finished_at: OffsetDateTime,
    bytes: u64,
    urgency: u8,
    cmd: &str,
    args: &[String],
    output: Option<&crate::platform::CommandOutput>,
    error: Option<String>,
) -> Result<PathBuf> {
    let dir = logs_dir(home_dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("ログディレクトリの作成に失敗しました: {}", dir.display()))?;

    let pid = std::process::id();
    let ts = finished_at.unix_timestamp_nanos();
    let file_name = format!("snapshots-thin-{pid}-{ts}.json");
    let path = dir.join(file_name);

    let attempt = command_attempt(cmd, args, output, error);
    let status = match (&attempt.error, attempt.exit_code) {
        (Some(_), _) => "error".to_string(),
        (None, Some(0)) => "ok".to_string(),
        (None, Some(_)) => "error".to_string(),
        (None, None) => "error".to_string(),
    };

    let log = SnapshotsThinLog {
        schema_version: "1.0",
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        command: "snapshots thin",
        started_at: started_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        finished_at: finished_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        status,
        bytes,
        urgency,
        attempt,
    };

    let buf = serde_json::to_vec_pretty(&log).context("ログ(JSON)のシリアライズに失敗しました")?;
    std::fs::write(&path, buf)
        .with_context(|| format!("ログの書き込みに失敗しました: {}", path.display()))?;
    Ok(path)
}

pub fn write_snapshots_delete_log(
    home_dir: &Path,
    started_at: OffsetDateTime,
    finished_at: OffsetDateTime,
    requested_id: &str,
    resolved_uuid: Option<String>,
    list_cmd: &str,
    list_args: &[String],
    list_output: Option<&crate::platform::CommandOutput>,
    list_error: Option<String>,
    delete_cmd: Option<&str>,
    delete_args: Option<&[String]>,
    delete_output: Option<&crate::platform::CommandOutput>,
    delete_error: Option<String>,
) -> Result<PathBuf> {
    let dir = logs_dir(home_dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("ログディレクトリの作成に失敗しました: {}", dir.display()))?;

    let pid = std::process::id();
    let ts = finished_at.unix_timestamp_nanos();
    let file_name = format!("snapshots-delete-{pid}-{ts}.json");
    let path = dir.join(file_name);

    let list_attempt = command_attempt(list_cmd, list_args, list_output, list_error);
    let delete_attempt = match (delete_cmd, delete_args) {
        (Some(cmd), Some(args)) => Some(command_attempt(cmd, args, delete_output, delete_error)),
        _ => None,
    };

    let status = if list_attempt.error.is_some() {
        "error"
    } else if list_attempt.exit_code != Some(0) {
        "error"
    } else if let Some(delete_attempt) = &delete_attempt {
        if delete_attempt.error.is_some() {
            "error"
        } else if delete_attempt.exit_code != Some(0) {
            "error"
        } else {
            "ok"
        }
    } else {
        "error"
    }
    .to_string();

    let log = SnapshotsDeleteLog {
        schema_version: "1.0",
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        command: "snapshots delete",
        started_at: started_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        finished_at: finished_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        status,
        requested_id: requested_id.to_string(),
        resolved_uuid,
        list_attempt,
        delete_attempt,
    };

    let buf = serde_json::to_vec_pretty(&log).context("ログ(JSON)のシリアライズに失敗しました")?;
    std::fs::write(&path, buf)
        .with_context(|| format!("ログの書き込みに失敗しました: {}", path.display()))?;
    Ok(path)
}

pub fn write_fix_run_cmd_log(
    home_dir: &Path,
    started_at: OffsetDateTime,
    finished_at: OffsetDateTime,
    action: &ActionPlan,
    output: Option<&crate::platform::CommandOutput>,
    error: Option<String>,
) -> Result<PathBuf> {
    let dir = logs_dir(home_dir);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("ログディレクトリの作成に失敗しました: {}", dir.display()))?;

    let pid = std::process::id();
    let ts = finished_at.unix_timestamp_nanos();
    let file_name = format!("fix-run-cmd-{pid}-{ts}.json");
    let path = dir.join(file_name);

    let (cmd, args) = match &action.kind {
        ActionKind::RunCmd { cmd, args } => (cmd.as_str(), args),
        other => {
            return Err(anyhow::anyhow!(
                "fix run_cmd log requires ActionKind::RunCmd (got: {})",
                match other {
                    ActionKind::TrashMove { .. } => "TRASH_MOVE",
                    ActionKind::Delete { .. } => "DELETE",
                    ActionKind::RunCmd { .. } => "RUN_CMD",
                    ActionKind::OpenInFinder { .. } => "OPEN_IN_FINDER",
                    ActionKind::ShowInstructions { .. } => "SHOW_INSTRUCTIONS",
                }
            ));
        }
    };

    let attempt = command_attempt(cmd, args, output, error);
    let status = match (&attempt.error, output) {
        (Some(_), _) => "error".to_string(),
        (None, Some(out)) => match crate::actions::evaluate_allowlisted_run_cmd_output(action, out)
        {
            crate::actions::AllowlistedRunCmdOutcome::Ok => "ok".to_string(),
            crate::actions::AllowlistedRunCmdOutcome::OkWithWarnings(_) => {
                "ok_with_warnings".to_string()
            }
            crate::actions::AllowlistedRunCmdOutcome::Error(_) => "error".to_string(),
        },
        (None, None) => "error".to_string(),
    };

    let log = FixRunCmdLog {
        schema_version: "1.0",
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        command: "fix run_cmd",
        started_at: started_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        finished_at: finished_at
            .format(&Rfc3339)
            .unwrap_or_else(|_| "unknown".to_string()),
        status,
        action_id: action.id.clone(),
        action_title: action.title.clone(),
        risk_level: action.risk_level.to_string(),
        attempt,
    };

    let buf = serde_json::to_vec_pretty(&log).context("ログ(JSON)のシリアライズに失敗しました")?;
    std::fs::write(&path, buf)
        .with_context(|| format!("ログの書き込みに失敗しました: {}", path.display()))?;
    Ok(path)
}

fn mask_home(path: &Path, home_dir: &Path) -> String {
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

fn command_attempt(
    cmd: &str,
    args: &[String],
    output: Option<&crate::platform::CommandOutput>,
    error: Option<String>,
) -> CommandAttemptLog {
    let Some(output) = output else {
        return CommandAttemptLog {
            cmd: cmd.to_string(),
            args: args.to_vec(),
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error,
        };
    };

    CommandAttemptLog {
        cmd: cmd.to_string(),
        args: args.to_vec(),
        exit_code: Some(output.exit_code),
        stdout: truncate_string(&output.stdout, MAX_CMD_OUTPUT_BYTES),
        stderr: truncate_string(&output.stderr, MAX_CMD_OUTPUT_BYTES),
        error,
    }
}

fn truncate_string(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut idx = max_bytes;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx = idx.saturating_sub(1);
    }
    let head = &s[..idx];
    format!("{head}\n...(truncated, total={} bytes)", s.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{ApplyOutcome, TrashMoveError, TrashMoveRecord};
    use crate::core::{ActionKind, ActionPlan, RiskLevel};
    use crate::platform::CommandOutput;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[test]
    fn write_fix_apply_log_writes_json_with_masked_paths() {
        static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

        let temp = std::env::temp_dir();
        let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
        let uniq = format!("macdiet-log-test-{}-{seq}", std::process::id());
        let home = temp.join(uniq);
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let actions = vec![ActionPlan {
            id: "xcode-derived-data-trash".to_string(),
            title: "Move DerivedData to Trash (R1)".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 1,
            related_findings: vec!["xcode-derived-data".to_string()],
            kind: ActionKind::TrashMove {
                paths: vec!["~/Library/Developer/Xcode/DerivedData".to_string()],
            },
            notes: vec![],
        }];

        let outcome = ApplyOutcome {
            moved: vec![TrashMoveRecord {
                from: home.join("Library/Developer/Xcode/DerivedData"),
                to: home.join(".Trash/DerivedData"),
            }],
            skipped_missing: vec![],
            errors: vec![TrashMoveError {
                path: home.join("Library/Missing"),
                error: "simulated error".to_string(),
            }],
        };

        let started_at = OffsetDateTime::now_utc();
        let finished_at = started_at;
        let log_path = write_fix_apply_log(
            &home,
            started_at,
            finished_at,
            RiskLevel::R1,
            &actions,
            &outcome,
        )
        .expect("write log");

        let bytes = std::fs::read(&log_path).expect("read log");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(v.get("command").and_then(|s| s.as_str()), Some("fix"));
        assert_eq!(v.get("max_risk").and_then(|s| s.as_str()), Some("R1"));
        assert_eq!(
            v.get("status").and_then(|s| s.as_str()),
            Some("partial_error")
        );

        let moved = v
            .get("outcome")
            .and_then(|o| o.get("moved"))
            .and_then(|a| a.as_array())
            .expect("outcome.moved array");
        assert!(
            moved.iter().any(|m| m.get("from").and_then(|s| s.as_str())
                == Some("~/Library/Developer/Xcode/DerivedData")),
            "moved={moved:?}"
        );

        let errors = v
            .get("outcome")
            .and_then(|o| o.get("errors"))
            .and_then(|a| a.as_array())
            .expect("outcome.errors array");
        assert!(
            errors
                .iter()
                .any(|e| e.get("path").and_then(|s| s.as_str()) == Some("~/Library/Missing")),
            "errors={errors:?}"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn write_fix_run_cmd_log_writes_attempt() {
        static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

        let temp = std::env::temp_dir();
        let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
        let uniq = format!("macdiet-log-run-cmd-test-{}-{seq}", std::process::id());
        let home = temp.join(uniq);
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create home");

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

        let started_at = OffsetDateTime::now_utc();
        let finished_at = started_at;
        let out = CommandOutput {
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: "".to_string(),
        };
        let log_path =
            write_fix_run_cmd_log(&home, started_at, finished_at, &action, Some(&out), None)
                .expect("write log");

        let bytes = std::fs::read(&log_path).expect("read log");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(
            v.get("command").and_then(|s| s.as_str()),
            Some("fix run_cmd")
        );
        assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("ok"));
        assert_eq!(
            v.get("action_id").and_then(|s| s.as_str()),
            Some("coresimulator-simctl-delete-unavailable")
        );
        assert_eq!(v.get("risk_level").and_then(|s| s.as_str()), Some("R2"));

        let attempt = v.get("attempt").expect("attempt");
        assert_eq!(attempt.get("cmd").and_then(|s| s.as_str()), Some("xcrun"));
        assert_eq!(attempt.get("exit_code").and_then(|n| n.as_i64()), Some(0));
        let args_v = attempt
            .get("args")
            .and_then(|a| a.as_array())
            .expect("args array");
        assert!(
            args_v.iter().any(|s| s.as_str() == Some("unavailable")),
            "args={args_v:?}"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn write_fix_run_cmd_log_writes_ok_with_warnings_for_brew_cleanup_exit_1() {
        static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

        let temp = std::env::temp_dir();
        let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
        let uniq = format!("macdiet-log-brew-cleanup-test-{}-{seq}", std::process::id());
        let home = temp.join(uniq);
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create home");

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

        let started_at = OffsetDateTime::now_utc();
        let finished_at = started_at;
        let out = CommandOutput {
            exit_code: 1,
            stdout: "Removing: ...\n".to_string(),
            stderr: "Warning: Skipping foo: most recent version 1.2.3 not installed\n".to_string(),
        };
        let log_path =
            write_fix_run_cmd_log(&home, started_at, finished_at, &action, Some(&out), None)
                .expect("write log");

        let bytes = std::fs::read(&log_path).expect("read log");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(
            v.get("command").and_then(|s| s.as_str()),
            Some("fix run_cmd")
        );
        assert_eq!(
            v.get("status").and_then(|s| s.as_str()),
            Some("ok_with_warnings")
        );
        assert_eq!(
            v.get("action_id").and_then(|s| s.as_str()),
            Some("homebrew-cache-cleanup")
        );
        assert_eq!(v.get("risk_level").and_then(|s| s.as_str()), Some("R1"));
        assert_eq!(
            v.pointer("/attempt/exit_code").and_then(|n| n.as_i64()),
            Some(1)
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn write_snapshots_thin_log_writes_attempt() {
        static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

        let temp = std::env::temp_dir();
        let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
        let uniq = format!("macdiet-log-thin-test-{}-{seq}", std::process::id());
        let home = temp.join(uniq);
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let started_at = OffsetDateTime::now_utc();
        let finished_at = started_at;
        let args: Vec<String> = vec![
            "thinlocalsnapshots".to_string(),
            "/".to_string(),
            "123".to_string(),
            "2".to_string(),
        ];
        let out = CommandOutput {
            exit_code: 0,
            stdout: "ok".to_string(),
            stderr: "".to_string(),
        };
        let log_path = write_snapshots_thin_log(
            &home,
            started_at,
            finished_at,
            123,
            2,
            "tmutil",
            &args,
            Some(&out),
            None,
        )
        .expect("write log");

        let bytes = std::fs::read(&log_path).expect("read log");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(
            v.get("command").and_then(|s| s.as_str()),
            Some("snapshots thin")
        );
        assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("ok"));
        assert_eq!(v.get("bytes").and_then(|n| n.as_u64()), Some(123));
        assert_eq!(v.get("urgency").and_then(|n| n.as_u64()), Some(2));

        let attempt = v.get("attempt").expect("attempt");
        assert_eq!(attempt.get("cmd").and_then(|s| s.as_str()), Some("tmutil"));
        let args_v = attempt
            .get("args")
            .and_then(|a| a.as_array())
            .expect("args array");
        assert!(
            args_v
                .iter()
                .any(|s| s.as_str() == Some("thinlocalsnapshots")),
            "args={args_v:?}"
        );

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn write_snapshots_delete_log_writes_list_and_delete_attempts() {
        static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

        let temp = std::env::temp_dir();
        let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
        let uniq = format!("macdiet-log-delete-test-{}-{seq}", std::process::id());
        let home = temp.join(uniq);
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let started_at = OffsetDateTime::now_utc();
        let finished_at = started_at;

        let list_args: Vec<String> = vec![
            "apfs".to_string(),
            "listSnapshots".to_string(),
            "/".to_string(),
        ];
        let list_out = CommandOutput {
            exit_code: 0,
            stdout: "Snapshots (1 found)".to_string(),
            stderr: "".to_string(),
        };

        let delete_args: Vec<String> = vec![
            "apfs".to_string(),
            "deleteSnapshot".to_string(),
            "/".to_string(),
            "-uuid".to_string(),
            "00000000-0000-0000-0000-000000000000".to_string(),
        ];
        let delete_out = CommandOutput {
            exit_code: 0,
            stdout: "OK".to_string(),
            stderr: "".to_string(),
        };

        let log_path = write_snapshots_delete_log(
            &home,
            started_at,
            finished_at,
            "name-or-uuid",
            Some("00000000-0000-0000-0000-000000000000".to_string()),
            "diskutil",
            &list_args,
            Some(&list_out),
            None,
            Some("diskutil"),
            Some(&delete_args),
            Some(&delete_out),
            None,
        )
        .expect("write log");

        let bytes = std::fs::read(&log_path).expect("read log");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("parse json");
        assert_eq!(
            v.get("command").and_then(|s| s.as_str()),
            Some("snapshots delete")
        );
        assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("ok"));
        assert_eq!(
            v.get("requested_id").and_then(|s| s.as_str()),
            Some("name-or-uuid")
        );
        assert_eq!(
            v.get("resolved_uuid").and_then(|s| s.as_str()),
            Some("00000000-0000-0000-0000-000000000000")
        );

        let list_attempt = v.get("list_attempt").expect("list_attempt");
        assert_eq!(
            list_attempt.get("cmd").and_then(|s| s.as_str()),
            Some("diskutil")
        );

        let delete_attempt = v
            .get("delete_attempt")
            .and_then(|a| a.as_object())
            .expect("delete_attempt");
        assert_eq!(
            delete_attempt.get("cmd").and_then(|s| s.as_str()),
            Some("diskutil")
        );

        let _ = std::fs::remove_dir_all(&home);
    }
}
