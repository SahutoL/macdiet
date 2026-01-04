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

fn make_temp_home() -> PathBuf {
    static HOME_SEQ: AtomicU64 = AtomicU64::new(0);

    let temp = std::env::temp_dir();
    let seq = HOME_SEQ.fetch_add(1, Ordering::Relaxed);
    let uniq = format!("macdiet-doctor-timeout-test-{}-{seq}", std::process::id());
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
fn doctor_timeout_is_shared_across_external_commands() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    let home = make_temp_home();

    write_file(
        home.join("Library/Containers/com.docker.docker/Data/data.bin")
            .as_path(),
        b"hello",
    );

    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("mkdir bin");

    for (name, script) in [
        (
            "docker",
            r#"#!/bin/sh
if [ "$1" = "system" ] && [ "$2" = "df" ]; then
  sleep 5
  exit 0
fi
exit 0
"#,
        ),
        (
            "tmutil",
            r#"#!/bin/sh
if [ "$1" = "listlocalsnapshots" ]; then
  sleep 5
  exit 0
fi
exit 0
"#,
        ),
        (
            "diskutil",
            r#"#!/bin/sh
if [ "$1" = "apfs" ] && [ "$2" = "listSnapshots" ]; then
  sleep 5
  exit 0
fi
exit 0
"#,
        ),
    ] {
        let path = bin_dir.join(name);
        write_file(path.as_path(), script.as_bytes());
        let mut perms = std::fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms).expect("chmod");
    }

    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let start = Instant::now();
    let out = {
        let mut cmd = macdiet_cmd(&home);
        cmd.env("PATH", path);
        cmd.args(["--timeout", "2", "doctor", "--top", "1"]);
        cmd.output().expect("run macdiet")
    };
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(4),
        "doctor took too long: elapsed={elapsed:?}\nstderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("スナップショット:"), "stdout={stdout}");
    assert!(stdout.contains("タイムアウト"), "stdout={stdout}");

    let _ = std::fs::remove_dir_all(&home);
}
