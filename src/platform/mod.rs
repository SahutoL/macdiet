use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use wait_timeout::ChildExt;

use crate::core::OsInfo;

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone)]
pub struct CommandRunAs {
    pub uid: u32,
    pub gid: u32,
}

#[derive(Debug, Clone, Default)]
pub struct CommandRunOptions {
    pub run_as: Option<CommandRunAs>,
    pub env: Vec<(String, String)>,
}

pub fn run_command(cmd: &str, args: &[&str], timeout: Duration) -> Result<CommandOutput> {
    run_command_with_options(cmd, args, timeout, &CommandRunOptions::default())
}

pub fn run_command_with_options(
    cmd: &str,
    args: &[&str],
    timeout: Duration,
    options: &CommandRunOptions,
) -> Result<CommandOutput> {
    let mut command = Command::new(cmd);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (k, v) in &options.env {
        command.env(k, v);
    }

    #[cfg(unix)]
    if let Some(run_as) = &options.run_as {
        use std::os::unix::process::CommandExt;
        command.uid(run_as.uid);
        command.gid(run_as.gid);
    }

    let mut child = command
        .spawn()
        .with_context(|| format!("プロセス起動に失敗しました: {cmd}"))?;

    let status = match child
        .wait_timeout(timeout)
        .with_context(|| format!("プロセス待機に失敗しました: {cmd}"))?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!("タイムアウトしました（{timeout:?}）: {cmd}"));
        }
    };

    let mut stdout = String::new();
    if let Some(mut out) = child.stdout.take() {
        let _ = out.read_to_string(&mut stdout);
    }
    let mut stderr = String::new();
    if let Some(mut err) = child.stderr.take() {
        let _ = err.read_to_string(&mut stderr);
    }

    Ok(CommandOutput {
        exit_code: status.code().unwrap_or(-1),
        stdout,
        stderr,
    })
}

#[derive(Debug, Clone)]
pub struct InvokingUser {
    pub uid: u32,
    pub gid: u32,
    pub username: Option<String>,
    pub home_dir: PathBuf,
}

pub fn invoking_user() -> Option<InvokingUser> {
    let uid = std::env::var("SUDO_UID").ok()?.parse::<u32>().ok()?;
    let gid = std::env::var("SUDO_GID").ok()?.parse::<u32>().ok()?;
    let username = std::env::var("SUDO_USER").ok();
    let home_dir = home_dir_for_uid(uid)?;

    Some(InvokingUser {
        uid,
        gid,
        username,
        home_dir,
    })
}

pub fn effective_home_dir() -> Result<PathBuf> {
    if let Some(user) = invoking_user() {
        return Ok(user.home_dir);
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("環境変数 HOME が設定されていません"))
}

pub fn run_command_invoking_user(cmd: &str, args: &[&str], timeout: Duration) -> Result<CommandOutput> {
    let Some(user) = invoking_user() else {
        return run_command(cmd, args, timeout);
    };

    let mut env = vec![("HOME".to_string(), user.home_dir.display().to_string())];
    if let Some(name) = user.username.clone() {
        env.push(("USER".to_string(), name.clone()));
        env.push(("LOGNAME".to_string(), name));
    }

    run_command_with_options(
        cmd,
        args,
        timeout,
        &CommandRunOptions {
            run_as: Some(CommandRunAs {
                uid: user.uid,
                gid: user.gid,
            }),
            env,
        },
    )
}

#[cfg(unix)]
fn home_dir_for_uid(uid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;

    unsafe {
        let bufsize = libc::sysconf(libc::_SC_GETPW_R_SIZE_MAX);
        let bufsize = if bufsize <= 0 {
            16 * 1024
        } else {
            bufsize as usize
        };
        let mut buf = vec![0u8; bufsize];
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut result: *mut libc::passwd = std::ptr::null_mut();

        let rc = libc::getpwuid_r(
            uid as libc::uid_t,
            &mut pwd,
            buf.as_mut_ptr() as *mut libc::c_char,
            buf.len(),
            &mut result,
        );
        if rc != 0 || result.is_null() {
            return None;
        }
        if pwd.pw_dir.is_null() {
            return None;
        }

        let dir = CStr::from_ptr(pwd.pw_dir).to_string_lossy().to_string();
        if dir.trim().is_empty() {
            return None;
        }
        Some(PathBuf::from(dir))
    }
}

#[cfg(not(unix))]
fn home_dir_for_uid(_uid: u32) -> Option<PathBuf> {
    None
}

pub fn os_info(timeout: Duration) -> OsInfo {
    #[cfg(target_os = "macos")]
    {
        return crate::platform::macos::os_info(timeout);
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = timeout;
        return OsInfo {
            name: "unknown".to_string(),
            version: "unknown".to_string(),
        };
    }
}

#[cfg(target_os = "macos")]
pub mod macos;
