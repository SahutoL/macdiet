use std::path::{Path, PathBuf};
use std::process::Command;
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

#[cfg(not(target_os = "macos"))]
fn run(home: &Path, args: &[&str]) -> std::process::Output {
    macdiet_cmd(home).args(args).output().expect("run macdiet")
}

fn make_temp_home() -> PathBuf {
    static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

    let temp = std::env::temp_dir();
    let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
    let uniq = format!("macdiet-snapshots-test-{}-{seq}", std::process::id());
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

#[cfg(target_os = "macos")]
#[test]
fn snapshots_status_unobserved_includes_next_steps_actions() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();

    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("mkdir bin");

    let tmutil_path = bin_dir.join("tmutil");
    write_file(
        tmutil_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "listlocalsnapshots" ]; then
  echo "tmutil: simulated failure" 1>&2
  exit 1
fi
exit 0
"#,
    );
    let mut perms = std::fs::metadata(&tmutil_path)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&tmutil_path, perms).expect("chmod");

    let diskutil_path = bin_dir.join("diskutil");
    write_file(
        diskutil_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "apfs" ] && [ "$2" = "listSnapshots" ]; then
  echo "diskutil: simulated failure" 1>&2
  exit 1
fi
exit 0
"#,
    );
    let mut perms = std::fs::metadata(&diskutil_path)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&diskutil_path, perms).expect("chmod");

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let out = {
        let mut cmd = macdiet_cmd(&home);
        cmd.env("PATH", path);
        cmd.args(["snapshots", "status", "--json"]);
        cmd.output().expect("run macdiet")
    };
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");

    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    let tm = findings
        .iter()
        .find(|f| f.get("id").and_then(|s| s.as_str()) == Some("tm-local-snapshots-unobserved"))
        .expect("tm unobserved finding");
    let tm_actions = tm
        .get("recommended_actions")
        .and_then(|a| a.as_array())
        .expect("recommended_actions array");
    assert!(
        tm_actions.iter().any(
            |a| a.get("id").and_then(|s| s.as_str()) == Some("tm-local-snapshots-troubleshoot")
        ),
        "tm recommended_actions={tm_actions:?}"
    );

    let apfs = findings
        .iter()
        .find(|f| f.get("id").and_then(|s| s.as_str()) == Some("apfs-snapshots-unobserved"))
        .expect("apfs unobserved finding");
    let apfs_actions = apfs
        .get("recommended_actions")
        .and_then(|a| a.as_array())
        .expect("recommended_actions array");
    assert!(
        apfs_actions
            .iter()
            .any(|a| a.get("id").and_then(|s| s.as_str()) == Some("apfs-snapshots-disk-utility")),
        "apfs recommended_actions={apfs_actions:?}"
    );

    let actions = v
        .get("actions")
        .and_then(|a| a.as_array())
        .expect("actions array");
    let tm_action = actions
        .iter()
        .find(|a| a.get("id").and_then(|s| s.as_str()) == Some("tm-local-snapshots-troubleshoot"))
        .expect("tm action");
    let tm_related = tm_action
        .get("related_findings")
        .and_then(|r| r.as_array())
        .expect("related_findings array");
    assert!(
        tm_related
            .iter()
            .any(|v| v.as_str() == Some("tm-local-snapshots-unobserved")),
        "tm related_findings={tm_related:?}"
    );
    let tm_markdown = tm_action
        .get("kind")
        .and_then(|k| k.get("markdown"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    assert!(
        tm_markdown.contains("tmutil listlocalsnapshots /"),
        "md={tm_markdown}"
    );

    let apfs_action = actions
        .iter()
        .find(|a| a.get("id").and_then(|s| s.as_str()) == Some("apfs-snapshots-disk-utility"))
        .expect("apfs action");
    let apfs_related = apfs_action
        .get("related_findings")
        .and_then(|r| r.as_array())
        .expect("related_findings array");
    assert!(
        apfs_related
            .iter()
            .any(|v| v.as_str() == Some("apfs-snapshots-unobserved")),
        "apfs related_findings={apfs_related:?}"
    );
    let apfs_markdown = apfs_action
        .get("kind")
        .and_then(|k| k.get("markdown"))
        .and_then(|m| m.as_str())
        .unwrap_or("");
    assert!(
        apfs_markdown.contains("sudo diskutil apfs listSnapshots /"),
        "md={apfs_markdown}"
    );

    let out = {
        let mut cmd = macdiet_cmd(&home);
        cmd.env(
            "PATH",
            format!(
                "{}:{}",
                bin_dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        cmd.args(["snapshots", "status"]);
        cmd.output().expect("run macdiet")
    };
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("理由: tmutil: simulated failure"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("理由: diskutil: simulated failure"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("Time Machine ローカルスナップショットのトラブルシュート"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("Disk Utility で APFS スナップショットを確認"),
        "stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(target_os = "macos")]
#[test]
fn doctor_includes_snapshots_findings_and_snapshots_section() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("mkdir bin");

    let tmutil_path = bin_dir.join("tmutil");
    write_file(
        tmutil_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "listlocalsnapshots" ]; then
  echo "tmutil: simulated failure" 1>&2
  exit 1
fi
exit 0
"#,
    );
    let mut perms = std::fs::metadata(&tmutil_path)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&tmutil_path, perms).expect("chmod");

    let diskutil_path = bin_dir.join("diskutil");
    write_file(
        diskutil_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "apfs" ] && [ "$2" = "listSnapshots" ]; then
  echo "diskutil: simulated failure" 1>&2
  exit 1
fi
exit 0
"#,
    );
    let mut perms = std::fs::metadata(&diskutil_path)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&diskutil_path, perms).expect("chmod");

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let out = {
        let mut cmd = macdiet_cmd(&home);
        cmd.env("PATH", &path);
        cmd.args(["doctor", "--json"]);
        cmd.output().expect("run macdiet")
    };
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    assert!(
        findings
            .iter()
            .any(|f| f.get("id").and_then(|s| s.as_str()) == Some("tm-local-snapshots-unobserved")),
        "findings={findings:?}"
    );
    assert!(
        findings
            .iter()
            .any(|f| f.get("id").and_then(|s| s.as_str()) == Some("apfs-snapshots-unobserved")),
        "findings={findings:?}"
    );

    let out = {
        let mut cmd = macdiet_cmd(&home);
        cmd.env("PATH", &path);
        cmd.args(["doctor", "--top", "1"]);
        cmd.output().expect("run macdiet")
    };
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("スナップショット:"), "stdout={stdout}");
    assert!(
        stdout.contains("tmutil: simulated failure"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("diskutil: simulated failure"),
        "stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(not(target_os = "macos"))]
#[test]
fn snapshots_status_non_macos_is_unavailable() {
    let home = make_temp_home();
    let out = run(&home, &["snapshots", "status", "--json"]);
    assert!(out.status.success());

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("parse json");
    let findings = v
        .get("findings")
        .and_then(|f| f.as_array())
        .expect("findings array");
    assert!(
        findings.iter().any(|f| {
            f.get("finding_type")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .ends_with("_UNOBSERVED")
        }),
        "expected *_UNOBSERVED on non-macos: {findings:?}"
    );

    let _ = std::fs::remove_dir_all(&home);
}
