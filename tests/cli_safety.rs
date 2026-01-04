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
    let uniq = format!("macdiet-cli-test-{}-{seq}", std::process::id());
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
fn fix_dry_run_does_not_change_filesystem() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["fix"]);
    assert!(
        out.status.success(),
        "stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    assert!(home.join(".npm").exists(), ".npm should remain");
    assert!(
        !home.join(".Trash").exists(),
        ".Trash should not be created in dry-run"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fix_outputs_impact_notes_for_r1_actions() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["fix"]);
    assert!(out.status.success());

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("影響:"), "stdout={stdout}");
    assert!(
        stdout.contains("npm install"),
        "expected npm impact note in stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fix_target_accepts_action_id() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(
        &home,
        &["fix", "--risk", "R1", "--target", "npm-cache-trash"],
    );
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("npm-cache-trash"), "stdout={stdout}");
    assert!(
        !stdout.contains("npm-cache-review"),
        "expected action-id targeting to exclude other actions: stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fix_apply_requires_tty_and_does_not_change_filesystem() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["fix", "--apply"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("TTY が必要"), "stderr={stderr}");

    assert!(home.join(".npm").exists(), ".npm should remain");
    assert!(
        !home.join(".Trash").exists(),
        ".Trash should not be created when apply is rejected"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fix_interactive_requires_tty() {
    let home = make_temp_home();
    let out = run(&home, &["fix", "--interactive"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("TTY が必要"), "stderr={stderr}");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn report_json_hides_evidence_by_default() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["report", "--json"]);
    assert!(out.status.success());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    assert!(!findings.is_empty(), "expected findings");
    for f in findings {
        let evidence = f
            .get("evidence")
            .and_then(|e| e.as_array())
            .expect("evidence array");
        assert!(
            evidence.is_empty(),
            "evidence should be empty by default: {f}"
        );
    }

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn report_json_include_evidence_includes_masked_paths() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["report", "--json", "--include-evidence"]);
    assert!(out.status.success());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    assert!(!findings.is_empty(), "expected findings");

    let mut saw_path = false;
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
            assert!(
                value.starts_with("~/"),
                "expected masked home path: {value}"
            );
            saw_path = true;
        }
    }
    assert!(saw_path, "expected at least one path evidence");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn completion_outputs_script() {
    let home = make_temp_home();
    let out = run(&home, &["completion", "bash"]);
    assert!(out.status.success());
    assert!(
        !out.stdout.is_empty(),
        "expected non-empty completion script"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("macdiet"), "stdout={stdout}");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn doctor_detects_gradle_caches() {
    let home = make_temp_home();
    write_file(home.join(".gradle/caches/cache.bin").as_path(), b"hello");

    let out = run(&home, &["doctor", "--json"]);
    assert!(out.status.success());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    assert!(
        findings
            .iter()
            .any(|f| f.get("id").and_then(|s| s.as_str()) == Some("gradle-caches")),
        "stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn doctor_includes_docker_system_df_evidence_when_docker_present() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();
    write_file(
        home.join("Library/Containers/com.docker.docker/Data/data.bin")
            .as_path(),
        b"hello",
    );

    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("mkdir bin");
    let docker_path = bin_dir.join("docker");
    write_file(
        docker_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "system" ] && [ "$2" = "df" ]; then
  echo "TYPE            TOTAL     ACTIVE    SIZE      RECLAIMABLE"
  echo "Images          1         0         1.0GB     0B (0%)"
  echo "Containers      0         0         0B        0B"
  echo "Local Volumes   0         0         0B        0B"
  echo "Build Cache     0         0         0B        0B"
  exit 0
fi
exit 1
"#,
    );
    let mut perms = std::fs::metadata(&docker_path)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&docker_path, perms).expect("chmod");

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let out = {
        let mut cmd = macdiet_cmd(&home);
        cmd.env("PATH", path);
        cmd.args(["doctor", "--json"]);
        cmd.output().expect("run macdiet")
    };
    assert!(out.status.success());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    let docker = findings
        .iter()
        .find(|f| f.get("id").and_then(|s| s.as_str()) == Some("docker-desktop-data"))
        .expect("docker finding");
    let evidence = docker
        .get("evidence")
        .and_then(|e| e.as_array())
        .expect("evidence array");
    assert!(
        evidence.iter().any(
            |ev| ev.get("kind").and_then(|k| k.as_str()) == Some("command")
                && ev.get("value").and_then(|v| v.as_str()) == Some("docker system df")
        ),
        "evidence={evidence:?}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn scan_dev_scope_includes_gradle_dir() {
    let home = make_temp_home();
    write_file(home.join(".gradle/caches/cache.bin").as_path(), b"hello");

    let out = run(
        &home,
        &[
            "scan",
            "--deep",
            "--scope",
            "dev",
            "--max-depth",
            "2",
            "--top-dirs",
            "50",
            "--json",
        ],
    );
    assert!(out.status.success());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    assert!(
        findings.iter().any(|f| {
            f.get("id")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .contains("~/.gradle")
        }),
        "stdout={}",
        String::from_utf8_lossy(&out.stdout)
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn doctor_respects_ui_max_table_rows_in_headers() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");
    write_file(home.join(".cargo/registry/cache.bin").as_path(), b"hello");

    let out = {
        let mut cmd = macdiet_cmd(&home);
        cmd.env("MACDIET_UI_MAX_TABLE_ROWS", "1");
        cmd.args(["doctor", "--top", "10"]);
        cmd.output().expect("run macdiet")
    };
    assert!(out.status.success());

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("上位の所見（1件表示 / 全"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("推奨アクション（1件表示 / 全"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("- ...（残り"), "stdout={stdout}");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fix_unknown_target_exits_2_and_shows_hint() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["fix", "--target", "no-such-finding"]);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("不明なtarget"), "stderr={stderr}");
    assert!(stderr.contains("fix --interactive"), "stderr={stderr}");
    assert!(stderr.contains("finding_id"), "stderr={stderr}");
    assert!(stderr.contains("action_id"), "stderr={stderr}");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn doctor_includes_system_data_definition_note() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["doctor"]);
    assert!(out.status.success());

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("System Data は、他カテゴリに属さない"),
        "stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn doctor_unobserved_hint_includes_full_disk_access_settings_path() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();
    write_file(
        home.join("Library/Developer/Xcode/DerivedData/file.bin")
            .as_path(),
        b"hello",
    );
    write_file(
        home.join("Library/Developer/Xcode/DerivedData/unreadable/child.bin")
            .as_path(),
        b"x",
    );

    let unreadable_dir = home.join("Library/Developer/Xcode/DerivedData/unreadable");
    let mut perms = std::fs::metadata(&unreadable_dir)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&unreadable_dir, perms).expect("chmod");

    let out = run(&home, &["doctor"]);
    assert!(out.status.success());

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("未観測:"), "stdout={stdout}");
    assert!(stdout.contains("Full Disk Access"), "stdout={stdout}");
    assert!(stdout.contains("システム設定"), "stdout={stdout}");
    assert!(stdout.contains("フルディスクアクセス"), "stdout={stdout}");

    let mut perms = std::fs::metadata(&unreadable_dir)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    let _ = std::fs::set_permissions(&unreadable_dir, perms);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn doctor_json_estimates_unobserved_bytes_when_scan_errors_present() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();
    write_file(
        home.join("Library/Developer/Xcode/DerivedData/file.bin")
            .as_path(),
        b"hello",
    );
    write_file(
        home.join("Library/Developer/Xcode/DerivedData/unreadable/child.bin")
            .as_path(),
        b"x",
    );

    let unreadable_dir = home.join("Library/Developer/Xcode/DerivedData/unreadable");
    let mut perms = std::fs::metadata(&unreadable_dir)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&unreadable_dir, perms).expect("chmod");

    let out = run(&home, &["doctor", "--json"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let unobserved = v
        .get("summary")
        .and_then(|s| s.get("unobserved_bytes"))
        .and_then(|b| b.as_u64())
        .expect("summary.unobserved_bytes");
    assert!(unobserved > 0, "unobserved_bytes={unobserved} json={v}");

    let mut perms = std::fs::metadata(&unreadable_dir)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    let _ = std::fs::set_permissions(&unreadable_dir, perms);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn doctor_prioritizes_system_data_notes_before_unobserved_notes() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();
    write_file(
        home.join("Library/Developer/Xcode/DerivedData/file.bin")
            .as_path(),
        b"hello",
    );
    write_file(
        home.join("Library/Developer/Xcode/DerivedData/unreadable/child.bin")
            .as_path(),
        b"x",
    );

    let unreadable_dir = home.join("Library/Developer/Xcode/DerivedData/unreadable");
    let mut perms = std::fs::metadata(&unreadable_dir)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o000);
    std::fs::set_permissions(&unreadable_dir, perms).expect("chmod");

    let out = run(&home, &["doctor"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let sys_idx = stdout
        .find("System Data は、他カテゴリに属さない")
        .expect("System Data note");
    let unobs_idx = stdout.find("未観測:").expect("unobserved note");
    assert!(sys_idx < unobs_idx, "stdout={stdout}");

    let mut perms = std::fs::metadata(&unreadable_dir)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    let _ = std::fs::set_permissions(&unreadable_dir, perms);

    let _ = std::fs::remove_dir_all(&home);
}
