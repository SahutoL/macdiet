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
    let uniq = format!("macdiet-fix-r2-test-{}-{seq}", std::process::id());
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
fn fix_r2_preview_includes_potential_reclaim_and_caution_notes() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();
    write_file(
        home.join("Library/Developer/Xcode/Archives/archive.bin")
            .as_path(),
        b"a",
    );
    write_file(
        home.join("Library/Developer/Xcode/iOS DeviceSupport/support.bin")
            .as_path(),
        b"b",
    );
    write_file(
        home.join("Library/Developer/CoreSimulator/Devices/device.bin")
            .as_path(),
        b"c",
    );
    write_file(
        home.join("Library/Containers/com.docker.docker/Data/data.bin")
            .as_path(),
        b"d",
    );

    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("mkdir bin");

    let xcrun_path = bin_dir.join("xcrun");
    write_file(
        xcrun_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "simctl" ] && [ "$2" = "list" ] && [ "$3" = "devices" ] && [ "$4" = "unavailable" ]; then
  echo "== Devices =="
  echo "-- iOS 17.0 --"
  echo "    iPhone 14 (AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE) (Shutdown) (unavailable, runtime profile not found)"
  exit 0
fi
exit 1
"#,
    );
    let mut perms = std::fs::metadata(&xcrun_path)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&xcrun_path, perms).expect("chmod");

    let docker_path = bin_dir.join("docker");
    write_file(
        docker_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "system" ] && [ "$2" = "df" ]; then
  echo "TYPE            TOTAL     ACTIVE    SIZE      RECLAIMABLE"
  echo "Images          1         0         1.0GB     0B (0%)"
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
        cmd.args(["fix", "--risk", "R2"]);
        cmd.output().expect("run macdiet")
    };
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("参考：削減見込み（R2+プレビュー）:"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("既定でプレビューのみ") && stdout.contains("許可リストされた RUN_CMD"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("注記: 注意: 過去ビルドの配布・デバッグに必要な場合があります（R2）。"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("注記: 削除は慎重に（R2）。"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("コマンド: xcrun simctl delete unavailable"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("コマンド: docker builder prune"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("コマンド: docker system prune"),
        "stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn fix_r2_preview_skips_simctl_action_when_no_unavailable_devices() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();
    write_file(
        home.join("Library/Developer/CoreSimulator/Devices/device.bin")
            .as_path(),
        b"c",
    );
    write_file(
        home.join("Library/Containers/com.docker.docker/Data/data.bin")
            .as_path(),
        b"d",
    );

    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("mkdir bin");

    let xcrun_path = bin_dir.join("xcrun");
    write_file(
        xcrun_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "simctl" ] && [ "$2" = "list" ] && [ "$3" = "devices" ] && [ "$4" = "unavailable" ]; then
  # no unavailable devices
  exit 0
fi
exit 1
"#,
    );
    let mut perms = std::fs::metadata(&xcrun_path)
        .expect("metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&xcrun_path, perms).expect("chmod");

    let docker_path = bin_dir.join("docker");
    write_file(
        docker_path.as_path(),
        br#"#!/bin/sh
if [ "$1" = "system" ] && [ "$2" = "df" ]; then
  echo "TYPE            TOTAL     ACTIVE    SIZE      RECLAIMABLE"
  echo "Images          1         0         1.0GB     0B (0%)"
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
        cmd.args(["fix", "--risk", "R2"]);
        cmd.output().expect("run macdiet")
    };
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("コマンド: xcrun simctl delete unavailable"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("コマンド: docker builder prune"),
        "stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}
