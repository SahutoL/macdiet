use std::io;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::core::RiskLevel;
use crate::engine::{Engine, EngineOptions, ScanRequest};
use crate::ui::UiConfig;

mod interactive;

#[derive(Debug, Parser)]
#[command(
    name = "macdiet",
    version,
    about = "macOSのSystem Dataを分解して可視化し、安全な改善提案と限定的な掃除を行う（開発者向け）"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long = "no-color", global = true)]
    pub no_color: bool,
    #[arg(long, global = true)]
    pub verbose: bool,
    #[arg(long, global = true)]
    pub quiet: bool,
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    #[arg(long, default_value_t = 30, global = true)]
    pub timeout: u64,
    #[arg(long, global = true)]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Doctor(DoctorArgs),
    Scan(ScanArgs),
    Snapshots(SnapshotsArgs),
    Fix(FixArgs),
    Report(ReportArgs),
    Ui(UiArgs),
    Completion(CompletionArgs),
    Config(ConfigArgs),
}

#[derive(Debug, Args)]
pub struct DoctorArgs {
    #[arg(long, default_value_t = 10)]
    pub top: usize,
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    #[arg(long)]
    pub scope: Option<String>,
    #[arg(long)]
    pub deep: bool,
    #[arg(long)]
    pub max_depth: Option<usize>,
    #[arg(long)]
    pub top_dirs: Option<usize>,
    #[arg(long)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Args)]
pub struct SnapshotsArgs {
    #[command(subcommand)]
    pub command: SnapshotsCommand,
}

#[derive(Debug, Subcommand)]
pub enum SnapshotsCommand {
    Status,
    Thin {
        #[arg(long)]
        bytes: u64,
        #[arg(long)]
        urgency: u8,
    },
    Delete {
        #[arg(long)]
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct FixArgs {
    #[arg(long)]
    pub interactive: bool,
    #[arg(long)]
    pub apply: bool,
    #[arg(long)]
    pub risk: Option<RiskLevel>,
    #[arg(long)]
    pub target: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ReportArgs {
    #[arg(long)]
    pub markdown: bool,
    #[arg(long)]
    pub include_evidence: bool,
}

#[derive(Debug, Args)]
pub struct UiArgs {}

#[derive(Debug, Args)]
pub struct CompletionArgs {
    pub shell: String,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[arg(long)]
    pub show: bool,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    let stdin_is_tty = io::stdin().is_terminal();
    let stdout_is_tty = io::stdout().is_terminal();
    let stderr_is_tty = io::stderr().is_terminal();

    let home_dir = crate::platform::effective_home_dir()?;

    let env_config_path = std::env::var_os("MACDIET_CONFIG").map(std::path::PathBuf::from);
    let cfg = crate::config::load(
        cli.config.as_deref().or(env_config_path.as_deref()),
        &home_dir,
    )
    .map_err(crate::exit::invalid_args_err)?;

    let color = stdout_is_tty && cfg.ui.color && !cli.no_color;

    let ui_cfg = UiConfig {
        color,
        stdin_is_tty,
        stdout_is_tty,
        stderr_is_tty,
        max_table_rows: cfg.ui.max_table_rows,
        quiet: cli.quiet,
        verbose: cli.verbose,
    };

    let is_ui_mode = matches!(&cli.command, Commands::Ui(_));
    let engine = Engine::new(EngineOptions {
        timeout: Duration::from_secs(cli.timeout),
        privacy_mask_home: cfg.privacy.mask_home,
        include_evidence: false,
        show_progress: ui_cfg.stderr_is_tty && !cli.quiet && !cli.json && !is_ui_mode,
    })?;

    match cli.command {
        Commands::Doctor(args) => {
            let report = engine.doctor()?;
            if cli.json {
                write_json(&report)?;
            } else {
                crate::ui::print_doctor(&report, &ui_cfg, args.top);
            }
        }
        Commands::Scan(args) => {
            let top_n = args.top_dirs.unwrap_or(20);
            let scope = args.scope.or_else(|| Some(cfg.scan.default_scope.clone()));
            let mut exclude = cfg.scan.exclude.clone();
            exclude.extend(args.exclude);
            exclude.sort();
            exclude.dedup();
            if args.deep {
                crate::scan::validate_excludes(&exclude).map_err(crate::exit::invalid_args_err)?;
            }
            let report = engine.scan(ScanRequest {
                scope,
                deep: args.deep,
                max_depth: args.max_depth.unwrap_or(3),
                top_dirs: top_n,
                exclude,
                show_progress: ui_cfg.stderr_is_tty && !cli.quiet && !cli.json,
            })?;
            if cli.json {
                write_json(&report)?;
            } else {
                crate::ui::print_doctor(&report, &ui_cfg, top_n);
            }
        }
        Commands::Snapshots(args) => match args.command {
            SnapshotsCommand::Status => {
                let report = engine.snapshots_status()?;
                if cli.json {
                    write_json(&report)?;
                } else {
                    crate::ui::print_snapshots_status(&report, &ui_cfg);
                }
            }
            SnapshotsCommand::Thin { bytes, urgency } => {
                if cli.json {
                    return Err(crate::exit::invalid_args(
                        "snapshots thin は --json と併用できません",
                    ));
                }
                if bytes == 0 {
                    return Err(crate::exit::invalid_args(
                        "snapshots thin: --bytes は 0 より大きい必要があります",
                    ));
                }
                if !(1..=4).contains(&urgency) {
                    return Err(crate::exit::invalid_args(
                        "snapshots thin: --urgency は 1..=4 で指定してください",
                    ));
                }

                let cmd = format!("tmutil thinlocalsnapshots / {bytes} {urgency}");

                if cli.dry_run {
                    if !ui_cfg.quiet {
                        println!("dry-run: 実行予定のコマンド: `{cmd}`");
                        println!("注意: これはR3で、必要に応じて `sudo` が必要です。");
                    }
                    return Ok(());
                }

                if !(ui_cfg.stdin_is_tty && ui_cfg.stdout_is_tty) {
                    return Err(crate::exit::invalid_args(
                        "snapshots thin は TTY が必要です（stdin + stdout）",
                    ));
                }

                if !confirm_exact(
                    "snapshots thin は R3 です。続行するには 'thin' と入力してください: ",
                    "thin",
                )? {
                    if !ui_cfg.quiet {
                        eprintln!("キャンセルしました。");
                    }
                    return Ok(());
                }
                if !confirm_exact("最終確認: 実行するには 'yes' と入力してください: ", "yes")?
                {
                    if !ui_cfg.quiet {
                        eprintln!("キャンセルしました。");
                    }
                    return Ok(());
                }

                #[cfg(not(target_os = "macos"))]
                {
                    return Err(crate::exit::invalid_args(
                        "snapshots thin は macOS のみ対応です",
                    ));
                }

                #[cfg(target_os = "macos")]
                {
                    let args: Vec<String> = vec![
                        "thinlocalsnapshots".to_string(),
                        "/".to_string(),
                        bytes.to_string(),
                        urgency.to_string(),
                    ];

                    let started_at = time::OffsetDateTime::now_utc();
                    let result = crate::platform::macos::tmutil_thin_local_snapshots(
                        "/",
                        bytes,
                        urgency,
                        Duration::from_secs(cli.timeout),
                    );
                    let finished_at = time::OffsetDateTime::now_utc();

                    let output = match result {
                        Ok(output) => output,
                        Err(err) => {
                            let err_s = err.to_string();
                            let log_path = crate::logs::write_snapshots_thin_log(
                                &home_dir,
                                started_at,
                                finished_at,
                                bytes,
                                urgency,
                                "tmutil",
                                &args,
                                None,
                                Some(err_s.clone()),
                            )
                            .map_err(|e| {
                                crate::exit::external_cmd(format!(
                                    "snapshots thin: tmutil が失敗しました: {err_s}\nさらにログの書き込みにも失敗しました: {e}"
                                ))
                            })?;
                            let log_hint = log_path
                                .strip_prefix(&home_dir)
                                .map(|p| format!("~/{p}", p = p.display()))
                                .unwrap_or_else(|_| log_path.display().to_string());
                            return Err(crate::exit::external_cmd(format!(
                                "外部コマンドが失敗しました: {cmd}\n{err_s}\nログ: {log_hint}"
                            )));
                        }
                    };

                    let log_path = crate::logs::write_snapshots_thin_log(
                        &home_dir,
                        started_at,
                        finished_at,
                        bytes,
                        urgency,
                        "tmutil",
                        &args,
                        Some(&output),
                        None,
                    )
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "snapshots thin: コマンドは終了しましたが、ログの書き込みに失敗しました: {e}"
                        )
                    })?;
                    let log_hint = log_path
                        .strip_prefix(&home_dir)
                        .map(|p| format!("~/{p}", p = p.display()))
                        .unwrap_or_else(|_| log_path.display().to_string());

                    if output.exit_code != 0 {
                        let mut msg = format!(
                            "外部コマンドが失敗しました（exit_code={}）: {cmd}",
                            output.exit_code
                        );
                        let stderr = output.stderr.trim();
                        if !stderr.is_empty() {
                            msg.push_str(&format!("\n{stderr}"));
                        }
                        msg.push_str(&format!("\nログ: {log_hint}"));
                        return Err(crate::exit::external_cmd(msg));
                    }
                    if !ui_cfg.quiet {
                        let stdout = output.stdout.trim();
                        if stdout.is_empty() {
                            println!("成功: `{cmd}`");
                        } else {
                            println!("{stdout}");
                        }
                        println!("ログ: {log_hint}");
                    }
                    if ui_cfg.verbose {
                        let stderr = output.stderr.trim();
                        if !stderr.is_empty() {
                            eprintln!("stderr（標準エラー出力）:\n{stderr}");
                        }
                    }
                }
            }
            SnapshotsCommand::Delete { id } => {
                if cli.json {
                    return Err(crate::exit::invalid_args(
                        "snapshots delete は --json と併用できません",
                    ));
                }

                let id = id.trim();
                if id.is_empty() {
                    return Err(crate::exit::invalid_args(
                        "snapshots delete: --id は空にできません",
                    ));
                }
                let is_uuid = crate::snapshots::is_uuid(id);
                let list_cmd = "diskutil apfs listSnapshots /";

                if !cli.dry_run && !(ui_cfg.stdin_is_tty && ui_cfg.stdout_is_tty) {
                    return Err(crate::exit::invalid_args(
                        "snapshots delete は TTY が必要です（stdin + stdout）",
                    ));
                }

                #[cfg(not(target_os = "macos"))]
                {
                    return Err(crate::exit::invalid_args(
                        "snapshots delete は macOS のみ対応です",
                    ));
                }

                #[cfg(target_os = "macos")]
                {
                    let started_at = if cli.dry_run {
                        None
                    } else {
                        Some(time::OffsetDateTime::now_utc())
                    };

                    let list_args: Vec<String> = vec![
                        "apfs".to_string(),
                        "listSnapshots".to_string(),
                        "/".to_string(),
                    ];

                    let list_out = match crate::platform::macos::diskutil_apfs_list_snapshots(
                        "/",
                        Duration::from_secs(cli.timeout),
                    ) {
                        Ok(out) => out,
                        Err(err) => {
                            let err_s = err.to_string();
                            if let Some(started_at) = started_at {
                                let finished_at = time::OffsetDateTime::now_utc();
                                let log_path = crate::logs::write_snapshots_delete_log(
                                    &home_dir,
                                    started_at,
                                    finished_at,
                                    id,
                                    None,
                                    "diskutil",
                                    &list_args,
                                    None,
                                    Some(err_s.clone()),
                                    None,
                                    None,
                                    None,
                                    None,
                                )
                                .map_err(|e| {
                                    crate::exit::external_cmd(format!(
                                        "snapshots delete: diskutil listSnapshots が失敗しました: {err_s}\nさらにログの書き込みにも失敗しました: {e}"
                                    ))
                                })?;
                                let log_hint = log_path
                                    .strip_prefix(&home_dir)
                                    .map(|p| format!("~/{p}", p = p.display()))
                                    .unwrap_or_else(|_| log_path.display().to_string());
                                return Err(crate::exit::external_cmd(format!(
                                    "外部コマンドが失敗しました: {list_cmd}\n{err_s}\nログ: {log_hint}"
                                )));
                            }
                            return Err(crate::exit::external_cmd_err(err));
                        }
                    };

                    if list_out.exit_code != 0 {
                        if let Some(started_at) = started_at {
                            let finished_at = time::OffsetDateTime::now_utc();
                            let log_path = crate::logs::write_snapshots_delete_log(
                                &home_dir,
                                started_at,
                                finished_at,
                                id,
                                None,
                                "diskutil",
                                &list_args,
                                Some(&list_out),
                                None,
                                None,
                                None,
                                None,
                                None,
                            )
                            .map_err(|e| {
                                anyhow::anyhow!(
                                    "snapshots delete: コマンドが失敗しましたが、ログを書き込めませんでした: {e}"
                                )
                            })?;
                            let log_hint = log_path
                                .strip_prefix(&home_dir)
                                .map(|p| format!("~/{p}", p = p.display()))
                                .unwrap_or_else(|_| log_path.display().to_string());

                            let mut msg = format!(
                                "外部コマンドが失敗しました（exit_code={}）: {list_cmd}",
                                list_out.exit_code
                            );
                            let stdout = list_out.stdout.trim();
                            if !stdout.is_empty() {
                                msg.push_str(&format!("\nstdout（標準出力）:\n{stdout}"));
                            }
                            let stderr = list_out.stderr.trim();
                            if !stderr.is_empty() {
                                msg.push_str(&format!("\nstderr（標準エラー出力）:\n{stderr}"));
                            }
                            msg.push_str(&format!("\nログ: {log_hint}"));
                            return Err(crate::exit::external_cmd(msg));
                        }

                        let mut msg = format!(
                            "外部コマンドが失敗しました（exit_code={}）: {list_cmd}",
                            list_out.exit_code
                        );
                        let stdout = list_out.stdout.trim();
                        if !stdout.is_empty() {
                            msg.push_str(&format!("\nstdout（標準出力）:\n{stdout}"));
                        }
                        let stderr = list_out.stderr.trim();
                        if !stderr.is_empty() {
                            msg.push_str(&format!("\nstderr（標準エラー出力）:\n{stderr}"));
                        }
                        return Err(crate::exit::external_cmd(msg));
                    }

                    let cat =
                        crate::snapshots::parse_diskutil_apfs_list_snapshots(&list_out.stdout);
                    if cat.uuids.is_empty() {
                        return Err(crate::exit::invalid_args(
                            "snapshots delete: diskutil の出力からスナップショットUUIDを検出できませんでした",
                        ));
                    }

                    let (uuid_to_delete, resolved_from_name): (String, Option<String>) = if is_uuid
                    {
                        let uuid = id.to_ascii_lowercase();
                        if !cat.uuids.contains(&uuid) {
                            return Err(crate::exit::invalid_args(format!(
                                "snapshots delete: diskutil の出力に指定UUIDが見つかりませんでした: {uuid}"
                            )));
                        }
                        (uuid, None)
                    } else {
                        let Some(candidates) = cat.name_to_uuids.get(id) else {
                            return Err(crate::exit::invalid_args(format!(
                                "snapshots delete: diskutil の出力に指定IDが見つかりませんでした: {id}"
                            )));
                        };
                        if candidates.len() != 1 {
                            return Err(crate::exit::invalid_args(format!(
                                "snapshots delete: スナップショットIDが曖昧です。UUIDを指定してください: {id}"
                            )));
                        }
                        let uuid = candidates.iter().next().cloned().unwrap_or_default();
                        if uuid.is_empty() {
                            return Err(crate::exit::invalid_args(format!(
                                "snapshots delete: IDからUUIDを一意に解決できませんでした: {id}"
                            )));
                        }
                        (uuid, Some(id.to_string()))
                    };

                    let delete_cmd =
                        format!("diskutil apfs deleteSnapshot / -uuid {uuid_to_delete}");

                    if cli.dry_run {
                        if !ui_cfg.quiet {
                            if let Some(name) = resolved_from_name {
                                println!("解決: `{name}` -> `{uuid_to_delete}`");
                            }
                            println!("dry-run: 実行予定のコマンド: `{delete_cmd}`");
                            println!("注意: これはR3で、必要に応じて `sudo` が必要です。");
                        }
                        return Ok(());
                    }

                    if !ui_cfg.quiet {
                        if let Some(name) = resolved_from_name.as_deref() {
                            println!("解決: `{name}` -> `{uuid_to_delete}`");
                        }
                        println!("実行するコマンド: `{delete_cmd}`");
                        println!("注意: これはR3で、必要に応じて `sudo` が必要です。");
                    }

                    if !confirm_exact(
                        "snapshots delete は R3 です。続行するには 'delete' と入力してください: ",
                        "delete",
                    )? {
                        if !ui_cfg.quiet {
                            eprintln!("キャンセルしました。");
                        }
                        return Ok(());
                    }
                    if !confirm_exact(
                        "最終確認: 削除するスナップショットUUIDをそのまま入力してください: ",
                        uuid_to_delete.as_str(),
                    )? {
                        if !ui_cfg.quiet {
                            eprintln!("キャンセルしました。");
                        }
                        return Ok(());
                    }

                    let delete_args: Vec<String> = vec![
                        "apfs".to_string(),
                        "deleteSnapshot".to_string(),
                        "/".to_string(),
                        "-uuid".to_string(),
                        uuid_to_delete.clone(),
                    ];

                    let started_at = started_at.unwrap_or_else(time::OffsetDateTime::now_utc);
                    let delete_result = crate::platform::macos::diskutil_apfs_delete_snapshot(
                        "/",
                        uuid_to_delete.as_str(),
                        Duration::from_secs(cli.timeout),
                    );
                    let finished_at = time::OffsetDateTime::now_utc();

                    let out = match delete_result {
                        Ok(out) => out,
                        Err(err) => {
                            let err_s = err.to_string();
                            let log_path = crate::logs::write_snapshots_delete_log(
                                &home_dir,
                                started_at,
                                finished_at,
                                id,
                                Some(uuid_to_delete.clone()),
                                "diskutil",
                                &list_args,
                                Some(&list_out),
                                None,
                                Some("diskutil"),
                                Some(&delete_args),
                                None,
                                Some(err_s.clone()),
                            )
                            .map_err(|e| {
                                crate::exit::external_cmd(format!(
                                    "snapshots delete: diskutil deleteSnapshot が失敗しました: {err_s}\nさらにログの書き込みにも失敗しました: {e}"
                                ))
                            })?;
                            let log_hint = log_path
                                .strip_prefix(&home_dir)
                                .map(|p| format!("~/{p}", p = p.display()))
                                .unwrap_or_else(|_| log_path.display().to_string());
                            return Err(crate::exit::external_cmd(format!(
                                "外部コマンドが失敗しました: {delete_cmd}\n{err_s}\nログ: {log_hint}"
                            )));
                        }
                    };

                    let log_path = crate::logs::write_snapshots_delete_log(
                        &home_dir,
                        started_at,
                        finished_at,
                        id,
                        Some(uuid_to_delete.clone()),
                        "diskutil",
                        &list_args,
                        Some(&list_out),
                        None,
                        Some("diskutil"),
                        Some(&delete_args),
                        Some(&out),
                        None,
                    )
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "snapshots delete: コマンドは終了しましたが、ログの書き込みに失敗しました: {e}"
                        )
                    })?;
                    let log_hint = log_path
                        .strip_prefix(&home_dir)
                        .map(|p| format!("~/{p}", p = p.display()))
                        .unwrap_or_else(|_| log_path.display().to_string());

                    if out.exit_code != 0 {
                        let mut msg = format!(
                            "外部コマンドが失敗しました（exit_code={}）: {delete_cmd}",
                            out.exit_code
                        );
                        let stdout = out.stdout.trim();
                        if !stdout.is_empty() {
                            msg.push_str(&format!("\nstdout（標準出力）:\n{stdout}"));
                        }
                        let stderr = out.stderr.trim();
                        if !stderr.is_empty() {
                            msg.push_str(&format!("\nstderr（標準エラー出力）:\n{stderr}"));
                        }
                        msg.push_str(&format!("\nログ: {log_hint}"));
                        return Err(crate::exit::external_cmd(msg));
                    }
                    if !ui_cfg.quiet {
                        let stdout = out.stdout.trim();
                        if stdout.is_empty() {
                            println!("成功: `{delete_cmd}`");
                        } else {
                            println!("{stdout}");
                        }
                        println!("ログ: {log_hint}");
                    }
                    if ui_cfg.verbose {
                        let stderr = out.stderr.trim();
                        if !stderr.is_empty() {
                            eprintln!("stderr（標準エラー出力）:\n{stderr}");
                        }
                    }
                }
            }
        },
        Commands::Fix(_args) => {
            if _args.apply && cli.dry_run {
                return Err(crate::exit::invalid_args(
                    "fix: `--apply` は `--dry-run` と併用できません",
                ));
            }
            if _args.interactive && cli.json {
                return Err(crate::exit::invalid_args(
                    "fix: `--interactive` は `--json` と併用できません",
                ));
            }
            if _args.interactive && ui_cfg.quiet {
                return Err(crate::exit::invalid_args(
                    "fix: `--interactive` は `--quiet` と併用できません",
                ));
            }
            if _args.interactive && !ui_cfg.stdout_is_tty {
                return Err(crate::exit::invalid_args(
                    "fix --interactive は TTY が必要です",
                ));
            }

            let max_risk = _args.risk.unwrap_or(cfg.fix.default_risk_max);
            if _args.apply {
                if cli.json {
                    return Err(crate::exit::invalid_args(
                        "fix --apply は --json と併用できません",
                    ));
                }
                if !(ui_cfg.stdin_is_tty && ui_cfg.stdout_is_tty) {
                    return Err(crate::exit::invalid_args(
                        "fix --apply は TTY が必要です（stdin + stdout）",
                    ));
                }
                if max_risk > crate::core::RiskLevel::R2 {
                    return Err(crate::exit::invalid_args(
                        "fix --apply は現在リスク <= R2 のみ対応です（R2 は許可リストの RUN_CMD のみ実行できます）",
                    ));
                }
            }
            let mut report = engine.doctor()?;
            report
                .summary
                .notes
                .push(format!("fix: dry-run（最大リスク={max_risk}）"));
            report
                .summary
                .notes
                .push("fix: ファイルシステムへの変更は行っていません".to_string());
            report.summary.notes.sort();
            report.summary.notes.dedup();

            let target_args = _args.target;
            let mut target_finding_ids: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            let mut target_action_ids: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            if !target_args.is_empty() {
                let available_finding_ids: std::collections::HashSet<&str> =
                    report.findings.iter().map(|f| f.id.as_str()).collect();
                let available_action_ids: std::collections::HashSet<&str> =
                    report.actions.iter().map(|a| a.id.as_str()).collect();

                let mut unknown = Vec::new();
                for t in &target_args {
                    let mut known = false;
                    if available_finding_ids.contains(t.as_str()) {
                        target_finding_ids.insert(t.clone());
                        known = true;
                    }
                    if available_action_ids.contains(t.as_str()) {
                        target_action_ids.insert(t.clone());
                        known = true;
                    }
                    if !known {
                        unknown.push(t.clone());
                    }
                }

                if !unknown.is_empty() {
                    let finding_sample: Vec<&str> = report
                        .findings
                        .iter()
                        .take(12)
                        .map(|f| f.id.as_str())
                        .collect();
                    let action_sample: Vec<&str> = report
                        .actions
                        .iter()
                        .take(12)
                        .map(|a| a.id.as_str())
                        .collect();
                    return Err(crate::exit::invalid_args(format!(
                        "fix: 不明なtargetがあります: {}\nヒント: `macdiet fix --interactive` を使うか、finding_id から選んでください: {}\nヒント: もしくは action_id から選んでください: {}",
                        unknown.join(", "),
                        finding_sample.join(", "),
                        action_sample.join(", ")
                    )));
                }
            }

            let mut actions: Vec<crate::core::ActionPlan> = report
                .actions
                .into_iter()
                .filter(|a| a.risk_level <= max_risk)
                .filter(|a| {
                    if target_args.is_empty() {
                        return true;
                    }
                    if target_action_ids.contains(&a.id) {
                        return true;
                    }
                    a.related_findings
                        .iter()
                        .any(|f| target_finding_ids.contains(f))
                })
                .collect();
            actions.sort_by_key(|a| (a.risk_level, std::cmp::Reverse(a.estimated_reclaimed_bytes)));

            crate::actions::validate_actions(&actions, &home_dir)?;

            if _args.interactive {
                let candidates: Vec<crate::core::ActionPlan> = if _args.apply {
                    actions
                        .iter()
                        .filter(|a| match &a.kind {
                            crate::core::ActionKind::TrashMove { .. } => {
                                a.risk_level <= crate::core::RiskLevel::R1
                            }
                            crate::core::ActionKind::RunCmd { .. } => {
                                crate::actions::allowlisted_run_cmd(a).is_some()
                            }
                            _ => false,
                        })
                        .cloned()
                        .collect()
                } else {
                    actions.clone()
                };

                crate::ui::print_fix_candidates(&candidates, &ui_cfg, max_risk);
                if candidates.is_empty() {
                    return Ok(());
                }
                match interactive::prompt_action_selection(candidates.len())
                    .map_err(crate::exit::invalid_args_err)?
                {
                    interactive::Selection::None => {
                        if !ui_cfg.quiet {
                            eprintln!("キャンセルしました。");
                        }
                        return Ok(());
                    }
                    interactive::Selection::All => {
                        actions = candidates;
                    }
                    interactive::Selection::Indices(indices) => {
                        actions = indices
                            .into_iter()
                            .map(|idx| candidates[idx].clone())
                            .collect();
                    }
                }

                if actions.is_empty() {
                    if !ui_cfg.quiet {
                        eprintln!("実行可能なアクションが選択されていません。");
                    }
                    return Ok(());
                }
            }

            if _args.apply {
                let part = partition_fix_apply_actions(&actions);

                if part.allowlisted_run_cmd_actions.len() > 1 {
                    let ids = part
                        .allowlisted_run_cmd_actions
                        .iter()
                        .map(|a| a.id.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(crate::exit::invalid_args(format!(
                        "安全のため、RUN_CMD の実行は一度に 1 つまでです（対象: {ids}）。\nヒント: `--target <action_id>` で 1 つに絞ってください。"
                    )));
                }

                if !ui_cfg.quiet {
                    print_fix_apply_partition(&part, &ui_cfg);
                }

                if part.trash_actions.is_empty() && part.allowlisted_run_cmd_actions.is_empty() {
                    if !ui_cfg.quiet {
                        eprintln!(
                            "適用できるアクションがありません（v0.1 の実行対象は R1/TRASH_MOVE と allowlisted RUN_CMD のみです）。"
                        );
                    }
                    return Ok(());
                }

                if !part.trash_actions.is_empty() {
                    crate::ui::print_fix_plan(
                        &part.trash_actions,
                        &ui_cfg,
                        crate::core::RiskLevel::R1,
                    );

                    if !confirm_exact(
                        "上記のパスをゴミ箱（~/.Trash）へ移動します。続行するには 'yes' と入力してください: ",
                        "yes",
                    )? {
                        if !ui_cfg.quiet {
                            eprintln!("キャンセルしました。");
                        }
                        return Ok(());
                    }

                    if !confirm_exact(
                        "最終確認: 適用するには 'trash' と入力してください: ",
                        "trash",
                    )? {
                        if !ui_cfg.quiet {
                            eprintln!("キャンセルしました。");
                        }
                        return Ok(());
                    }

                    let started_at = time::OffsetDateTime::now_utc();
                    let outcome =
                        crate::actions::apply_trash_moves(&part.trash_actions, &home_dir)?;
                    let finished_at = time::OffsetDateTime::now_utc();
                    let log_path = crate::logs::write_fix_apply_log(
                        &home_dir,
                        started_at,
                        finished_at,
                        crate::core::RiskLevel::R1,
                        &part.trash_actions,
                        &outcome,
                    )
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "fix --apply: 適用は完了しましたが、トランザクションログの書き込みに失敗しました: {e}"
                        )
                    })?;
                    if !ui_cfg.quiet {
                        println!(
                            "ゴミ箱へ移動: {} 件 / 見つからずスキップ: {} 件 / エラー: {} 件",
                            outcome.moved.len(),
                            outcome.skipped_missing.len(),
                            outcome.errors.len()
                        );
                        let log_hint = log_path
                            .strip_prefix(&home_dir)
                            .map(|p| format!("~/{p}", p = p.display()))
                            .unwrap_or_else(|_| log_path.display().to_string());
                        println!("ログ: {log_hint}");
                    }
                    if ui_cfg.verbose {
                        for record in &outcome.moved {
                            println!("移動: {} -> {}", record.from.display(), record.to.display());
                        }
                        for missing in &outcome.skipped_missing {
                            println!("見つからず: {}", missing.display());
                        }
                        for err in &outcome.errors {
                            println!("エラー: {}: {}", err.path.display(), err.error);
                        }
                    }
                    if !outcome.errors.is_empty() {
                        return Err(anyhow::anyhow!(
                            "fix --apply: ゴミ箱へ移動できないパスがありました（errors={}）",
                            outcome.errors.len()
                        ));
                    }
                }

                if let Some(action) = part.allowlisted_run_cmd_actions.first() {
                    let spec = crate::actions::allowlisted_run_cmd(action).expect("allowlisted");
                    let (cmd, args) = match &action.kind {
                        crate::core::ActionKind::RunCmd { cmd, args } => (cmd, args),
                        _ => return Err(anyhow::anyhow!("内部エラー: RUN_CMD ではありません")),
                    };
                    let cmdline = format_cmdline(cmd, args);

                    if !ui_cfg.quiet {
                        println!("\nRUN_CMD（許可リスト）:");
                        println!(
                            "- {} [{}] id={}",
                            action.title, action.risk_level, action.id
                        );
                        println!("  - コマンド: {cmdline}");
                    }

                    if !confirm_exact(
                        &format!(
                            "RUN_CMD は {} です。続行するには '{}' と入力してください: ",
                            action.risk_level, spec.confirm_token
                        ),
                        spec.confirm_token,
                    )? {
                        if !ui_cfg.quiet {
                            eprintln!("キャンセルしました。");
                        }
                        return Ok(());
                    }
                    if !confirm_exact(
                        &format!(
                            "最終確認: 実行するには '{}' と入力してください: ",
                            spec.final_confirm_token
                        ),
                        spec.final_confirm_token,
                    )? {
                        if !ui_cfg.quiet {
                            eprintln!("キャンセルしました。");
                        }
                        return Ok(());
                    }

                    let started_at = time::OffsetDateTime::now_utc();
                    let result = crate::actions::run_allowlisted_cmd(
                        action,
                        std::time::Duration::from_secs(cli.timeout),
                    );
                    let finished_at = time::OffsetDateTime::now_utc();

                    let output = match result {
                        Ok(out) => out,
                        Err(err) => {
                            let err_s = err.to_string();
                            let log_path = crate::logs::write_fix_run_cmd_log(
                                &home_dir,
                                started_at,
                                finished_at,
                                action,
                                None,
                                Some(err_s.clone()),
                            )
                            .map_err(|e| {
                                crate::exit::external_cmd(format!(
                                    "fix --apply: RUN_CMD は失敗しました: {cmdline}\n{err_s}\nさらにログの書き込みにも失敗しました: {e}"
                                ))
                            })?;
                            let log_hint = log_path
                                .strip_prefix(&home_dir)
                                .map(|p| format!("~/{p}", p = p.display()))
                                .unwrap_or_else(|_| log_path.display().to_string());
                            return Err(crate::exit::external_cmd(format!(
                                "外部コマンドが失敗しました: {cmdline}\n{err_s}\nログ: {log_hint}"
                            )));
                        }
                    };

                    let log_path = crate::logs::write_fix_run_cmd_log(
                        &home_dir,
                        started_at,
                        finished_at,
                        action,
                        Some(&output),
                        None,
                    )
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "fix --apply: コマンドは終了しましたが、ログの書き込みに失敗しました: {e}"
                        )
                    })?;
                    let log_hint = log_path
                        .strip_prefix(&home_dir)
                        .map(|p| format!("~/{p}", p = p.display()))
                        .unwrap_or_else(|_| log_path.display().to_string());

                    let eval = crate::actions::evaluate_allowlisted_run_cmd_output(action, &output);
                    match eval {
                        crate::actions::AllowlistedRunCmdOutcome::Ok => {}
                        crate::actions::AllowlistedRunCmdOutcome::OkWithWarnings(w) => {
                            if !ui_cfg.quiet {
                                eprintln!("警告: {w}（exit_code={}）", output.exit_code);
                                eprintln!("ログ: {log_hint}");
                            }
                        }
                        crate::actions::AllowlistedRunCmdOutcome::Error(_e) => {
                            let mut msg = format!(
                                "外部コマンドが失敗しました（exit_code={}）: {cmdline}",
                                output.exit_code
                            );
                            let stdout = output.stdout.trim();
                            if !stdout.is_empty() {
                                msg.push_str(&format!("\nstdout（標準出力）:\n{stdout}"));
                            }
                            let stderr = output.stderr.trim();
                            if !stderr.is_empty() {
                                msg.push_str(&format!("\nstderr（標準エラー出力）:\n{stderr}"));
                            }
                            msg.push_str(&format!("\nログ: {log_hint}"));
                            return Err(crate::exit::external_cmd(msg));
                        }
                    }

                    if !ui_cfg.quiet {
                        let stdout = output.stdout.trim();
                        if stdout.is_empty() {
                            println!("成功: `{cmdline}`");
                        } else {
                            println!("{stdout}");
                        }
                        println!("ログ: {log_hint}");
                    }
                    if ui_cfg.verbose {
                        let stderr = output.stderr.trim();
                        if !stderr.is_empty() {
                            eprintln!("stderr（標準エラー出力）:\n{stderr}");
                        }
                    }
                }

                return Ok(());
            }

            use std::collections::HashSet;
            let related: HashSet<String> = actions
                .iter()
                .flat_map(|a| a.related_findings.iter().cloned())
                .collect();

            report.actions = actions.clone();
            report.findings.retain(|f| related.contains(&f.id));
            report.summary.estimated_total_bytes =
                report.findings.iter().map(|f| f.estimated_bytes).sum();

            if cli.json {
                write_json(&report)?;
            } else {
                crate::ui::print_fix_plan(&actions, &ui_cfg, max_risk);
            }
        }
        Commands::Report(args) => {
            let include_evidence = args.include_evidence || cfg.report.include_evidence;
            let mut report = engine.report()?;
            if !include_evidence {
                strip_evidence(&mut report);
            }
            if cli.json {
                write_json(&report)?;
            } else if args.markdown {
                write_markdown_summary(&report, include_evidence)?;
            } else {
                crate::ui::print_doctor(&report, &ui_cfg, 10);
            }
        }
        Commands::Ui(_args) => {
            if cli.json {
                return Err(crate::exit::invalid_args("ui は --json と併用できません"));
            }
            if !(ui_cfg.stdin_is_tty && ui_cfg.stdout_is_tty) {
                return Err(crate::exit::invalid_args(
                    "ui は TTY が必要です（stdin + stdout）",
                ));
            }
            crate::tui::run(
                engine,
                ui_cfg.color,
                cfg.fix.default_risk_max,
                cli.dry_run,
                cfg.scan.default_scope.clone(),
                cfg.scan.exclude.clone(),
            )?;
        }
        Commands::Completion(_args) => {
            let shell = parse_shell(&_args.shell)?;
            let mut cmd = Cli::command();
            let mut out = std::io::stdout().lock();
            clap_complete::generate(shell, &mut cmd, "macdiet", &mut out);
        }
        Commands::Config(_args) => {
            if _args.show {
                if cli.json {
                    let stdout = std::io::stdout();
                    serde_json::to_writer_pretty(stdout.lock(), &cfg)?;
                } else {
                    println!("{}", toml::to_string_pretty(&cfg)?);
                }
            } else if !ui_cfg.quiet {
                eprintln!("config: `macdiet config --show` を使用してください");
            }
        }
    }

    Ok(())
}

fn write_json(report: &crate::core::Report) -> Result<()> {
    use std::io::Write;

    let buf = serde_json::to_vec_pretty(report)?;

    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(&buf) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => return Ok(()),
        Err(err) => return Err(err.into()),
    }
    match stdout.write_all(b"\n") {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn strip_evidence(report: &mut crate::core::Report) {
    for finding in &mut report.findings {
        finding.evidence.clear();
    }
}

fn write_markdown_summary(report: &crate::core::Report, include_evidence: bool) -> Result<()> {
    use std::io::Write;

    let markdown = format_markdown_summary(report, include_evidence);
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(markdown.as_bytes()) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(err) => Err(err.into()),
    }
}

fn format_markdown_summary(report: &crate::core::Report, include_evidence: bool) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();

    let _ = writeln!(out, "# macdiet レポート");
    let _ = writeln!(out);
    let _ = writeln!(out, "- ツールバージョン: {}", report.tool_version);
    let _ = writeln!(out, "- 生成日時: {}", report.generated_at);
    let _ = writeln!(out, "- OS: {} {}", report.os.name, report.os.version);
    let _ = writeln!(
        out,
        "- 推定合計: {}",
        crate::ui::format_bytes(report.summary.estimated_total_bytes)
    );
    if report.summary.unobserved_bytes > 0 {
        let _ = writeln!(
            out,
            "- 未観測: {}",
            crate::ui::format_bytes(report.summary.unobserved_bytes)
        );
    }
    for note in &report.summary.notes {
        let _ = writeln!(out, "- 注記: {note}");
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "## 所見 ({})", report.findings.len());
    if report.findings.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "_所見はありません。_");
    }

    let mut findings: Vec<&crate::core::Finding> = report.findings.iter().collect();
    findings.sort_by_key(|f| (std::cmp::Reverse(f.estimated_bytes), f.id.as_str()));
    for f in findings {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "### {}（推定: {}）",
            f.title,
            crate::ui::format_bytes(f.estimated_bytes)
        );
        let _ = writeln!(out, "- id: `{}`", f.id);
        let _ = writeln!(out, "- リスク: {}", f.risk_level);
        let _ = writeln!(out, "- 確度: {:.2}", f.confidence);
        if !f.recommended_actions.is_empty() {
            let _ = writeln!(out, "- 推奨アクション:");
            let mut ids: Vec<&str> = f
                .recommended_actions
                .iter()
                .map(|a| a.id.as_str())
                .collect();
            ids.sort();
            for id in ids {
                let _ = writeln!(out, "  - `{id}`");
            }
        }
        if include_evidence && !f.evidence.is_empty() {
            let _ = writeln!(out, "- 根拠:");
            for ev in &f.evidence {
                let kind = evidence_kind_name(&ev.kind);
                let value = ev.value.trim_end();
                if value.contains('\n') {
                    let _ = writeln!(out, "  - {kind}:");
                    write_fenced_code_block(&mut out, "    ", "text", value);
                } else {
                    let _ = writeln!(out, "  - {kind}: `{value}`");
                }
            }
        }
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "## アクション ({})", report.actions.len());
    if report.actions.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "_アクションはありません。_");
    }

    let mut actions: Vec<&crate::core::ActionPlan> = report.actions.iter().collect();
    actions.sort_by_key(|a| {
        (
            a.risk_level,
            std::cmp::Reverse(a.estimated_reclaimed_bytes),
            a.id.as_str(),
        )
    });
    for a in actions {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "### {}（推定: {}）",
            a.title,
            crate::ui::format_bytes(a.estimated_reclaimed_bytes)
        );
        let _ = writeln!(out, "- id: `{}`", a.id);
        let _ = writeln!(out, "- リスク: {}", a.risk_level);
        if !a.related_findings.is_empty() {
            let _ = writeln!(out, "- 対象:");
            let mut ids: Vec<&str> = a.related_findings.iter().map(|s| s.as_str()).collect();
            ids.sort();
            for id in ids {
                let _ = writeln!(out, "  - `{id}`");
            }
        }
        let _ = writeln!(out, "- 種類: {}", action_kind_name(&a.kind));
        match &a.kind {
            crate::core::ActionKind::TrashMove { paths }
            | crate::core::ActionKind::Delete { paths } => {
                if !paths.is_empty() {
                    let _ = writeln!(out, "- パス:");
                    let mut sorted: Vec<&str> = paths.iter().map(|p| p.as_str()).collect();
                    sorted.sort();
                    for p in sorted {
                        let _ = writeln!(out, "  - `{p}`");
                    }
                }
            }
            crate::core::ActionKind::RunCmd { cmd, args } => {
                let mut cmdline = cmd.clone();
                for arg in args {
                    cmdline.push(' ');
                    cmdline.push_str(arg);
                }
                let _ = writeln!(out, "- コマンド: `{cmdline}`");
            }
            crate::core::ActionKind::OpenInFinder { path } => {
                let _ = writeln!(out, "- 開く: `{path}`");
            }
            crate::core::ActionKind::ShowInstructions { markdown } => {
                let md = markdown.trim();
                if !md.is_empty() {
                    let _ = writeln!(out);
                    let _ = writeln!(out, "#### 手順");
                    let _ = writeln!(out);
                    let _ = writeln!(out, "{md}");
                }
            }
        }
        if !a.notes.is_empty() {
            let _ = writeln!(out, "- 影響:");
            for note in &a.notes {
                let _ = writeln!(out, "  - {note}");
            }
        }
    }

    let _ = writeln!(out);
    out
}

fn action_kind_name(kind: &crate::core::ActionKind) -> &'static str {
    match kind {
        crate::core::ActionKind::TrashMove { .. } => "ゴミ箱へ移動（TRASH_MOVE）",
        crate::core::ActionKind::Delete { .. } => "削除（DELETE）",
        crate::core::ActionKind::RunCmd { .. } => "コマンド実行（RUN_CMD）",
        crate::core::ActionKind::OpenInFinder { .. } => "Finderで開く（OPEN_IN_FINDER）",
        crate::core::ActionKind::ShowInstructions { .. } => "手順表示（SHOW_INSTRUCTIONS）",
    }
}

fn evidence_kind_name(kind: &crate::core::EvidenceKind) -> &'static str {
    match kind {
        crate::core::EvidenceKind::Path => "パス",
        crate::core::EvidenceKind::Command => "コマンド",
        crate::core::EvidenceKind::Stat => "統計",
    }
}

fn write_fenced_code_block(out: &mut String, indent: &str, lang: &str, content: &str) {
    use std::fmt::Write as _;

    let _ = writeln!(out, "{indent}```{lang}");
    for line in content.lines() {
        let _ = writeln!(out, "{indent}{line}");
    }
    let _ = writeln!(out, "{indent}```");
}

fn format_cmdline(cmd: &str, args: &[String]) -> String {
    let mut out = String::from(cmd);
    for arg in args {
        out.push(' ');
        out.push_str(arg);
    }
    out
}

#[derive(Debug, Clone)]
struct FixApplyPartition {
    trash_actions: Vec<crate::core::ActionPlan>,
    allowlisted_run_cmd_actions: Vec<crate::core::ActionPlan>,
    skipped_actions: Vec<FixApplySkippedAction>,
}

#[derive(Debug, Clone)]
struct FixApplySkippedAction {
    action: crate::core::ActionPlan,
    reason: String,
}

fn partition_fix_apply_actions(actions: &[crate::core::ActionPlan]) -> FixApplyPartition {
    use crate::core::{ActionKind, RiskLevel};

    let mut trash_actions = Vec::<crate::core::ActionPlan>::new();
    let mut allowlisted_run_cmd_actions = Vec::<crate::core::ActionPlan>::new();
    let mut skipped_actions = Vec::<FixApplySkippedAction>::new();

    for action in actions {
        match &action.kind {
            ActionKind::TrashMove { .. } => {
                if action.risk_level <= RiskLevel::R1 {
                    trash_actions.push(action.clone());
                } else {
                    skipped_actions.push(FixApplySkippedAction {
                        action: action.clone(),
                        reason: format!(
                            "v0.1 の --apply で実行できる TRASH_MOVE は R1 のみです（現在: {}）",
                            action.risk_level,
                        ),
                    });
                }
            }
            ActionKind::RunCmd { cmd, args } => {
                if crate::actions::allowlisted_run_cmd(action).is_some() {
                    allowlisted_run_cmd_actions.push(action.clone());
                } else {
                    skipped_actions.push(FixApplySkippedAction {
                        action: action.clone(),
                        reason: format!(
                            "許可リスト外の RUN_CMD のため実行しません（提案のみ）: {}",
                            format_cmdline(cmd, args)
                        ),
                    });
                }
            }
            other => {
                skipped_actions.push(FixApplySkippedAction {
                    action: action.clone(),
                    reason: format!(
                        "v0.1 の --apply では実行しません（{}）",
                        action_kind_name(other)
                    ),
                });
            }
        }
    }

    trash_actions.sort_by_key(|a| (std::cmp::Reverse(a.estimated_reclaimed_bytes), a.id.clone()));
    allowlisted_run_cmd_actions
        .sort_by_key(|a| (std::cmp::Reverse(a.estimated_reclaimed_bytes), a.id.clone()));
    skipped_actions.sort_by_key(|s| (s.action.risk_level, s.action.id.clone()));

    FixApplyPartition {
        trash_actions,
        allowlisted_run_cmd_actions,
        skipped_actions,
    }
}

fn print_fix_apply_partition(part: &FixApplyPartition, cfg: &crate::ui::UiConfig) {
    use std::io::Write as _;

    if cfg.quiet {
        return;
    }

    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "fix --apply（実行対象の整理）");
    let _ = writeln!(
        out,
        "- 実行対象（R1/TRASH_MOVE）: {} 件",
        part.trash_actions.len()
    );
    let _ = writeln!(
        out,
        "- 実行対象（許可リスト/RUN_CMD）: {} 件",
        part.allowlisted_run_cmd_actions.len()
    );
    let _ = writeln!(
        out,
        "- 対象外（プレビューのみ）: {} 件",
        part.skipped_actions.len()
    );

    if !part.skipped_actions.is_empty() {
        let max = if cfg.verbose { 12 } else { 6 };
        let _ = writeln!(out, "\n対象外（プレビューのみ）:");
        for s in part.skipped_actions.iter().take(max) {
            let _ = writeln!(
                out,
                "- {} [{}] id={} — {}",
                s.action.title, s.action.risk_level, s.action.id, s.reason
            );
        }
        if part.skipped_actions.len() > max {
            let omitted = part.skipped_actions.len().saturating_sub(max);
            let _ = writeln!(out, "- …（省略: {omitted} 件）");
        }
        let _ = writeln!(
            out,
            "ヒント: 実行対象だけ選ぶには `macdiet fix --interactive --apply`、特定するには `--target <action_id>` を使ってください。"
        );
    }

    let _ = writeln!(out);
}

fn confirm_exact(prompt: &str, expected: &str) -> Result<bool> {
    use std::io::{BufRead, Write};

    let mut stderr = std::io::stderr().lock();
    write!(stderr, "{prompt}")?;
    stderr.flush()?;

    let mut input = String::new();
    let mut stdin = std::io::stdin().lock();
    let n = stdin.read_line(&mut input)?;
    if n == 0 {
        return Ok(false);
    }
    Ok(input.trim() == expected)
}

fn parse_shell(s: &str) -> Result<clap_complete::Shell> {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "bash" => Ok(clap_complete::Shell::Bash),
        "zsh" => Ok(clap_complete::Shell::Zsh),
        "fish" => Ok(clap_complete::Shell::Fish),
        other => Err(crate::exit::invalid_args(format!(
            "未対応のシェルです: {other}（bash|zsh|fish を指定してください）"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ActionKind, ActionPlan, RiskLevel};

    fn trash_action(id: &str, risk: RiskLevel) -> ActionPlan {
        ActionPlan {
            id: id.to_string(),
            title: id.to_string(),
            risk_level: risk,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec!["~/Library/Caches/Test".to_string()],
            },
            notes: vec![],
        }
    }

    fn run_cmd_action(id: &str, risk: RiskLevel, cmd: &str, args: &[&str]) -> ActionPlan {
        ActionPlan {
            id: id.to_string(),
            title: id.to_string(),
            risk_level: risk,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::RunCmd {
                cmd: cmd.to_string(),
                args: args.iter().map(|s| (*s).to_string()).collect(),
            },
            notes: vec![],
        }
    }

    fn instructions_action(id: &str, risk: RiskLevel) -> ActionPlan {
        ActionPlan {
            id: id.to_string(),
            title: id.to_string(),
            risk_level: risk,
            estimated_reclaimed_bytes: 0,
            related_findings: vec![],
            kind: ActionKind::ShowInstructions {
                markdown: "test".to_string(),
            },
            notes: vec![],
        }
    }

    #[test]
    fn partition_fix_apply_actions_separates_eligible_and_skipped() {
        let actions = vec![
            trash_action("t1", RiskLevel::R1),
            run_cmd_action(
                "homebrew-cache-cleanup",
                RiskLevel::R1,
                "brew",
                &["cleanup"],
            ),
            run_cmd_action(
                "coresimulator-simctl-delete-unavailable",
                RiskLevel::R2,
                "xcrun",
                &["simctl", "delete", "unavailable"],
            ),
            run_cmd_action(
                "docker-system-prune",
                RiskLevel::R2,
                "docker",
                &["system", "prune", "-a", "-f"],
            ),
            instructions_action("info", RiskLevel::R0),
        ];

        let part = partition_fix_apply_actions(&actions);
        assert_eq!(part.trash_actions.len(), 1);
        assert_eq!(part.trash_actions[0].id, "t1");
        assert_eq!(part.allowlisted_run_cmd_actions.len(), 2);
        assert!(
            part.allowlisted_run_cmd_actions
                .iter()
                .any(|a| a.id == "homebrew-cache-cleanup")
        );
        assert!(
            part.allowlisted_run_cmd_actions
                .iter()
                .any(|a| a.id == "coresimulator-simctl-delete-unavailable")
        );
        assert_eq!(part.skipped_actions.len(), 2);
        assert!(
            part.skipped_actions
                .iter()
                .any(|s| s.action.id == "docker-system-prune")
        );
        assert!(part.skipped_actions.iter().any(|s| s.action.id == "info"));
    }
}
