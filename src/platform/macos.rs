use std::time::Duration;

use anyhow::Result;

use crate::core::OsInfo;
use crate::platform::{CommandOutput, run_command};

pub fn os_info(timeout: Duration) -> OsInfo {
    let output = run_command("sw_vers", &["-productVersion"], timeout);
    match output {
        Ok(output) if output.exit_code == 0 => OsInfo {
            name: "macOS".to_string(),
            version: output.stdout.trim().to_string(),
        },
        _ => OsInfo {
            name: "macOS".to_string(),
            version: "unknown".to_string(),
        },
    }
}

pub fn tmutil_list_local_snapshots(timeout: Duration) -> Result<CommandOutput> {
    run_command("tmutil", &["listlocalsnapshots", "/"], timeout)
}

pub fn tmutil_thin_local_snapshots(
    mount_point: &str,
    bytes: u64,
    urgency: u8,
    timeout: Duration,
) -> Result<CommandOutput> {
    let bytes_s = bytes.to_string();
    let urgency_s = urgency.to_string();
    let args = [
        "thinlocalsnapshots",
        mount_point,
        bytes_s.as_str(),
        urgency_s.as_str(),
    ];
    run_command("tmutil", &args, timeout)
}

pub fn diskutil_apfs_list_snapshots(mount_point: &str, timeout: Duration) -> Result<CommandOutput> {
    run_command("diskutil", &["apfs", "listSnapshots", mount_point], timeout)
}

pub fn diskutil_apfs_delete_snapshot(
    mount_point: &str,
    uuid: &str,
    timeout: Duration,
) -> Result<CommandOutput> {
    run_command(
        "diskutil",
        &["apfs", "deleteSnapshot", mount_point, "-uuid", uuid],
        timeout,
    )
}
