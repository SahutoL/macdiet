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
    let uniq = format!("macdiet-report-md-test-{}-{seq}", std::process::id());
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
fn report_markdown_includes_actions_paths_and_instructions() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["report", "--markdown"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("## 所見 ("), "stdout={stdout}");
    assert!(stdout.contains("## アクション ("), "stdout={stdout}");
    assert!(
        stdout.contains("npm cache をゴミ箱へ移動"),
        "stdout={stdout}"
    );
    assert!(
        stdout.contains("- 種類: ゴミ箱へ移動（TRASH_MOVE）"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("`~/.npm`"), "stdout={stdout}");
    assert!(stdout.contains("`npm install`"), "stdout={stdout}");
    assert!(stdout.contains("#### 手順"), "stdout={stdout}");
    assert!(
        stdout.contains("npm のキャッシュは再取得可能です"),
        "stdout={stdout}"
    );
    assert!(
        !stdout.contains("- 根拠:"),
        "evidence should be hidden by default: stdout={stdout}"
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn report_markdown_include_evidence_prints_path_and_stat() {
    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");

    let out = run(&home, &["report", "--markdown", "--include-evidence"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("- 根拠:"), "stdout={stdout}");
    assert!(stdout.contains("パス: `~/.npm`"), "stdout={stdout}");
    assert!(stdout.contains("統計: `files="), "stdout={stdout}");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn report_markdown_multiline_stat_is_rendered_as_code_block() {
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
        cmd.args(["report", "--markdown", "--include-evidence"]);
        cmd.output().expect("run macdiet")
    };
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("コマンド: `docker system df`"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("```text"), "stdout={stdout}");
    assert!(stdout.contains("docker system df:"), "stdout={stdout}");
    assert!(stdout.contains("Images"), "stdout={stdout}");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn report_markdown_sorts_actions_by_risk_then_size_then_id() {
    use std::os::unix::fs::PermissionsExt;

    let home = make_temp_home();
    write_file(home.join(".npm/cache.bin").as_path(), b"hello");
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
        cmd.args(["report", "--markdown"]);
        cmd.output().expect("run macdiet")
    };
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let r1 = stdout
        .find("npm cache をゴミ箱へ移動")
        .expect("R1 action present");
    let r2 = stdout
        .find("利用できないシミュレータを削除")
        .expect("R2 action present");
    assert!(r1 < r2, "stdout={stdout}");

    let _ = std::fs::remove_dir_all(&home);
}
