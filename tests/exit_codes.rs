use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

fn macdiet_cmd(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_macdiet"));
    cmd.env("HOME", home);
    cmd.env_remove("MACDIET_CONFIG");
    cmd.env_remove("MACDIET_UI_COLOR");
    cmd.env_remove("MACDIET_UI_MAX_TABLE_ROWS");
    cmd.env_remove("MACDIET_SCAN_DEFAULT_SCOPE");
    cmd.env_remove("MACDIET_SCAN_EXCLUDE");
    cmd.env_remove("MACDIET_FIX_DEFAULT_RISK_MAX");
    cmd.env_remove("MACDIET_PRIVACY_MASK_HOME");
    cmd.env_remove("MACDIET_REPORT_INCLUDE_EVIDENCE");
    cmd
}

fn run(home: &Path, args: &[&str]) -> Output {
    macdiet_cmd(home).args(args).output().expect("run macdiet")
}

fn make_temp_home() -> PathBuf {
    static HOME_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("macdiet-exit-test-{}-{seq}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create home");
    home
}

#[test]
fn completion_unknown_shell_exits_2() {
    let home = make_temp_home();
    let out = run(&home, &["completion", "nope"]);
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fix_apply_requires_tty_exits_2() {
    let home = make_temp_home();
    let out = run(&home, &["fix", "--apply"]);
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fix_interactive_requires_tty_exits_2() {
    let home = make_temp_home();
    let out = run(&home, &["fix", "--interactive"]);
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn ui_requires_tty_exits_2() {
    let home = make_temp_home();
    let out = run(&home, &["ui"]);
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn scan_invalid_exclude_exits_2() {
    let home = make_temp_home();
    let out = run(&home, &["scan", "--deep", "--exclude", "["]);
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn snapshots_thin_requires_tty_exits_2() {
    let home = make_temp_home();
    let out = run(
        &home,
        &["snapshots", "thin", "--bytes", "1", "--urgency", "1"],
    );
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn snapshots_thin_dry_run_succeeds_non_tty() {
    let home = make_temp_home();
    let out = run(
        &home,
        &[
            "--dry-run",
            "snapshots",
            "thin",
            "--bytes",
            "1",
            "--urgency",
            "1",
        ],
    );
    assert!(out.status.success());
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn snapshots_delete_requires_tty_exits_2() {
    let home = make_temp_home();
    let out = run(
        &home,
        &[
            "snapshots",
            "delete",
            "--id",
            "01234567-89ab-cdef-0123-456789abcdef",
        ],
    );
    assert_eq!(out.status.code(), Some(2));
    let _ = std::fs::remove_dir_all(&home);
}
