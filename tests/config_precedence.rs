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

    let temp = std::env::temp_dir();
    let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
    let uniq = format!("macdiet-config-test-{}-{seq}", std::process::id());
    let home = temp.join(uniq);
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create home");
    home
}

fn write_file(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdirs");
    }
    std::fs::write(path, bytes).expect("write");
}

#[test]
fn config_can_enable_report_evidence_and_disable_masking() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");
    write_file(
        home.join(".config/macdiet/config.toml").as_path(),
        br#"
[report]
include_evidence = true

[privacy]
mask_home = false
"#,
    );

    let out = run(&home, &["report", "--json"]);
    assert!(out.status.success());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    assert!(!findings.is_empty(), "expected findings");

    let mut saw_unmasked_path = false;
    for f in findings {
        let evidence = f
            .get("evidence")
            .and_then(|e| e.as_array())
            .expect("evidence array");
        for ev in evidence {
            if ev.get("kind").and_then(|k| k.as_str()) != Some("path") {
                continue;
            }
            let value = ev.get("value").and_then(|v| v.as_str()).unwrap_or("");
            let masked = ev.get("masked").and_then(|m| m.as_bool()).unwrap_or(true);
            assert!(
                !masked,
                "expected masked=false when privacy.mask_home=false"
            );
            assert!(
                value.starts_with(home.to_string_lossy().as_ref()),
                "expected absolute home path when masking disabled: {value}"
            );
            saw_unmasked_path = true;
        }
    }
    assert!(
        saw_unmasked_path,
        "expected at least one unmasked path evidence"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn config_default_fix_risk_max_affects_dry_run() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");
    write_file(
        home.join(".config/macdiet/config.toml").as_path(),
        br#"
[fix]
default_risk_max = "R0"
"#,
    );

    let out = run(&home, &["fix"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("実行可能なアクションがありません。"),
        "stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn config_show_emits_effective_config() {
    let home = make_temp_home();
    write_file(
        home.join(".config/macdiet/config.toml").as_path(),
        br#"
[ui]
max_table_rows = 3
"#,
    );

    let out = run(&home, &["config", "--show"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("max_table_rows = 3"), "stdout={stdout}");
    assert!(stdout.contains("config_path"), "stdout={stdout}");

    let _ = std::fs::remove_dir_all(&home);
}
