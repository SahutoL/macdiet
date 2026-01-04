use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn base_cmd(home: &Path) -> Command {
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

fn make_temp_home() -> PathBuf {
    static HOME_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
    let home = std::env::temp_dir().join(format!("macdiet-env-test-{}-{seq}", std::process::id()));
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
fn env_overrides_config_file() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");
    write_file(
        home.join(".config/macdiet/config.toml").as_path(),
        br#"
[report]
include_evidence = false

[privacy]
mask_home = true
"#,
    );

    let out = {
        let mut cmd = base_cmd(&home);
        cmd.env("MACDIET_REPORT_INCLUDE_EVIDENCE", "true");
        cmd.env("MACDIET_PRIVACY_MASK_HOME", "false");
        cmd.args(["report", "--json"]);
        cmd.output().expect("run macdiet")
    };
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
            assert!(!masked);
            assert!(value.starts_with(home.to_string_lossy().as_ref()));
            saw_unmasked_path = true;
        }
    }
    assert!(saw_unmasked_path);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn cli_config_path_overrides_env_config_path() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let cfg_env = home.join("env-config.toml");
    let cfg_cli = home.join("cli-config.toml");
    write_file(
        cfg_env.as_path(),
        br#"
[fix]
default_risk_max = "R0"
"#,
    );
    write_file(
        cfg_cli.as_path(),
        br#"
[fix]
default_risk_max = "R1"
"#,
    );

    let out = {
        let mut cmd = base_cmd(&home);
        cmd.env("MACDIET_CONFIG", &cfg_env);
        cmd.args(["fix", "--config"]);
        cmd.arg(&cfg_cli);
        cmd.output().expect("run macdiet")
    };

    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("No eligible actions."), "stdout={stdout}");

    let _ = std::fs::remove_dir_all(&home);
}
