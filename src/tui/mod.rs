use std::collections::HashSet;
use std::io;
use std::panic;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use time::OffsetDateTime;

use crate::core::{ActionKind, Report, RiskLevel};
use crate::engine::{Engine, ScanRequest};

pub fn run(
    engine: Engine,
    color: bool,
    default_fix_risk: RiskLevel,
    dry_run: bool,
    scan_default_scope: String,
    scan_exclude: Vec<String>,
) -> Result<()> {
    enable_raw_mode().context("raw mode の有効化")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("代替画面への切り替え")?;

    let mut tui = Tui {
        terminal: Terminal::new(CrosstermBackend::new(stdout)).context("ターミナルの初期化")?,
    };
    tui.terminal.clear().ok();

    let res = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        run_app(
            &mut tui.terminal,
            engine,
            color,
            default_fix_risk,
            dry_run,
            scan_default_scope,
            scan_exclude,
        )
    }));

    let _ = tui.terminal.show_cursor();
    let _ = disable_raw_mode();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen);

    match res {
        Ok(res) => res,
        Err(_) => Err(anyhow::anyhow!(
            "TUI 内部で panic が発生しました（端末状態は復旧済みのはずです）"
        )),
    }
}

struct Tui {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Home,
    Running,
    ScanConfig,
    LogsList,
    LogsDetail,
    ReportView,
    Utilities,
    CleanupView,
    FixView,
    FixConfirm,
    FixResult,
    FixRunCmdConfirm,
    FixRunCmdResult,
    Error,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Findings = 0,
    Actions = 1,
    Notes = 2,
}

impl Tab {
    fn next(self) -> Self {
        match self {
            Tab::Findings => Tab::Actions,
            Tab::Actions => Tab::Notes,
            Tab::Notes => Tab::Findings,
        }
    }

    fn prev(self) -> Self {
        match self {
            Tab::Findings => Tab::Notes,
            Tab::Actions => Tab::Findings,
            Tab::Notes => Tab::Actions,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommandKind {
    Doctor,
    ScanDeep,
    SnapshotsStatus,
    FixDryRun,
    Utilities,
    Logs,
}

#[derive(Debug, Clone)]
struct CommandItem {
    title: &'static str,
    description: &'static str,
    kind: CommandKind,
}

struct PendingRun {
    kind: CommandKind,
    rx: mpsc::Receiver<Result<Report>>,
    started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupKind {
    XcodeArchives,
    XcodeDeviceSupport,
    CoreSimulatorUnavailable,
}

struct PendingCleanup {
    kind: CleanupKind,
    rx: mpsc::Receiver<Result<Vec<crate::core::ActionPlan>>>,
    started_at: Instant,
}

struct PendingApply {
    rx: mpsc::Receiver<Result<FixApplyResult>>,
    started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixConfirmStage {
    Yes,
    Trash,
}

struct FixConfirm {
    stage: FixConfirmStage,
    input: String,
    actions: Vec<crate::core::ActionPlan>,
    selected_total: usize,
    ignored_total: usize,
    error: Option<String>,
    max_risk: RiskLevel,
    return_to: Screen,
    result_return_to: Screen,
}

struct FixApplyResult {
    max_risk: RiskLevel,
    actions: Vec<crate::core::ActionPlan>,
    outcome: crate::actions::ApplyOutcome,
    log_path: PathBuf,
    return_to: Screen,
}

struct PendingRunCmd {
    rx: mpsc::Receiver<Result<FixRunCmdResult>>,
    started_at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunCmdConfirmStage {
    Token,
    Run,
}

struct FixRunCmdConfirm {
    stage: RunCmdConfirmStage,
    input: String,
    actions: Vec<crate::core::ActionPlan>,
    selected_total: usize,
    ignored_total: usize,
    confirm_token: String,
    final_confirm_token: String,
    error: Option<String>,
    return_to: Screen,
    result_return_to: Screen,
}

struct FixRunCmdActionResult {
    action: crate::core::ActionPlan,
    exit_code: Option<i32>,
    warning: Option<String>,
    error: Option<String>,
    repair_actions: Vec<crate::core::ActionPlan>,
    log_path: Option<PathBuf>,
    log_error: Option<String>,
}

struct FixRunCmdResult {
    results: Vec<FixRunCmdActionResult>,
    return_to: Screen,
}

struct LogEntry {
    file_name: String,
    path: PathBuf,
    size: u64,
    modified_unix_nanos: Option<u128>,
    search_text: String,
}

struct LogDetail {
    entry: LogEntry,
    summary: Vec<String>,
    content: String,
    truncated: bool,
    parse_error: Option<String>,
}

struct App {
    color: bool,
    home_dir: PathBuf,
    dry_run: bool,
    scan_default_scope: String,
    scan_exclude: Vec<String>,
    scan_scope_input: String,
    scan_max_depth_input: String,
    scan_top_dirs_input: String,
    scan_exclude_input: String,
    scan_config_state: ListState,
    scan_edit_mode: bool,
    last_scan_request: Option<ScanRequest>,
    logs_entries: Vec<LogEntry>,
    logs_state: ListState,
    logs_view: Option<LogDetail>,
    logs_scroll: u16,
    utilities_actions: Vec<crate::core::ActionPlan>,
    utilities_state: ListState,
    cleanup_kind: CleanupKind,
    cleanup_actions: Vec<crate::core::ActionPlan>,
    cleanup_state: ListState,
    cleanup_selected: HashSet<String>,
    cleanup_return_to: Screen,
    fix_max_risk: RiskLevel,
    fix_selected: HashSet<String>,
    fix_confirm: Option<FixConfirm>,
    fix_apply_result: Option<FixApplyResult>,
    fix_run_cmd_confirm: Option<FixRunCmdConfirm>,
    fix_run_cmd_result: Option<FixRunCmdResult>,

    screen: Screen,
    help_return_to: Screen,
    error_return_to: Screen,
    tab: Tab,
    show_evidence: bool,

    commands: Vec<CommandItem>,
    query: String,
    query_mode: bool,
    command_state: ListState,

    filter: String,
    filter_mode: bool,

    report: Option<Report>,
    report_kind: Option<CommandKind>,
    error: Option<String>,
    pending: Option<PendingRun>,
    pending_cleanup: Option<PendingCleanup>,
    pending_apply: Option<PendingApply>,
    pending_run_cmd: Option<PendingRunCmd>,

    findings_state: ListState,
    actions_state: ListState,
    fix_state: ListState,
    notes_scroll: u16,

    tick: u64,
}

impl App {
    fn new(
        color: bool,
        home_dir: PathBuf,
        default_fix_risk: RiskLevel,
        dry_run: bool,
        scan_default_scope: String,
        scan_exclude: Vec<String>,
    ) -> Self {
        let commands = vec![
            CommandItem {
                title: "診断（簡易）",
                description: "簡易診断: 上位原因 → 推奨アクション → 未観測のヒント",
                kind: CommandKind::Doctor,
            },
            CommandItem {
                title: "スキャン（深掘り）",
                description: "トップディレクトリを深掘りスキャン（既定値）。大きいツリーでは時間がかかります（ベストエフォート）。",
                kind: CommandKind::ScanDeep,
            },
            CommandItem {
                title: "スナップショット状態",
                description: "Time Machine ローカルスナップショット + APFS スナップショット（ベストエフォート／失敗時は未観測）",
                kind: CommandKind::SnapshotsStatus,
            },
            CommandItem {
                title: "掃除（dry-run）",
                description: "掃除アクションを確認し、R1/TRASH_MOVE を安全に適用（入力による確認）。",
                kind: CommandKind::FixDryRun,
            },
            CommandItem {
                title: "ユーティリティ",
                description:
                    "許可リストの外部コマンドを実行（例: `brew cleanup`）。doctor の所見に依存しません。",
                kind: CommandKind::Utilities,
            },
            CommandItem {
                title: "ログ",
                description: "監査ログ（~/.config/macdiet/logs/）を閲覧（fix apply / snapshots など）。",
                kind: CommandKind::Logs,
            },
        ];

        let mut command_state = ListState::default();
        command_state.select(Some(0));

        let mut findings_state = ListState::default();
        findings_state.select(Some(0));

        let mut actions_state = ListState::default();
        actions_state.select(Some(0));

        let mut fix_state = ListState::default();
        fix_state.select(Some(0));

        let mut scan_config_state = ListState::default();
        scan_config_state.select(Some(0));

        let mut logs_state = ListState::default();
        logs_state.select(Some(0));

        let mut utilities_state = ListState::default();
        utilities_state.select(Some(0));
        let utilities_actions = default_utilities_actions();

        let mut cleanup_state = ListState::default();
        cleanup_state.select(Some(0));

        let scan_scope_input = scan_default_scope.clone();
        let scan_exclude_input = scan_exclude.join(",");

        let scope = scan_scope_input.trim().to_string();
        let default_scope = if scope.is_empty() { None } else { Some(scope) };
        let default_scan_request = ScanRequest {
            scope: default_scope,
            deep: true,
            max_depth: 3,
            top_dirs: 20,
            exclude: scan_exclude.clone(),
            show_progress: false,
        };

        Self {
            color,
            home_dir,
            dry_run,
            scan_default_scope,
            scan_exclude,
            scan_scope_input,
            scan_max_depth_input: "3".to_string(),
            scan_top_dirs_input: "20".to_string(),
            scan_exclude_input,
            scan_config_state,
            scan_edit_mode: false,
            last_scan_request: Some(default_scan_request),
            logs_entries: Vec::new(),
            logs_state,
            logs_view: None,
            logs_scroll: 0,
            utilities_actions,
            utilities_state,
            cleanup_kind: CleanupKind::XcodeArchives,
            cleanup_actions: Vec::new(),
            cleanup_state,
            cleanup_selected: HashSet::new(),
            cleanup_return_to: Screen::Home,
            fix_max_risk: default_fix_risk,
            fix_selected: HashSet::new(),
            fix_confirm: None,
            fix_apply_result: None,
            fix_run_cmd_confirm: None,
            fix_run_cmd_result: None,
            screen: Screen::Home,
            help_return_to: Screen::Home,
            error_return_to: Screen::Home,
            tab: Tab::Findings,
            show_evidence: false,
            commands,
            query: String::new(),
            query_mode: false,
            command_state,
            filter: String::new(),
            filter_mode: false,
            report: None,
            report_kind: None,
            error: None,
            pending: None,
            pending_cleanup: None,
            pending_apply: None,
            pending_run_cmd: None,
            findings_state,
            actions_state,
            fix_state,
            notes_scroll: 0,
            tick: 0,
        }
    }

    fn filtered_command_indices(&self) -> Vec<usize> {
        let q = self.query.trim().to_ascii_lowercase();
        if q.is_empty() {
            return (0..self.commands.len()).collect();
        }
        self.commands
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                let hay = format!("{} {}", c.title, c.description).to_ascii_lowercase();
                hay.contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn selected_command_kind(&self) -> Option<CommandKind> {
        let indices = self.filtered_command_indices();
        let selected = self.command_state.selected().unwrap_or(0);
        let idx = indices.get(selected).copied()?;
        Some(self.commands.get(idx)?.kind)
    }

    fn ensure_command_selection_in_range(&mut self) {
        let n = self.filtered_command_indices().len();
        if n == 0 {
            self.command_state.select(None);
            return;
        }
        let selected = self.command_state.selected().unwrap_or(0);
        let selected = selected.min(n.saturating_sub(1));
        self.command_state.select(Some(selected));
    }

    fn move_command_selection(&mut self, delta: i32) {
        self.ensure_command_selection_in_range();
        let n = self.filtered_command_indices().len();
        if n == 0 {
            return;
        }
        let selected = self.command_state.selected().unwrap_or(0) as i32;
        let next = (selected + delta).clamp(0, (n as i32).saturating_sub(1));
        self.command_state.select(Some(next as usize));
    }

    fn move_list_selection(state: &mut ListState, len: usize, delta: i32) {
        if len == 0 {
            state.select(None);
            return;
        }
        let selected = state.selected().unwrap_or(0) as i32;
        let next = (selected + delta).clamp(0, (len as i32).saturating_sub(1));
        state.select(Some(next as usize));
    }
}

fn screen_supports_filter(screen: Screen) -> bool {
    matches!(
        screen,
        Screen::ReportView
            | Screen::FixView
            | Screen::LogsList
            | Screen::Utilities
            | Screen::CleanupView
    )
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    engine: Engine,
    color: bool,
    default_fix_risk: RiskLevel,
    dry_run: bool,
    scan_default_scope: String,
    scan_exclude: Vec<String>,
) -> Result<()> {
    let home_dir = engine.home_dir().to_path_buf();
    let mut app = App::new(
        color,
        home_dir,
        default_fix_risk,
        dry_run,
        scan_default_scope,
        scan_exclude,
    );

    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|f| draw(f, &mut app)).context("画面描画")?;

        if let Some(pending) = app.pending.take() {
            match pending.rx.try_recv() {
                Ok(res) => match res {
                    Ok(report) => {
                        app.report_kind = Some(pending.kind);
                        app.report = Some(report);
                        app.error = None;
                        match pending.kind {
                            CommandKind::FixDryRun => {
                                app.screen = Screen::FixView;
                                app.fix_state.select(Some(0));
                                trim_fix_selected(&mut app);
                            }
                            _ => {
                                app.screen = Screen::ReportView;
                                app.tab = Tab::Findings;
                                app.findings_state.select(Some(0));
                                app.actions_state.select(Some(0));
                                app.notes_scroll = 0;
                            }
                        }
                    }
                    Err(err) => {
                        open_error_return_to(&mut app, err.to_string(), Screen::Home);
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {
                    if pending.started_at.elapsed() > Duration::from_secs(120) {
                        open_error_return_to(
                            &mut app,
                            "バックグラウンドタスクの完了待ちがタイムアウトしました。".to_string(),
                            Screen::Home,
                        );
                    } else {
                        app.pending = Some(pending);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    open_error_return_to(
                        &mut app,
                        "バックグラウンドタスクとの接続が切れました。".to_string(),
                        Screen::Home,
                    );
                }
            }
        }

        if let Some(pending) = app.pending_cleanup.take() {
            match pending.rx.try_recv() {
                Ok(res) => match res {
                    Ok(actions) => {
                        app.cleanup_kind = pending.kind;
                        app.cleanup_actions = actions;
                        app.cleanup_selected.clear();
                        app.cleanup_state.select(Some(0));
                        app.error = None;
                        app.screen = Screen::CleanupView;
                    }
                    Err(err) => {
                        let return_to = app.cleanup_return_to;
                        open_error_return_to(&mut app, err.to_string(), return_to);
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {
                    if pending.started_at.elapsed() > Duration::from_secs(120) {
                        let return_to = app.cleanup_return_to;
                        open_error_return_to(
                            &mut app,
                            "個別削除候補の取得がタイムアウトしました。".to_string(),
                            return_to,
                        );
                    } else {
                        app.pending_cleanup = Some(pending);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let return_to = app.cleanup_return_to;
                    open_error_return_to(
                        &mut app,
                        "個別削除タスクとの接続が切れました。".to_string(),
                        return_to,
                    );
                }
            }
        }

        if let Some(pending) = app.pending_apply.take() {
            match pending.rx.try_recv() {
                Ok(res) => match res {
                    Ok(result) => {
                        for action in &result.actions {
                            app.fix_selected.remove(&action.id);
                            app.cleanup_selected.remove(&action.id);
                        }
                        app.fix_apply_result = Some(result);
                        app.fix_confirm = None;
                        app.error = None;
                        app.screen = Screen::FixResult;
                    }
                    Err(err) => {
                        app.fix_confirm = None;
                        open_error_return_to(&mut app, err.to_string(), Screen::FixView);
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {
                    if pending.started_at.elapsed() > Duration::from_secs(300) {
                        app.fix_confirm = None;
                        open_error_return_to(
                            &mut app,
                            "fix apply の結果待ちがタイムアウトしました。変更はまだ進行中（または既に適用済み）の可能性があります。".to_string(),
                            Screen::FixView,
                        );
                    } else {
                        app.pending_apply = Some(pending);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    app.fix_confirm = None;
                    open_error_return_to(
                        &mut app,
                        "fix apply タスクとの接続が切れました。変更が一部適用されている可能性があります。"
                            .to_string(),
                        Screen::FixView,
                    );
                }
            }
        }

        if let Some(pending) = app.pending_run_cmd.take() {
            match pending.rx.try_recv() {
                Ok(res) => match res {
                    Ok(result) => {
                        for r in &result.results {
                            app.fix_selected.remove(&r.action.id);
                        }
                        app.fix_run_cmd_result = Some(result);
                        app.fix_run_cmd_confirm = None;
                        app.error = None;
                        app.screen = Screen::FixRunCmdResult;
                    }
                    Err(err) => {
                        app.fix_run_cmd_confirm = None;
                        open_error_return_to(&mut app, err.to_string(), Screen::FixView);
                    }
                },
                Err(mpsc::TryRecvError::Empty) => {
                    if pending.started_at.elapsed() > Duration::from_secs(300) {
                        app.fix_run_cmd_confirm = None;
                        open_error_return_to(
                            &mut app,
                            "RUN_CMD の結果待ちがタイムアウトしました。コマンドはまだ実行中の可能性があります。"
                                .to_string(),
                            Screen::FixView,
                        );
                    } else {
                        app.pending_run_cmd = Some(pending);
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    app.fix_run_cmd_confirm = None;
                    open_error_return_to(
                        &mut app,
                        "RUN_CMD タスクとの接続が切れました。コマンドが一部実行されている可能性があります。"
                            .to_string(),
                        Screen::FixView,
                    );
                }
            }
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout).context("イベント待ち")? {
            match event::read().context("イベント読み取り")? {
                Event::Key(key) => {
                    if key.kind == KeyEventKind::Press {
                        if handle_key(&mut app, &engine, key)? {
                            break;
                        }
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.tick = app.tick.wrapping_add(1);
            last_tick = Instant::now();
        }
    }

    Ok(())
}

fn open_help(app: &mut App) {
    app.help_return_to = app.screen;
    app.screen = Screen::Help;
}

fn open_error_return_to(app: &mut App, msg: impl Into<String>, return_to: Screen) {
    app.error = Some(msg.into());
    app.error_return_to = match return_to {
        Screen::Home
        | Screen::ScanConfig
        | Screen::LogsList
        | Screen::LogsDetail
        | Screen::ReportView
        | Screen::Utilities
        | Screen::CleanupView
        | Screen::FixView
        | Screen::FixConfirm
        | Screen::FixResult
        | Screen::FixRunCmdConfirm
        | Screen::FixRunCmdResult
        | Screen::Help => return_to,
        Screen::Running | Screen::Error => Screen::Home,
    };
    app.screen = Screen::Error;
}

fn open_error(app: &mut App, msg: impl Into<String>) {
    let return_to = app.screen;
    open_error_return_to(app, msg, return_to);
}

fn handle_key(app: &mut App, engine: &Engine, key: KeyEvent) -> Result<bool> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(true);
    }

    if !app.filter_mode
        && screen_supports_filter(app.screen)
        && key.code == KeyCode::Char('/')
        && !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
    {
        app.filter_mode = true;
        return Ok(false);
    }

    if app.filter_mode {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => {
                app.filter_mode = false;
                app.filter = app.filter.trim().to_string();
                if app.screen == Screen::LogsList {
                    refresh_logs_preview(app);
                }
                if app.screen == Screen::Utilities {
                    trim_utilities_selection(app);
                }
                if app.screen == Screen::CleanupView {
                    trim_cleanup_selection(app);
                }
            }
            KeyCode::Backspace => {
                app.filter.pop();
                if app.screen == Screen::LogsList {
                    refresh_logs_preview(app);
                }
                if app.screen == Screen::Utilities {
                    trim_utilities_selection(app);
                }
                if app.screen == Screen::CleanupView {
                    trim_cleanup_selection(app);
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.filter.clear();
                if app.screen == Screen::LogsList {
                    refresh_logs_preview(app);
                }
                if app.screen == Screen::Utilities {
                    trim_utilities_selection(app);
                }
                if app.screen == Screen::CleanupView {
                    trim_cleanup_selection(app);
                }
            }
            KeyCode::Char(c) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT)
                {
                    app.filter.push(c);
                    if app.screen == Screen::LogsList {
                        refresh_logs_preview(app);
                    }
                    if app.screen == Screen::Utilities {
                        trim_utilities_selection(app);
                    }
                    if app.screen == Screen::CleanupView {
                        trim_cleanup_selection(app);
                    }
                }
            }
            _ => {}
        }
        return Ok(false);
    }

    match app.screen {
        Screen::Help => match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => {
                app.screen = app.help_return_to;
            }
            _ => {}
        },
        Screen::Home => match key.code {
            KeyCode::Char('q') if !app.query_mode => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char(':') => {
                app.query_mode = !app.query_mode;
                if !app.query_mode {
                    app.query = app.query.trim().to_string();
                }
            }
            KeyCode::Esc => {
                if app.query_mode {
                    app.query_mode = false;
                }
            }
            KeyCode::Up => app.move_command_selection(-1),
            KeyCode::Down => app.move_command_selection(1),
            KeyCode::Char('k') if !app.query_mode => app.move_command_selection(-1),
            KeyCode::Char('j') if !app.query_mode => app.move_command_selection(1),
            KeyCode::Enter => {
                if let Some(kind) = app.selected_command_kind() {
                    match kind {
                        CommandKind::ScanDeep => open_scan_config(app),
                        CommandKind::Utilities => open_utilities(app),
                        CommandKind::Logs => open_logs(app),
                        _ => start_run(app, engine.clone(), kind),
                    }
                }
            }
            KeyCode::Backspace => {
                if app.query_mode {
                    app.query.pop();
                    app.ensure_command_selection_in_range();
                }
            }
            KeyCode::Char(c) => {
                if app.query_mode {
                    app.query.push(c);
                    app.ensure_command_selection_in_range();
                }
            }
            _ => {}
        },
        Screen::ScanConfig => match key.code {
            _ if app.scan_edit_mode => match key.code {
                KeyCode::Enter | KeyCode::Esc => {
                    app.scan_edit_mode = false;
                    trim_scan_inputs(app);
                }
                KeyCode::Backspace => {
                    scan_field_mut(app).pop();
                }
                KeyCode::Char(c) => {
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                    {
                        scan_field_mut(app).push(c);
                    }
                }
                _ => {}
            },
            _ => match key.code {
                KeyCode::Char('q') => return Ok(true),
                KeyCode::Char('?') => open_help(app),
                KeyCode::Char('b') | KeyCode::Esc => app.screen = Screen::Home,
                KeyCode::Up | KeyCode::Char('k') => {
                    App::move_list_selection(&mut app.scan_config_state, 4, -1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    App::move_list_selection(&mut app.scan_config_state, 4, 1)
                }
                KeyCode::Enter => app.scan_edit_mode = true,
                KeyCode::Char('r') => start_scan_from_inputs(app, engine.clone()),
                _ => {}
            },
        },
        Screen::LogsList => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => app.screen = Screen::Home,
            KeyCode::Char('r') => refresh_logs(app),
            KeyCode::Up | KeyCode::Char('k') => {
                let indices = logs_filtered_indices(&app.logs_entries, &app.filter);
                App::move_list_selection(&mut app.logs_state, indices.len(), -1);
                refresh_logs_preview(app);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let indices = logs_filtered_indices(&app.logs_entries, &app.filter);
                App::move_list_selection(&mut app.logs_state, indices.len(), 1);
                refresh_logs_preview(app);
            }
            KeyCode::Enter => open_selected_log_detail(app),
            _ => {}
        },
        Screen::LogsDetail => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => {
                app.logs_scroll = 0;
                app.screen = Screen::LogsList;
            }
            KeyCode::Char('r') => reload_log_detail(app),
            KeyCode::Up | KeyCode::Char('k') => app.logs_scroll = app.logs_scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                app.logs_scroll = app.logs_scroll.saturating_add(1)
            }
            _ => {}
        },
        Screen::Running => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Esc => {
                if app.pending_apply.is_some() || app.pending_run_cmd.is_some() {
                    // Cannot cancel destructive work safely; ignore.
                } else if app.pending_cleanup.is_some() {
                    app.pending_cleanup = None;
                    app.screen = app.cleanup_return_to;
                } else {
                    app.pending = None;
                    app.screen = Screen::Home;
                }
            }
            KeyCode::Char('?') => open_help(app),
            _ => {}
        },
        Screen::Error => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => {
                app.screen = app.error_return_to;
                app.error = None;
            }
            KeyCode::Char('r') => {
                if let Some(kind) = app.report_kind.or(Some(CommandKind::Doctor)) {
                    start_run(app, engine.clone(), kind);
                }
            }
            _ => {}
        },
        Screen::ReportView => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => {
                app.screen = Screen::Home;
            }
            KeyCode::Char('r') => {
                if let Some(kind) = app.report_kind {
                    start_run(app, engine.clone(), kind);
                }
            }
            KeyCode::Char('e') => app.show_evidence = !app.show_evidence,
            KeyCode::Tab => app.tab = app.tab.next(),
            KeyCode::BackTab => app.tab = app.tab.prev(),
            KeyCode::Up | KeyCode::Char('k') => match app.tab {
                Tab::Findings => {
                    if let Some(report) = &app.report {
                        let indices = report_filtered_finding_indices(report, &app.filter);
                        App::move_list_selection(&mut app.findings_state, indices.len(), -1);
                    }
                }
                Tab::Actions => {
                    if let Some(report) = &app.report {
                        let indices = report_filtered_action_indices(report, &app.filter);
                        App::move_list_selection(&mut app.actions_state, indices.len(), -1);
                    }
                }
                Tab::Notes => {
                    app.notes_scroll = app.notes_scroll.saturating_sub(1);
                }
            },
            KeyCode::Down | KeyCode::Char('j') => match app.tab {
                Tab::Findings => {
                    if let Some(report) = &app.report {
                        let indices = report_filtered_finding_indices(report, &app.filter);
                        App::move_list_selection(&mut app.findings_state, indices.len(), 1);
                    }
                }
                Tab::Actions => {
                    if let Some(report) = &app.report {
                        let indices = report_filtered_action_indices(report, &app.filter);
                        App::move_list_selection(&mut app.actions_state, indices.len(), 1);
                    }
                }
                Tab::Notes => {
                    app.notes_scroll = app.notes_scroll.saturating_add(1);
                }
            },
            _ => {}
        },
        Screen::Utilities => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => app.screen = Screen::Home,
            KeyCode::Char('1') => {
                app.fix_max_risk = RiskLevel::R1;
                trim_utilities_selection(app);
            }
            KeyCode::Char('2') => {
                app.fix_max_risk = RiskLevel::R2;
                trim_utilities_selection(app);
            }
            KeyCode::Char('3') => {
                app.fix_max_risk = RiskLevel::R3;
                trim_utilities_selection(app);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let indices = utilities_filtered_indices(app);
                App::move_list_selection(&mut app.utilities_state, indices.len(), -1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let indices = utilities_filtered_indices(app);
                App::move_list_selection(&mut app.utilities_state, indices.len(), 1);
            }
            KeyCode::Char('x') | KeyCode::Enter => {
                let indices = utilities_filtered_indices(app);
                let Some(sel) = app.utilities_state.selected() else {
                    return Ok(false);
                };
                let Some(idx) = indices.get(sel).copied() else {
                    return Ok(false);
                };
                let Some(action) = app.utilities_actions.get(idx).cloned() else {
                    return Ok(false);
                };
                start_fix_run_cmd_confirm_for_action(app, action)?;
            }
            _ => {}
        },
        Screen::FixView => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => app.screen = Screen::Home,
            KeyCode::Char('r') => start_run(app, engine.clone(), CommandKind::FixDryRun),
            KeyCode::Char('1') => {
                app.fix_max_risk = RiskLevel::R1;
                trim_fix_selected(app);
            }
            KeyCode::Char('2') => {
                app.fix_max_risk = RiskLevel::R2;
                trim_fix_selected(app);
            }
            KeyCode::Char('3') => {
                app.fix_max_risk = RiskLevel::R3;
                trim_fix_selected(app);
            }
            KeyCode::Char('a') => select_all_fix_candidates(app),
            KeyCode::Char('n') => app.fix_selected.clear(),
            KeyCode::Char('p') => start_fix_apply_confirm(app)?,
            KeyCode::Char('x') => start_fix_run_cmd_confirm(app)?,
            KeyCode::Char('c') => open_cleanup_from_fix_view(app, engine.timeout())?,
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(report) = app.report.as_ref() {
                    let candidates =
                        fix_filtered_candidate_indices(report, app.fix_max_risk, &app.filter);
                    App::move_list_selection(&mut app.fix_state, candidates.len(), -1);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(report) = app.report.as_ref() {
                    let candidates =
                        fix_filtered_candidate_indices(report, app.fix_max_risk, &app.filter);
                    App::move_list_selection(&mut app.fix_state, candidates.len(), 1);
                }
            }
            KeyCode::Char(' ') => toggle_fix_selected(app),
            KeyCode::Enter => start_fix_primary_action_for_cursor(app, engine.timeout())?,
            _ => {}
        },
        Screen::FixConfirm => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Esc => {
                let return_to = app
                    .fix_confirm
                    .as_ref()
                    .map(|c| c.return_to)
                    .unwrap_or(Screen::FixView);
                app.fix_confirm = None;
                app.screen = return_to;
            }
            KeyCode::Backspace => {
                if let Some(confirm) = app.fix_confirm.as_mut() {
                    confirm.input.pop();
                }
            }
            KeyCode::Enter => submit_fix_confirm(app)?,
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return Ok(false);
                }
                if let Some(confirm) = app.fix_confirm.as_mut() {
                    confirm.input.push(c);
                }
            }
            _ => {}
        },
        Screen::FixResult => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => {
                let return_to = app
                    .fix_apply_result
                    .as_ref()
                    .map(|r| r.return_to)
                    .unwrap_or(Screen::FixView);
                match return_to {
                    Screen::FixView => start_run(app, engine.clone(), CommandKind::FixDryRun),
                    Screen::CleanupView => start_cleanup_refresh(app, engine.timeout()),
                    _ => app.screen = return_to,
                }
            }
            KeyCode::Char('r') => {
                let return_to = app
                    .fix_apply_result
                    .as_ref()
                    .map(|r| r.return_to)
                    .unwrap_or(Screen::FixView);
                match return_to {
                    Screen::FixView => start_run(app, engine.clone(), CommandKind::FixDryRun),
                    Screen::CleanupView => start_cleanup_refresh(app, engine.timeout()),
                    _ => {}
                }
            }
            _ => {}
        },
        Screen::FixRunCmdConfirm => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Esc => {
                let return_to = app
                    .fix_run_cmd_confirm
                    .as_ref()
                    .map(|c| c.return_to)
                    .unwrap_or(Screen::FixView);
                app.fix_run_cmd_confirm = None;
                app.screen = return_to;
            }
            KeyCode::Backspace => {
                if let Some(confirm) = app.fix_run_cmd_confirm.as_mut() {
                    confirm.input.pop();
                }
            }
            KeyCode::Enter => submit_fix_run_cmd_confirm(app, engine.timeout())?,
            KeyCode::Char(c) => {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    || key.modifiers.contains(KeyModifiers::ALT)
                {
                    return Ok(false);
                }
                if let Some(confirm) = app.fix_run_cmd_confirm.as_mut() {
                    confirm.input.push(c);
                }
            }
            _ => {}
        },
        Screen::FixRunCmdResult => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => {
                let return_to = app
                    .fix_run_cmd_result
                    .as_ref()
                    .map(|r| r.return_to)
                    .unwrap_or(Screen::FixView);
                if return_to == Screen::FixView {
                    start_run(app, engine.clone(), CommandKind::FixDryRun);
                } else {
                    app.screen = return_to;
                }
            }
            KeyCode::Char('f') => {
                let repair = app
                    .fix_run_cmd_result
                    .as_ref()
                    .and_then(|r| r.results.first())
                    .and_then(|r| r.repair_actions.get(0).cloned());
                if let Some(action) = repair {
                    start_fix_run_cmd_confirm_for_action(app, action)?;
                }
            }
            KeyCode::Char('g') => {
                let repair = app
                    .fix_run_cmd_result
                    .as_ref()
                    .and_then(|r| r.results.first())
                    .and_then(|r| r.repair_actions.get(1).cloned());
                if let Some(action) = repair {
                    start_fix_run_cmd_confirm_for_action(app, action)?;
                }
            }
            KeyCode::Char('r') => start_run(app, engine.clone(), CommandKind::FixDryRun),
            _ => {}
        },
        Screen::CleanupView => match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => open_help(app),
            KeyCode::Char('b') | KeyCode::Esc => {
                app.screen = app.cleanup_return_to;
            }
            KeyCode::Char('r') => start_cleanup_refresh(app, engine.timeout()),
            KeyCode::Char('1') => {
                app.fix_max_risk = RiskLevel::R1;
                trim_cleanup_selection(app);
            }
            KeyCode::Char('2') => {
                app.fix_max_risk = RiskLevel::R2;
                trim_cleanup_selection(app);
            }
            KeyCode::Char('3') => {
                app.fix_max_risk = RiskLevel::R3;
                trim_cleanup_selection(app);
            }
            KeyCode::Char('a') => select_all_cleanup_candidates(app),
            KeyCode::Char('n') => app.cleanup_selected.clear(),
            KeyCode::Char('p') => start_cleanup_apply_confirm(app)?,
            KeyCode::Up | KeyCode::Char('k') => {
                let indices = cleanup_filtered_indices(app);
                App::move_list_selection(&mut app.cleanup_state, indices.len(), -1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let indices = cleanup_filtered_indices(app);
                App::move_list_selection(&mut app.cleanup_state, indices.len(), 1);
            }
            KeyCode::Char(' ') | KeyCode::Enter => toggle_cleanup_selected(app),
            _ => {}
        },
    }

    Ok(false)
}

fn start_run(app: &mut App, engine: Engine, kind: CommandKind) {
    let (tx, rx) = mpsc::channel::<Result<Report>>();
    let scan_req = if kind == CommandKind::ScanDeep {
        Some(
            app.last_scan_request
                .clone()
                .unwrap_or_else(|| default_scan_request(app)),
        )
    } else {
        None
    };
    thread::spawn(move || {
        let res = match kind {
            CommandKind::Doctor => engine.doctor(),
            CommandKind::ScanDeep => engine.scan(scan_req.expect("スキャンリクエスト（内部）")),
            CommandKind::SnapshotsStatus => engine.snapshots_status(),
            CommandKind::FixDryRun => engine.doctor(),
            CommandKind::Utilities => Err(anyhow::anyhow!(
                "内部エラー: Utilities は実行対象のコマンドではありません"
            )),
            CommandKind::Logs => Err(anyhow::anyhow!(
                "内部エラー: Logs は実行対象のコマンドではありません"
            )),
        };
        let _ = tx.send(res);
    });
    app.pending = Some(PendingRun {
        kind,
        rx,
        started_at: Instant::now(),
    });
    app.screen = Screen::Running;
    app.error = None;
}

fn open_cleanup_from_fix_view(app: &mut App, timeout: Duration) -> Result<()> {
    let Some(report) = app.report.as_ref() else {
        open_error_return_to(
            app,
            "レポートがありません。先に「掃除（dry-run）」を実行してください。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    };

    if app.fix_max_risk < RiskLevel::R2 {
        open_error_return_to(
            app,
            format!(
                "最大リスクが {} のため、個別削除（R2）を開けません。\nヒント: 2 を押して R2 を含めてください。",
                app.fix_max_risk
            ),
            Screen::FixView,
        );
        return Ok(());
    }

    let candidates = fix_filtered_candidate_indices(report, app.fix_max_risk, &app.filter);
    let Some(sel) = app.fix_state.selected() else {
        return Ok(());
    };
    let Some(idx) = candidates.get(sel).copied() else {
        return Ok(());
    };
    let Some(action) = report.actions.get(idx) else {
        return Ok(());
    };

    let kind = match action.id.as_str() {
        "xcode-archives-review" => CleanupKind::XcodeArchives,
        "xcode-device-support-review" => CleanupKind::XcodeDeviceSupport,
        "coresimulator-devices-xcrun" => CleanupKind::CoreSimulatorUnavailable,
        _ => {
            open_error_return_to(
                app,
                "このアクションでは個別削除（ゴミ箱へ移動）を利用できません。".to_string(),
                Screen::FixView,
            );
            return Ok(());
        }
    };

    app.cleanup_kind = kind;
    app.cleanup_actions.clear();
    app.cleanup_selected.clear();
    app.cleanup_state.select(Some(0));
    app.cleanup_return_to = Screen::FixView;

    start_cleanup_refresh(app, timeout);
    Ok(())
}

fn current_fix_view_action<'a>(report: &'a Report, app: &App) -> Option<&'a crate::core::ActionPlan> {
    let candidates = fix_filtered_candidate_indices(report, app.fix_max_risk, &app.filter);
    let sel = app.fix_state.selected()?;
    let idx = *candidates.get(sel)?;
    report.actions.get(idx)
}

fn start_fix_primary_action_for_cursor(app: &mut App, timeout: Duration) -> Result<()> {
    if app.dry_run {
        open_error_return_to(
            app,
            "dry-run モード: 破壊的操作は無効です。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    }

    let Some(report) = app.report.as_ref() else {
        open_error_return_to(
            app,
            "レポートがありません。先に「掃除（dry-run）」を実行してください。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    };

    let Some(action) = current_fix_view_action(report, app).cloned() else {
        return Ok(());
    };

    if matches!(
        action.id.as_str(),
        "xcode-archives-review" | "xcode-device-support-review" | "coresimulator-devices-xcrun"
    ) {
        return open_cleanup_from_fix_view(app, timeout);
    }

    if crate::actions::allowlisted_run_cmd(&action).is_some() {
        return start_fix_run_cmd_confirm_for_action(app, action);
    }

    if action.risk_level == RiskLevel::R1 && matches!(action.kind, ActionKind::TrashMove { .. }) {
        return start_fix_apply_confirm_for_action(app, action);
    }

    open_error_return_to(
        app,
        "このアクションはTUIから直接実行できません（手順表示/プレビューのみ）。右ペインの手順を確認してください。"
            .to_string(),
        Screen::FixView,
    );
    Ok(())
}

fn start_cleanup_refresh(app: &mut App, timeout: Duration) {
    let home_dir = app.home_dir.clone();
    let kind = app.cleanup_kind;
    let (tx, rx) = mpsc::channel::<Result<Vec<crate::core::ActionPlan>>>();
    thread::spawn(move || {
        let res = build_cleanup_actions(kind, &home_dir, timeout);
        let _ = tx.send(res);
    });
    app.pending_cleanup = Some(PendingCleanup {
        kind,
        rx,
        started_at: Instant::now(),
    });
    app.screen = Screen::Running;
    app.error = None;
}

fn start_cleanup_apply_confirm(app: &mut App) -> Result<()> {
    if app.dry_run {
        open_error_return_to(
            app,
            "dry-run モード: 個別削除（ゴミ箱へ移動）は無効です。".to_string(),
            Screen::CleanupView,
        );
        return Ok(());
    }
    if app.fix_max_risk < RiskLevel::R2 {
        open_error_return_to(
            app,
            format!(
                "最大リスクが {} のため、個別削除（R2）を適用できません。\nヒント: 2 を押して R2 を含めてください。",
                app.fix_max_risk
            ),
            Screen::CleanupView,
        );
        return Ok(());
    }

    let selected: Vec<crate::core::ActionPlan> = app
        .cleanup_actions
        .iter()
        .filter(|a| app.cleanup_selected.contains(&a.id))
        .cloned()
        .collect();
    if selected.is_empty() {
        open_error_return_to(
            app,
            "適用可能な項目が選択されていません。".to_string(),
            Screen::CleanupView,
        );
        return Ok(());
    }

    crate::actions::validate_actions(&selected, &app.home_dir)?;

    app.fix_confirm = Some(FixConfirm {
        stage: FixConfirmStage::Yes,
        input: String::new(),
        selected_total: selected.len(),
        ignored_total: 0,
        actions: selected,
        error: None,
        max_risk: RiskLevel::R2,
        return_to: Screen::CleanupView,
        result_return_to: Screen::CleanupView,
    });
    app.screen = Screen::FixConfirm;
    Ok(())
}

fn start_fix_apply_confirm(app: &mut App) -> Result<()> {
    if app.dry_run {
        open_error_return_to(
            app,
            "dry-run モード: fix apply は無効です。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    }

    let Some(report) = app.report.as_ref() else {
        open_error_return_to(
            app,
            "レポートがありません。先に「掃除（dry-run）」を実行してください。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    };

    let (actions, selected_total, ignored_total) =
        selected_fix_apply_actions(report, &app.fix_selected);

    if actions.is_empty() {
        let (run_cmd_actions, _, _, _) = selected_fix_run_cmd_actions(report, &app.fix_selected);
        if !run_cmd_actions.is_empty() {
            return start_fix_run_cmd_confirm(app);
        }

        let msg = if selected_total == 0 {
            "適用可能なアクションが選択されていません。\nヒント: UI Phase 3 では R1/TRASH_MOVE のみ適用できます。"
                .to_string()
        } else {
            use std::collections::HashSet;

            let mut selected_related = HashSet::<&str>::new();
            for action in &report.actions {
                if app.fix_selected.contains(&action.id) {
                    for fid in &action.related_findings {
                        selected_related.insert(fid.as_str());
                    }
                }
            }

            let mut suggestions = Vec::<(&str, &str)>::new();
            for action in &report.actions {
                if action.risk_level != RiskLevel::R1 {
                    continue;
                }
                if !matches!(action.kind, ActionKind::TrashMove { .. }) {
                    continue;
                }
                if action
                    .related_findings
                    .iter()
                    .any(|fid| selected_related.contains(fid.as_str()))
                {
                    suggestions.push((action.title.as_str(), action.id.as_str()));
                }
            }
            suggestions.sort();
            suggestions.truncate(6);

            let mut out = format!(
                "選択した {selected_total} 件は「適用（R1）」の対象外です（例: 手順表示）。\nヒント: UI Phase 3 では R1/TRASH_MOVE のみ適用できます。"
            );
            if suggestions.is_empty() {
                out.push_str(
                    "\nヒント: 「…をゴミ箱へ移動（R1）」のような TRASH_MOVE を選択してください。",
                );
            } else {
                out.push_str("\nヒント: 次を選択してください:");
                for (title, id) in &suggestions {
                    out.push_str(&format!("\n- {title} id={id}"));
                }
            }
            out.push_str("\n注: 手順表示（SHOW_INSTRUCTIONS）は、右ペインに手順を表示するだけで、p では実行されません。");
            out
        };
        open_error_return_to(app, msg, Screen::FixView);
        return Ok(());
    }

    crate::actions::validate_actions(&actions, &app.home_dir)?;

    app.fix_confirm = Some(FixConfirm {
        stage: FixConfirmStage::Yes,
        input: String::new(),
        actions,
        selected_total,
        ignored_total,
        error: None,
        max_risk: RiskLevel::R1,
        return_to: Screen::FixView,
        result_return_to: Screen::FixView,
    });
    app.screen = Screen::FixConfirm;
    Ok(())
}

fn start_fix_apply_confirm_for_action(app: &mut App, action: crate::core::ActionPlan) -> Result<()> {
    if app.dry_run {
        open_error_return_to(
            app,
            "dry-run モード: fix apply は無効です。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    }

    if action.risk_level != RiskLevel::R1 || !matches!(action.kind, ActionKind::TrashMove { .. }) {
        open_error_return_to(
            app,
            "この項目は「ゴミ箱へ移動（R1）」ではありません。\nヒント: RUN_CMD は x、個別削除は c/Enter を使ってください。"
                .to_string(),
            Screen::FixView,
        );
        return Ok(());
    }

    crate::actions::validate_actions(std::slice::from_ref(&action), &app.home_dir)?;

    app.fix_confirm = Some(FixConfirm {
        stage: FixConfirmStage::Yes,
        input: String::new(),
        actions: vec![action],
        selected_total: 1,
        ignored_total: 0,
        error: None,
        max_risk: RiskLevel::R1,
        return_to: Screen::FixView,
        result_return_to: Screen::FixView,
    });
    app.screen = Screen::FixConfirm;
    Ok(())
}

fn start_fix_run_cmd_confirm(app: &mut App) -> Result<()> {
    if app.dry_run {
        open_error_return_to(
            app,
            "dry-run モード: RUN_CMD は無効です。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    }

    let Some(report) = app.report.as_ref() else {
        open_error_return_to(
            app,
            "レポートがありません。先に「掃除（dry-run）」を実行してください。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    };

    let (actions, selected_total, ignored_total, spec) =
        selected_fix_run_cmd_actions(report, &app.fix_selected);

    if actions.is_empty() {
        open_error_return_to(
            app,
            "実行可能な RUN_CMD アクションが選択されていません。\nヒント: 許可リストの RUN_CMD（例: `homebrew-cache-cleanup` / `npm-cache-cleanup` / `yarn-cache-cleanup` / `pnpm-store-prune` / `docker-storage-df` / `docker-builder-prune` / `docker-system-prune` / `coresimulator-simctl-delete-unavailable`）を選択して x を押してください。R2 が表示されない場合は 2 を押してください。"
                .to_string(),
            Screen::FixView,
        );
        return Ok(());
    }
    if actions.len() != 1 {
        open_error_return_to(
            app,
            "安全のため、RUN_CMD の実行は一度に 1 つまでです。\nヒント: 他の選択を外して再試行してください。"
                .to_string(),
            Screen::FixView,
        );
        return Ok(());
    }
    let Some(spec) = spec else {
        open_error_return_to(
            app,
            "許可リストの仕様が見つかりません。".to_string(),
            Screen::FixView,
        );
        return Ok(());
    };

    app.fix_run_cmd_confirm = Some(FixRunCmdConfirm {
        stage: RunCmdConfirmStage::Token,
        input: String::new(),
        actions,
        selected_total,
        ignored_total,
        confirm_token: spec.confirm_token.to_string(),
        final_confirm_token: spec.final_confirm_token.to_string(),
        error: None,
        return_to: Screen::FixView,
        result_return_to: Screen::FixView,
    });
    app.screen = Screen::FixRunCmdConfirm;
    Ok(())
}

fn start_fix_run_cmd_confirm_for_action(
    app: &mut App,
    action: crate::core::ActionPlan,
) -> Result<()> {
    let return_to = app.screen;

    if app.dry_run {
        open_error_return_to(
            app,
            "dry-run モード: RUN_CMD は無効です。".to_string(),
            return_to,
        );
        return Ok(());
    }

    let result_return_to = if return_to == Screen::FixRunCmdResult {
        app.fix_run_cmd_result
            .as_ref()
            .map(|r| r.return_to)
            .unwrap_or(Screen::FixView)
    } else {
        return_to
    };

    if action.risk_level > app.fix_max_risk {
        open_error_return_to(
            app,
            format!(
                "この操作は {} です。最大リスクを {} 以上にしてから再試行してください（1/2/3 キー）。",
                action.risk_level, action.risk_level
            ),
            return_to,
        );
        return Ok(());
    }

    let Some(spec) = crate::actions::allowlisted_run_cmd(&action) else {
        open_error_return_to(
            app,
            "許可リストの仕様が見つかりません。".to_string(),
            return_to,
        );
        return Ok(());
    };

    app.fix_run_cmd_confirm = Some(FixRunCmdConfirm {
        stage: RunCmdConfirmStage::Token,
        input: String::new(),
        actions: vec![action],
        selected_total: 1,
        ignored_total: 0,
        confirm_token: spec.confirm_token.to_string(),
        final_confirm_token: spec.final_confirm_token.to_string(),
        error: None,
        return_to,
        result_return_to,
    });
    app.screen = Screen::FixRunCmdConfirm;
    Ok(())
}

fn submit_fix_confirm(app: &mut App) -> Result<()> {
    if app.dry_run {
        let return_to = app
            .fix_confirm
            .as_ref()
            .map(|c| c.return_to)
            .unwrap_or(Screen::FixView);
        open_error_return_to(
            app,
            "dry-run モード: fix apply は無効です。".to_string(),
            return_to,
        );
        return Ok(());
    }

    let Some(confirm) = app.fix_confirm.as_mut() else {
        open_error_return_to(app, "確認状態がありません。".to_string(), Screen::FixView);
        return Ok(());
    };

    let expected = match confirm.stage {
        FixConfirmStage::Yes => "yes",
        FixConfirmStage::Trash => "trash",
    };

    if confirm.input.trim() != expected {
        confirm.error = Some(format!("続行するには '{expected}' と入力してください。"));
        return Ok(());
    }

    confirm.error = None;
    confirm.input.clear();

    match confirm.stage {
        FixConfirmStage::Yes => {
            confirm.stage = FixConfirmStage::Trash;
        }
        FixConfirmStage::Trash => {
            let confirm = app.fix_confirm.take().expect("confirm");
            start_fix_apply(app, confirm.max_risk, confirm.actions, confirm.result_return_to);
        }
    }

    Ok(())
}

fn submit_fix_run_cmd_confirm(app: &mut App, timeout: Duration) -> Result<()> {
    if app.dry_run {
        let return_to = app
            .fix_run_cmd_confirm
            .as_ref()
            .map(|c| c.return_to)
            .unwrap_or(Screen::FixView);
        open_error_return_to(
            app,
            "dry-run モード: RUN_CMD は無効です。".to_string(),
            return_to,
        );
        return Ok(());
    }

    let Some(confirm) = app.fix_run_cmd_confirm.as_mut() else {
        open_error_return_to(app, "確認状態がありません。".to_string(), Screen::FixView);
        return Ok(());
    };

    let expected = match confirm.stage {
        RunCmdConfirmStage::Token => confirm.confirm_token.as_str(),
        RunCmdConfirmStage::Run => confirm.final_confirm_token.as_str(),
    };

    if confirm.input.trim() != expected {
        confirm.error = Some(format!("続行するには '{expected}' と入力してください。"));
        return Ok(());
    }

    confirm.error = None;
    confirm.input.clear();

    match confirm.stage {
        RunCmdConfirmStage::Token => {
            confirm.stage = RunCmdConfirmStage::Run;
        }
        RunCmdConfirmStage::Run => {
            let confirm = app.fix_run_cmd_confirm.take().expect("confirm");
            start_fix_run_cmd(app, timeout, confirm.actions, confirm.result_return_to);
        }
    }

    Ok(())
}

fn selected_fix_apply_actions(
    report: &Report,
    selected: &HashSet<String>,
) -> (Vec<crate::core::ActionPlan>, usize, usize) {
    let mut selected_total = 0usize;
    let mut actions = Vec::new();

    for action in &report.actions {
        if !selected.contains(&action.id) {
            continue;
        }
        selected_total += 1;
        if action.risk_level != RiskLevel::R1 {
            continue;
        }
        if !matches!(action.kind, ActionKind::TrashMove { .. }) {
            continue;
        }
        actions.push(action.clone());
    }

    actions.sort_by_key(|a| (std::cmp::Reverse(a.estimated_reclaimed_bytes), a.id.clone()));

    let ignored_total = selected_total.saturating_sub(actions.len());
    (actions, selected_total, ignored_total)
}

fn selected_fix_run_cmd_actions(
    report: &Report,
    selected: &HashSet<String>,
) -> (
    Vec<crate::core::ActionPlan>,
    usize,
    usize,
    Option<crate::actions::AllowlistedRunCmdSpec>,
) {
    let mut selected_total = 0usize;
    let mut actions = Vec::new();
    let mut spec = None;

    for action in &report.actions {
        if !selected.contains(&action.id) {
            continue;
        }
        selected_total += 1;
        let Some(s) = crate::actions::allowlisted_run_cmd(action) else {
            continue;
        };
        actions.push(action.clone());
        spec = Some(s);
    }

    actions.sort_by_key(|a| (std::cmp::Reverse(a.estimated_reclaimed_bytes), a.id.clone()));

    let ignored_total = selected_total.saturating_sub(actions.len());
    (actions, selected_total, ignored_total, spec)
}

#[derive(Debug, Clone)]
struct CleanupCandidate {
    path: PathBuf,
    bytes: u64,
    title: String,
    notes: Vec<String>,
}

fn build_cleanup_actions(
    kind: CleanupKind,
    home_dir: &std::path::Path,
    timeout: Duration,
) -> Result<Vec<crate::core::ActionPlan>> {
    let budget = std::cmp::min(timeout, Duration::from_secs(20));
    let deadline = if budget == Duration::from_secs(0) {
        Some(Instant::now())
    } else {
        Some(Instant::now() + budget)
    };

    let per_item_budget = std::cmp::min(timeout, Duration::from_millis(800));

    let candidates = match kind {
        CleanupKind::XcodeArchives => {
            let base = home_dir.join("Library/Developer/Xcode/Archives");
            if !base.exists() {
                Vec::new()
            } else {
                let mut dirs = Vec::<PathBuf>::new();
                let outer = std::fs::read_dir(&base)
                    .with_context(|| format!("ディレクトリを読めません: {}", base.display()))?;
                for e in outer.flatten() {
                    let p = e.path();
                    if !p.is_dir() {
                        continue;
                    }
                    if path_ends_with(p.as_path(), ".xcarchive") {
                        dirs.push(p);
                        continue;
                    }
                    if let Ok(inner) = std::fs::read_dir(&p) {
                        for child in inner.flatten() {
                            let cp = child.path();
                            if cp.is_dir() && path_ends_with(cp.as_path(), ".xcarchive") {
                                dirs.push(cp);
                            }
                        }
                    }
                }

                dirs.sort();
                dirs.dedup();

                dirs.into_iter()
                    .map(|p| {
                        let rel = p.strip_prefix(&base).ok();
                        let title = rel
                            .map(|r| r.display().to_string())
                            .unwrap_or_else(|| p.display().to_string());
                        let bytes = estimate_candidate_bytes(&p, per_item_budget, deadline);
                        CleanupCandidate {
                            path: p,
                            bytes,
                            title: format!("Archive: {title}"),
                            notes: vec![
                                "影響: 過去ビルドの配布・デバッグに必要な場合があります。削除前に内容を確認してください。"
                                    .to_string(),
                            ],
                        }
                    })
                    .collect()
            }
        }
        CleanupKind::XcodeDeviceSupport => {
            let base = home_dir.join("Library/Developer/Xcode/iOS DeviceSupport");
            if !base.exists() {
                Vec::new()
            } else {
                let mut dirs = Vec::<PathBuf>::new();
                let outer = std::fs::read_dir(&base)
                    .with_context(|| format!("ディレクトリを読めません: {}", base.display()))?;
                for e in outer.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        dirs.push(p);
                    }
                }
                dirs.sort();
                dirs.dedup();

                dirs.into_iter()
                    .map(|p| {
                        let rel = p.strip_prefix(&base).ok();
                        let title = rel
                            .map(|r| r.display().to_string())
                            .unwrap_or_else(|| p.display().to_string());
                        let bytes = estimate_candidate_bytes(&p, per_item_budget, deadline);
                        CleanupCandidate {
                            path: p,
                            bytes,
                            title: format!("DeviceSupport: {title}"),
                            notes: vec![
                                "影響: 古い iOS バージョンのデバッグで必要になる可能性があります。".to_string(),
                            ],
                        }
                    })
                    .collect()
            }
        }
        CleanupKind::CoreSimulatorUnavailable => {
            let base = home_dir.join("Library/Developer/CoreSimulator/Devices");
            if !base.exists() {
                Vec::new()
            } else {
                let cmd_timeout = std::cmp::min(timeout, Duration::from_secs(8));
                if cmd_timeout == Duration::from_secs(0) {
                    return Err(anyhow::anyhow!(
                        "タイムアウト予算が 0 のため、`xcrun simctl list devices unavailable` を実行できません。"
                    ));
                }
                let out = crate::platform::run_command_invoking_user(
                    "xcrun",
                    &["simctl", "list", "devices", "unavailable"],
                    cmd_timeout,
                )
                .context("xcrun simctl の実行")?;
                if out.exit_code != 0 {
                    return Err(anyhow::anyhow!(
                        "`xcrun simctl list devices unavailable` が失敗しました（exit_code={}）: {}",
                        out.exit_code,
                        out.stderr.trim()
                    ));
                }

                let devices = parse_simctl_unavailable_devices(&out.stdout);
                let mut candidates = Vec::<CleanupCandidate>::new();
                for (name, uuid) in devices {
                    let p = base.join(&uuid);
                    if !p.is_dir() {
                        continue;
                    }
                    let bytes = estimate_candidate_bytes(&p, per_item_budget, deadline);
                    let title = if name.is_empty() {
                        format!("Simulator: {uuid}")
                    } else {
                        format!("Simulator: {name} ({uuid})")
                    };
                    candidates.push(CleanupCandidate {
                        path: p,
                        bytes,
                        title,
                        notes: vec![
                            "影響: 利用できないシミュレータのデータをゴミ箱へ移動します。必要な場合は事前に確認してください。"
                                .to_string(),
                        ],
                    });
                }
                candidates
            }
        }
    };

    let mut candidates = candidates;
    candidates.sort_by(|a, b| {
        (
            std::cmp::Reverse(a.bytes),
            a.title.as_str(),
            a.path.as_path(),
        )
            .cmp(&(std::cmp::Reverse(b.bytes), b.title.as_str(), b.path.as_path()))
    });

    let prefix = match kind {
        CleanupKind::XcodeArchives => "cleanup-xcode-archives",
        CleanupKind::XcodeDeviceSupport => "cleanup-xcode-device-support",
        CleanupKind::CoreSimulatorUnavailable => "cleanup-coresimulator-unavailable",
    };

    let actions = candidates
        .into_iter()
        .enumerate()
        .map(|(i, c)| crate::core::ActionPlan {
            id: format!("{prefix}-{i}"),
            title: c.title,
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: c.bytes,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec![mask_home_path(&c.path, Some(home_dir))],
            },
            notes: c.notes,
        })
        .collect();

    Ok(actions)
}

fn estimate_candidate_bytes(path: &std::path::Path, per_item: Duration, deadline: Option<Instant>) -> u64 {
    crate::scan::estimate_dir_size(path, per_item, deadline)
        .map(|e| e.bytes)
        .unwrap_or(0)
}

fn path_ends_with(path: &std::path::Path, suffix: &str) -> bool {
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return false;
    };
    name.to_ascii_lowercase()
        .ends_with(&suffix.to_ascii_lowercase())
}

fn is_uuid_like(s: &str) -> bool {
    let s = s.trim();
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        let c = *b as char;
        let is_dash = c == '-';
        if matches!(i, 8 | 13 | 18 | 23) {
            if !is_dash {
                return false;
            }
            continue;
        }
        if is_dash {
            return false;
        }
        if !c.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn parse_simctl_unavailable_devices(stdout: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !line.contains("unavailable") {
            continue;
        }
        let Some((name_part, rest)) = line.split_once('(') else {
            continue;
        };
        let Some((uuid_part, _)) = rest.split_once(')') else {
            continue;
        };
        let uuid = uuid_part.trim();
        if !is_uuid_like(uuid) {
            continue;
        }
        out.push((name_part.trim().to_string(), uuid.to_string()));
    }
    out.sort();
    out.dedup();
    out
}

fn start_fix_apply(
    app: &mut App,
    max_risk: RiskLevel,
    actions: Vec<crate::core::ActionPlan>,
    return_to: Screen,
) {
    let home_dir = app.home_dir.clone();
    let (tx, rx) = mpsc::channel::<Result<FixApplyResult>>();
    thread::spawn(move || {
        let res = (|| -> Result<FixApplyResult> {
            let started_at = OffsetDateTime::now_utc();
            let outcome = crate::actions::apply_trash_moves(&actions, &home_dir)?;
            let finished_at = OffsetDateTime::now_utc();
            let log_path = crate::logs::write_fix_apply_log(
                &home_dir,
                started_at,
                finished_at,
                max_risk,
                &actions,
                &outcome,
            )
            .map_err(|e| {
                anyhow::anyhow!(
                    "fix apply: 変更を適用しましたが、トランザクションログの書き込みに失敗しました: {e}"
                )
            })?;

            Ok(FixApplyResult {
                max_risk,
                actions,
                outcome,
                log_path,
                return_to,
            })
        })();
        let _ = tx.send(res);
    });

    app.pending_apply = Some(PendingApply {
        rx,
        started_at: Instant::now(),
    });
    app.screen = Screen::Running;
    app.error = None;
}

fn start_fix_run_cmd(
    app: &mut App,
    timeout: Duration,
    actions: Vec<crate::core::ActionPlan>,
    return_to: Screen,
) {
    let home_dir = app.home_dir.clone();
    let (tx, rx) = mpsc::channel::<Result<FixRunCmdResult>>();
    thread::spawn(move || {
        let res = (|| -> Result<FixRunCmdResult> {
            let mut results = Vec::<FixRunCmdActionResult>::new();

            for action in actions {
                let started_at = OffsetDateTime::now_utc();
                let attempt = crate::actions::run_allowlisted_cmd(&action, timeout);
                let (output, attempt_error) = match attempt {
                    Ok(output) => (Some(output), None),
                    Err(err) => (None, Some(err.to_string())),
                };
                let finished_at = OffsetDateTime::now_utc();

                let log_path = crate::logs::write_fix_run_cmd_log(
                    &home_dir,
                    started_at,
                    finished_at,
                    &action,
                    output.as_ref(),
                    attempt_error.clone(),
                );

                let (log_path, log_error) = match log_path {
                    Ok(path) => (Some(path), None),
                    Err(err) => (None, Some(err.to_string())),
                };

                let exit_code = output.as_ref().map(|o| o.exit_code);
                let mut warning = None::<String>;
                let error = if let Some(err) = attempt_error {
                    Some(err)
                } else if let Some(out) = output.as_ref() {
                    match crate::actions::evaluate_allowlisted_run_cmd_output(&action, out) {
                        crate::actions::AllowlistedRunCmdOutcome::Ok => None,
                        crate::actions::AllowlistedRunCmdOutcome::OkWithWarnings(w) => {
                            warning = Some(w);
                            None
                        }
                        crate::actions::AllowlistedRunCmdOutcome::Error(e) => Some(e),
                    }
                } else {
                    Some("コマンド出力がありません。".to_string())
                };
                let repair_actions = output
                    .as_ref()
                    .map(|out| crate::actions::suggest_allowlisted_run_cmd_repair_actions(&action, out))
                    .unwrap_or_default();

                results.push(FixRunCmdActionResult {
                    action,
                    exit_code,
                    warning,
                    error,
                    repair_actions,
                    log_path,
                    log_error,
                });
            }

            Ok(FixRunCmdResult { results, return_to })
        })();
        let _ = tx.send(res);
    });

    app.pending_run_cmd = Some(PendingRunCmd {
        rx,
        started_at: Instant::now(),
    });
    app.screen = Screen::Running;
    app.error = None;
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let size = f.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(size);

    draw_header(f, chunks[0], app);
    draw_footer(f, chunks[2], app);

    match app.screen {
        Screen::Home => draw_home(f, chunks[1], app),
        Screen::Running => draw_running(f, chunks[1], app),
        Screen::ScanConfig => draw_scan_config(f, chunks[1], app),
        Screen::LogsList => draw_logs_list(f, chunks[1], app),
        Screen::LogsDetail => draw_logs_detail(f, chunks[1], app),
        Screen::ReportView => draw_report(f, chunks[1], app),
        Screen::Utilities => draw_utilities(f, chunks[1], app),
        Screen::CleanupView => draw_cleanup(f, chunks[1], app),
        Screen::FixView => draw_fix(f, chunks[1], app),
        Screen::FixConfirm => draw_fix_confirm(f, chunks[1], app),
        Screen::FixResult => draw_fix_result(f, chunks[1], app),
        Screen::FixRunCmdConfirm => draw_fix_run_cmd_confirm(f, chunks[1], app),
        Screen::FixRunCmdResult => draw_fix_run_cmd_result(f, chunks[1], app),
        Screen::Error => draw_error(f, chunks[1], app),
        Screen::Help => draw_help(f, chunks[1], app),
    }
}

fn draw_header(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let title = match app.screen {
        Screen::Home => "macdiet — ホーム",
        Screen::Running => "macdiet — 実行中",
        Screen::ScanConfig => "macdiet — スキャン設定",
        Screen::LogsList => "macdiet — ログ",
        Screen::LogsDetail => "macdiet — ログ（詳細）",
        Screen::ReportView => "macdiet — レポート",
        Screen::Utilities => "macdiet — ユーティリティ",
        Screen::CleanupView => "macdiet — 個別削除（ゴミ箱へ移動）",
        Screen::FixView => "macdiet — 掃除（dry-run）",
        Screen::FixConfirm => "macdiet — 適用（確認）",
        Screen::FixResult => "macdiet — 適用（結果）",
        Screen::FixRunCmdConfirm => "macdiet — RUN_CMD（確認）",
        Screen::FixRunCmdResult => "macdiet — RUN_CMD（結果）",
        Screen::Error => "macdiet — エラー",
        Screen::Help => "macdiet — ヘルプ",
    };
    let right = format!("v{}", env!("CARGO_PKG_VERSION"));

    let line = Line::from(vec![
        Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::styled(right, Style::default().fg(Color::DarkGray)),
    ]);

    let w = Paragraph::new(line).block(Block::default().borders(Borders::ALL));
    f.render_widget(w, area);
}

fn draw_footer(f: &mut ratatui::Frame, area: Rect, app: &App) {
    if app.filter_mode {
        let filter = truncate_chars(app.filter.trim(), 60);
        let line1 = Line::from(vec![
            Span::styled("フィルタ: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                if filter.is_empty() {
                    "（空）"
                } else {
                    filter.as_str()
                },
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let line2 = Line::from("Backspace 削除 | Ctrl-U クリア | Enter/Esc 終了 | Ctrl-C 強制終了");
        let w = Paragraph::new(Text::from(vec![line1, line2]))
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: true });
        f.render_widget(w, area);
        return;
    }

    let (line1, line2) = match app.screen {
        Screen::Home => {
            if app.query_mode {
                (
                    "Enter 実行 | ↑↓ 選択 | Backspace 削除 | : 検索切替 | Esc 検索終了",
                    "Ctrl-C 終了 | ? ヘルプ",
                )
            } else {
                (
                    "Enter 実行 | ↑↓/j/k 選択 | : 検索",
                    "q 終了 | ? ヘルプ | Ctrl-C 強制終了",
                )
            }
        }
        Screen::Running => {
            if app.pending_apply.is_some() || app.pending_run_cmd.is_some() {
                ("（実行中）", "q 終了 | Ctrl-C 強制終了 | ? ヘルプ")
            } else {
                (
                    "Esc 中止 | （実行中）",
                    "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
                )
            }
        }
        Screen::ScanConfig => {
            if app.scan_edit_mode {
                (
                    "文字 編集 | Backspace 削除 | Enter/Esc 編集完了",
                    "Ctrl-C 強制終了",
                )
            } else {
                (
                    "↑↓/j/k 選択 | Enter 編集 | r 実行 | b/Esc 戻る",
                    "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
                )
            }
        }
        Screen::LogsList => (
            "↑↓/j/k 選択 | Enter 開く | / フィルタ | r 更新 | b/Esc 戻る",
            "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
        Screen::LogsDetail => (
            "↑↓/j/k スクロール | r 再読込 | b/Esc 戻る",
            "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
        Screen::ReportView => (
            "Tab タブ | ↑↓/j/k 移動 | e 根拠 | / フィルタ | r 再実行 | b/Esc 戻る",
            "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
        Screen::Utilities => (
            "↑↓/j/k 選択 | 1/2/3 リスク | x/Enter 実行(RUN_CMD許可リスト) | / フィルタ | b/Esc 戻る",
            "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
        Screen::FixView => (
            "↑↓/j/k 移動 | Space 選択 | Enter おすすめ | a 全選択 | n 全解除 | 1/2/3 リスク | / フィルタ | r 更新",
            "p 適用（ゴミ箱へ移動/R1） | x 実行（許可リストRUN_CMD） | c 個別削除（対応項目） | b/Esc 戻る | q 終了 | ? ヘルプ",
        ),
        Screen::FixConfirm => (
            "Enter 送信 | Backspace 削除 | Esc キャンセル",
            "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
        Screen::FixResult => (
            "b/Esc 戻る（自動更新） | r 更新",
            "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
        Screen::FixRunCmdConfirm => (
            "Enter 送信 | Backspace 削除 | Esc キャンセル",
            "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
        Screen::FixRunCmdResult => {
            let auto_refresh = app
                .fix_run_cmd_result
                .as_ref()
                .is_some_and(|r| r.return_to == Screen::FixView);
            let has_repair = app
                .fix_run_cmd_result
                .as_ref()
                .is_some_and(|r| r.results.iter().any(|x| !x.repair_actions.is_empty()));
            let has_secondary_repair = app.fix_run_cmd_result.as_ref().is_some_and(|r| {
                r.results
                    .iter()
                    .any(|x| x.repair_actions.len() >= 2)
            });
            if has_secondary_repair {
                if auto_refresh {
                    (
                        "f 修復 | g 追加修復 | b/Esc 戻る（自動更新） | r 更新",
                        "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
                    )
                } else {
                    (
                        "f 修復 | g 追加修復 | b/Esc 戻る | r 更新",
                        "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
                    )
                }
            } else if has_repair {
                if auto_refresh {
                    (
                        "f 修復 | b/Esc 戻る（自動更新） | r 更新",
                        "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
                    )
                } else {
                    (
                        "f 修復 | b/Esc 戻る | r 更新",
                        "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
                    )
                }
            } else {
                if auto_refresh {
                    (
                        "b/Esc 戻る（自動更新） | r 更新",
                        "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
                    )
                } else {
                    ("b/Esc 戻る | r 更新", "q 終了 | Ctrl-C 強制終了 | ? ヘルプ")
                }
            }
        }
        Screen::Error => (
            "r 再試行 | b/Esc 戻る",
            "q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
        Screen::Help => ("Esc/? 閉じる", ""),
        Screen::CleanupView => (
            "↑↓/j/k 移動 | Space 選択 | a 全選択 | n 全解除 | 1/2/3 リスク | / フィルタ | r 更新 | b/Esc 戻る",
            "p 適用(R2/TRASH_MOVE) | q 終了 | Ctrl-C 強制終了 | ? ヘルプ",
        ),
    };
    let w = Paragraph::new(Text::from(vec![Line::from(line1), Line::from(line2)]))
        .style(Style::default().fg(Color::DarkGray))
        .wrap(Wrap { trim: true });
    f.render_widget(w, area);
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let mut s = String::new();
    for (i, ch) in input.chars().enumerate() {
        if i >= max_chars {
            s.push('…');
            break;
        }
        s.push(ch);
    }
    s
}

fn draw_home(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let indices = app.filtered_command_indices();
    app.ensure_command_selection_in_range();

    let items: Vec<ListItem> = if indices.is_empty() {
        vec![ListItem::new(Line::from("一致するコマンドがありません。"))]
    } else {
        indices
            .iter()
            .map(|idx| {
                let c = &app.commands[*idx];
                ListItem::new(Line::from(vec![
                    Span::styled(c.title, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" "),
                    Span::styled("—", Style::default().fg(Color::DarkGray)),
                    Span::raw(" "),
                    Span::styled(c.description, Style::default().fg(Color::DarkGray)),
                ]))
            })
            .collect()
    };

    let title = if app.query_mode {
        format!("コマンド（検索: {}）", app.query)
    } else if app.query.trim().is_empty() {
        "コマンド".to_string()
    } else {
        format!("コマンド（絞り込み: {}）", app.query)
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, chunks[0], &mut app.command_state);

    let detail = if let Some(kind) = app.selected_command_kind() {
        let (title, desc, note) = match kind {
            CommandKind::Doctor => (
                "診断（簡易）",
                "開発者向けに、System Data の主要原因を素早く分解して提示します。",
                "注: 診断は読み取り専用です。適用には明示的な確認が必要です。",
            ),
            CommandKind::ScanDeep => (
                "スキャン（深掘り）",
                "トップディレクトリを深掘りスキャンします（既定値: scope=設定 / max_depth=3 / top_dirs=20）。",
                "注: 時間がかかることがあります（ベストエフォート）。細かい制御は CLI の `macdiet scan --deep` を使用してください。",
            ),
            CommandKind::SnapshotsStatus => (
                "スナップショット状態",
                "Time Machine ローカルスナップショット + APFS スナップショットを表示します（ベストエフォート）。",
                "注: thin/delete（R3）は、UI からはまだ実行できません。",
            ),
            CommandKind::FixDryRun => (
                "掃除（dry-run）",
                "掃除アクションを閲覧し、プランを選択します（まだ変更は適用しません）。",
                "注: 適用は R1/TRASH_MOVE のみ。RUN_CMD は許可リストのみで、入力による確認が必要です。",
            ),
            CommandKind::Utilities => (
                "ユーティリティ",
                "doctor の所見に依存せず、許可リストの外部コマンド（brew/xcrun 等）を実行します。",
                "注: RUN_CMD は許可リストのみで、入力による確認が必要です。",
            ),
            CommandKind::Logs => (
                "ログ",
                "ローカル監査ログ（fix apply / snapshots thin/delete など）を閲覧します。",
                "注: ログは ~/.config/macdiet/logs/ に保存され、UI では閲覧のみです。",
            ),
        };
        Text::from(vec![
            Line::from(Span::styled(
                title,
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(desc),
            Line::from(""),
            Line::from(Span::styled(note, Style::default().fg(Color::DarkGray))),
        ])
    } else {
        Text::from("コマンドが選択されていません。")
    };
    let w = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title("詳細"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, chunks[1]);
}

fn draw_scan_config(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let sel = app.scan_config_state.selected().unwrap_or(0);
    let field_title = match sel {
        0 => "scope",
        1 => "max_depth",
        2 => "top_dirs",
        3 => "exclude",
        _ => "scope",
    };

    let edit_style = if app.scan_edit_mode {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let items = vec![
        ListItem::new(Line::from(vec![
            Span::styled("scope: ", Style::default().fg(Color::DarkGray)),
            Span::raw(app.scan_scope_input.clone()),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("max_depth: ", Style::default().fg(Color::DarkGray)),
            Span::raw(app.scan_max_depth_input.clone()),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("top_dirs: ", Style::default().fg(Color::DarkGray)),
            Span::raw(app.scan_top_dirs_input.clone()),
        ])),
        ListItem::new(Line::from(vec![
            Span::styled("exclude: ", Style::default().fg(Color::DarkGray)),
            Span::raw(app.scan_exclude_input.clone()),
        ])),
    ];

    let title = if app.scan_edit_mode {
        format!("スキャン設定（編集中: {field_title}）")
    } else {
        "スキャン設定".to_string()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, chunks[0], &mut app.scan_config_state);

    let mut detail_lines = Vec::<Line>::new();
    detail_lines.push(Line::from(Span::styled(
        "スキャン（深掘り）",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(vec![
        Span::styled("選択中の項目: ", Style::default().fg(Color::DarkGray)),
        Span::styled(field_title, edit_style),
    ]));
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled(
        "実行プレビュー:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    detail_lines.push(Line::from(format!(
        "$ macdiet scan --deep --scope {} --max-depth {} --top-dirs {} --exclude {}",
        app.scan_scope_input.trim(),
        app.scan_max_depth_input.trim(),
        app.scan_top_dirs_input.trim(),
        app.scan_exclude_input.trim(),
    )));
    detail_lines.push(Line::from(""));
    detail_lines.push(Line::from(Span::styled(
        "メモ:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    detail_lines.push(Line::from(
        "- exclude はカンマ区切りです（例: **/node_modules/**,**/.git/**）",
    ));
    detail_lines.push(Line::from(
        "- 深掘りスキャンは大きなツリーで遅くなることがあります（結果はベストエフォート）。",
    ));

    let w = Paragraph::new(Text::from(detail_lines))
        .block(Block::default().borders(Borders::ALL).title("詳細"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, chunks[1]);
}

fn draw_logs_list(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let indices = logs_filtered_indices(&app.logs_entries, &app.filter);
    App::move_list_selection(&mut app.logs_state, indices.len(), 0);

    let items: Vec<ListItem> = if app.logs_entries.is_empty() {
        vec![ListItem::new(Line::from("ログが見つかりません。"))]
    } else if indices.is_empty() {
        vec![
            ListItem::new(Line::from("一致するログがありません。")),
            ListItem::new(Line::from(Span::styled(
                "ヒント: '/' でフィルタを編集できます。",
                Style::default().fg(Color::DarkGray),
            ))),
        ]
    } else {
        indices
            .iter()
            .filter_map(|idx| app.logs_entries.get(*idx))
            .map(|entry| {
                let kind = log_kind_label(&entry.file_name);
                ListItem::new(Line::from(vec![
                    Span::styled(format!("[{kind}]"), Style::default().fg(Color::DarkGray)),
                    Span::raw(" "),
                    Span::styled(
                        entry.file_name.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("({})", crate::ui::format_bytes(entry.size)),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect()
    };

    let title = if app.filter.trim().is_empty() {
        format!("ログ ({})", app.logs_entries.len())
    } else {
        format!(
            "ログ ({shown}/{total})",
            shown = indices.len(),
            total = app.logs_entries.len()
        )
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, chunks[0], &mut app.logs_state);

    let logs_dir_hint = crate::logs::logs_dir(&app.home_dir)
        .strip_prefix(&app.home_dir)
        .ok()
        .map(|p| format!("~/{p}", p = p.display()))
        .unwrap_or_else(|| "~/.config/macdiet/logs".to_string());

    let detail = if app.logs_entries.is_empty() {
        Text::from(vec![
            Line::from(Span::styled(
                "利用可能なログがありません。",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(format!("ログ保存先: {logs_dir_hint}")),
            Line::from(""),
            Line::from("ログは次の実行時に書き込まれます:"),
            Line::from("- `macdiet fix --apply`（トランザクションログ）"),
            Line::from("- `macdiet ui`（掃除画面）での許可リスト RUN_CMD 実行"),
            Line::from("- `macdiet snapshots thin/delete`（コマンド試行ログ）"),
        ])
    } else if indices.is_empty() {
        Text::from(vec![
            Line::from(Span::styled(
                "一致するログがありません。",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("ヒント: '/' でフィルタ編集、Ctrl-U でクリアできます。"),
            Line::from(format!("ログ保存先: {logs_dir_hint}")),
        ])
    } else if let Some(view) = app.logs_view.as_ref() {
        let mut lines = Vec::<Line>::new();
        lines.push(Line::from(vec![
            Span::styled("ファイル: ", Style::default().fg(Color::DarkGray)),
            Span::raw(view.entry.file_name.clone()),
        ]));
        lines.push(Line::from(vec![
            Span::styled("サイズ: ", Style::default().fg(Color::DarkGray)),
            Span::raw(crate::ui::format_bytes(view.entry.size)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("パス: ", Style::default().fg(Color::DarkGray)),
            Span::raw(mask_home_path(&view.entry.path, Some(&app.home_dir))),
        ]));
        if view.truncated {
            lines.push(Line::from(Span::styled(
                "注: UI のプレビューではログ内容が省略されています",
                Style::default().fg(Color::Yellow),
            )));
        }
        if let Some(err) = view.parse_error.as_deref() {
            lines.push(Line::from(Span::styled(
                format!("解析エラー: {err}"),
                Style::default().fg(Color::Red),
            )));
        }
        lines.push(Line::from(""));
        for s in &view.summary {
            lines.push(Line::from(s.as_str()));
        }
        Text::from(lines)
    } else {
        Text::from(vec![
            Line::from("ログを選択するとプレビューします。"),
            Line::from(format!("ログ保存先: {logs_dir_hint}")),
        ])
    };

    let w = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title("詳細"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, chunks[1]);
}

fn draw_logs_detail(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(view) = app.logs_view.as_ref() else {
        let w = Paragraph::new("ログが読み込まれていません。")
            .block(Block::default().borders(Borders::ALL).title("ログ"));
        f.render_widget(w, area);
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(1)])
        .split(area);

    let mut summary_lines = Vec::<Line>::new();
    summary_lines.push(Line::from(vec![
        Span::styled("ファイル: ", Style::default().fg(Color::DarkGray)),
        Span::raw(view.entry.file_name.clone()),
    ]));
    summary_lines.push(Line::from(vec![
        Span::styled("サイズ: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(view.entry.size)),
        Span::raw("  "),
        Span::styled("パス: ", Style::default().fg(Color::DarkGray)),
        Span::raw(mask_home_path(&view.entry.path, Some(&app.home_dir))),
    ]));
    if view.truncated {
        summary_lines.push(Line::from(Span::styled(
            "注: UI ではログ内容が省略されています（ログが非常に大きい）",
            Style::default().fg(Color::Yellow),
        )));
    }
    if let Some(err) = view.parse_error.as_deref() {
        summary_lines.push(Line::from(Span::styled(
            format!("解析エラー: {err}"),
            Style::default().fg(Color::Red),
        )));
    }
    for s in &view.summary {
        summary_lines.push(Line::from(s.as_str()));
    }

    let summary = Paragraph::new(Text::from(summary_lines))
        .block(Block::default().borders(Borders::ALL).title("概要"))
        .wrap(Wrap { trim: false });
    f.render_widget(summary, chunks[0]);

    let content = Paragraph::new(view.content.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("ログ（JSON 生データ）"),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.logs_scroll, 0));
    f.render_widget(content, chunks[1]);
}

fn draw_running(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let spinner = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let idx = (app.tick as usize) % spinner.len();
    let s = spinner[idx];
    let msg = if app.pending_apply.is_some() {
        "適用中（TRASH_MOVE）..."
    } else if app.pending_run_cmd.is_some() {
        "実行中（許可リスト RUN_CMD）..."
    } else if app.pending_cleanup.is_some() {
        "個別削除候補を収集中..."
    } else {
        match app.pending.as_ref().map(|p| p.kind) {
            Some(CommandKind::Doctor) => "診断を実行中...",
            Some(CommandKind::ScanDeep) => "スキャン（深掘り）を実行中...",
            Some(CommandKind::SnapshotsStatus) => "スナップショット状態を取得中...",
            Some(CommandKind::FixDryRun) => "掃除プラン（dry-run）を作成中...",
            Some(CommandKind::Utilities) => "ユーティリティを準備中...",
            Some(CommandKind::Logs) => "ログを読み込み中...",
            None => "処理中...",
        }
    };

    let w = Paragraph::new(Line::from(vec![
        Span::styled(s, Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
        Span::raw(msg),
    ]))
    .block(Block::default().borders(Borders::ALL));
    f.render_widget(w, centered_rect(60, 20, area));
}

fn draw_report(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(report) = app.report.as_ref() else {
        return;
    };
    let tab = app.tab;
    let color = app.color;
    let show_evidence = app.show_evidence;
    let filter = app.filter.as_str();
    let findings_state = &mut app.findings_state;
    let actions_state = &mut app.actions_state;
    let notes_scroll = app.notes_scroll;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let tab_titles = ["所見", "アクション", "注記"];
    let selected = app.tab as usize;
    let tabs = Tabs::new(tab_titles)
        .select(selected)
        .block(Block::default().borders(Borders::ALL).title("レポート"))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    f.render_widget(tabs, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    match tab {
        Tab::Findings => draw_findings(
            f,
            body[0],
            body[1],
            report,
            filter,
            findings_state,
            color,
            show_evidence,
        ),
        Tab::Actions => draw_actions(f, body[0], body[1], report, filter, actions_state, color),
        Tab::Notes => draw_notes(f, chunks[1], report, filter, notes_scroll),
    }
}

fn cleanup_kind_title(kind: CleanupKind) -> &'static str {
    match kind {
        CleanupKind::XcodeArchives => "Xcode Archives（.xcarchive）",
        CleanupKind::XcodeDeviceSupport => "Xcode iOS DeviceSupport",
        CleanupKind::CoreSimulatorUnavailable => "CoreSimulator（unavailable のみ）",
    }
}

fn cleanup_kind_base_path(kind: CleanupKind) -> &'static str {
    match kind {
        CleanupKind::XcodeArchives => "~/Library/Developer/Xcode/Archives",
        CleanupKind::XcodeDeviceSupport => "~/Library/Developer/Xcode/iOS DeviceSupport",
        CleanupKind::CoreSimulatorUnavailable => "~/Library/Developer/CoreSimulator/Devices",
    }
}

fn draw_cleanup(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let all_candidates = cleanup_candidate_indices(app);
    let candidates = cleanup_filtered_indices(app);
    App::move_list_selection(&mut app.cleanup_state, candidates.len(), 0);

    let mut selected_count = 0usize;
    let mut selected_estimated = 0u64;
    for idx in &all_candidates {
        let Some(action) = app.cleanup_actions.get(*idx) else {
            continue;
        };
        if !app.cleanup_selected.contains(&action.id) {
            continue;
        }
        selected_count += 1;
        selected_estimated = selected_estimated.saturating_add(action.estimated_reclaimed_bytes);
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let apply_hint = if app.dry_run {
        Span::styled(
            "(dry-run: 破壊的操作は無効です)",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else if app.fix_max_risk < RiskLevel::R2 {
        Span::styled(
            "(最大リスクが R2 未満のため p は無効です: 2 を押してください)",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "(p: 適用(R2/TRASH_MOVE))",
            Style::default().fg(Color::DarkGray),
        )
    };

    let candidate_label = if app.filter.trim().is_empty() {
        candidates.len().to_string()
    } else {
        format!("{}/{}", candidates.len(), all_candidates.len())
    };
    let summary = Line::from(vec![
        Span::styled("対象: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            cleanup_kind_title(app.cleanup_kind),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("ベース: ", Style::default().fg(Color::DarkGray)),
        Span::raw(cleanup_kind_base_path(app.cleanup_kind)),
        Span::raw("  "),
        Span::styled("最大リスク: ", Style::default().fg(Color::DarkGray)),
        Span::raw(app.fix_max_risk.to_string()),
        Span::raw("  "),
        Span::styled("候補: ", Style::default().fg(Color::DarkGray)),
        Span::raw(candidate_label),
        Span::raw("  "),
        Span::styled("選択: ", Style::default().fg(Color::DarkGray)),
        Span::raw(selected_count.to_string()),
        Span::raw("  "),
        Span::styled("推定: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(selected_estimated)),
        Span::raw("  "),
        apply_hint,
    ]);
    let w =
        Paragraph::new(summary).block(Block::default().borders(Borders::ALL).title("個別削除"));
    f.render_widget(w, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    let items: Vec<ListItem> = if candidates.is_empty() {
        if app.fix_max_risk < RiskLevel::R2 {
            vec![
                ListItem::new(Line::from("最大リスクが R2 未満のため候補を表示できません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: 2 を押して R2 を含めてください。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        } else if app.filter.trim().is_empty() {
            vec![
                ListItem::new(Line::from("候補がありません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: r で再取得できます。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        } else {
            vec![
                ListItem::new(Line::from("一致する候補がありません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: '/' でフィルタを編集できます。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        }
    } else {
        candidates
            .iter()
            .filter_map(|idx| app.cleanup_actions.get(*idx))
            .map(|action| {
                let checked = app.cleanup_selected.contains(&action.id);
                let checkbox = if checked { "[x]" } else { "[ ]" };
                let checkbox_style = if checked {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let risk_style = risk_style(action.risk_level, app.color);
                ListItem::new(Line::from(vec![
                    Span::styled(checkbox, checkbox_style),
                    Span::raw(" "),
                    Span::styled(
                        format!(
                            "{:>10}",
                            crate::ui::format_bytes(action.estimated_reclaimed_bytes)
                        ),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(" "),
                    Span::styled(action.risk_level.to_string(), risk_style),
                    Span::raw(" "),
                    Span::styled(
                        action.title.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                ]))
            })
            .collect()
    };

    let title = if app.filter.trim().is_empty() {
        format!("候補（{}）", all_candidates.len())
    } else {
        format!("候補（{shown}/{total}）", shown = candidates.len(), total = all_candidates.len())
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, body[0], &mut app.cleanup_state);

    let detail = if let Some(sel) = app.cleanup_state.selected() {
        candidates
            .get(sel)
            .and_then(|idx| app.cleanup_actions.get(*idx))
            .map(cleanup_action_detail)
    } else {
        None
    }
    .unwrap_or_else(|| Text::from("項目が選択されていません。"));

    let w = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title("詳細"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, body[1]);
}

fn draw_fix(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(report) = app.report.as_ref() else {
        let w =
            Paragraph::new("ホームで「掃除（dry-run）」を実行すると、アクションを確認できます。")
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("掃除（dry-run）"),
                );
        f.render_widget(w, area);
        return;
    };

    let all_candidates = fix_candidate_indices(report, app.fix_max_risk);
    let candidates = fix_filtered_candidate_indices(report, app.fix_max_risk, &app.filter);
    App::move_list_selection(&mut app.fix_state, candidates.len(), 0);

    let mut selected_count = 0usize;
    let mut selected_estimated = 0u64;
    let mut selected_trash_estimated = 0u64;
    for idx in &all_candidates {
        let Some(action) = report.actions.get(*idx) else {
            continue;
        };
        if !app.fix_selected.contains(&action.id) {
            continue;
        }
        selected_count += 1;
        selected_estimated = selected_estimated.saturating_add(action.estimated_reclaimed_bytes);
        if matches!(action.kind, ActionKind::TrashMove { .. }) {
            selected_trash_estimated =
                selected_trash_estimated.saturating_add(action.estimated_reclaimed_bytes);
        }
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(1)])
        .split(area);

    let candidate_label = if app.filter.trim().is_empty() {
        candidates.len().to_string()
    } else {
        format!("{}/{}", candidates.len(), all_candidates.len())
    };
    let summary = Line::from(vec![
        Span::styled("最大リスク: ", Style::default().fg(Color::DarkGray)),
        Span::raw(app.fix_max_risk.to_string()),
        Span::raw("  "),
        Span::styled("候補: ", Style::default().fg(Color::DarkGray)),
        Span::raw(candidate_label),
        Span::raw("  "),
        Span::styled("選択: ", Style::default().fg(Color::DarkGray)),
        Span::raw(selected_count.to_string()),
        Span::raw("  "),
        Span::styled("推定合計: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(selected_estimated)),
        Span::raw("  "),
        Span::styled("ゴミ箱へ移動: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(selected_trash_estimated)),
    ]);

    let hint_line = if app.dry_run {
        Line::from(Span::styled(
            "dry-run: 破壊的操作は無効です",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from(vec![
            Span::styled("操作: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "Enter=おすすめ(現在行)  c=個別削除  p=ゴミ箱(選択)  x=コマンド(選択)",
                Style::default().fg(Color::DarkGray),
            ),
        ])
    };

    let w = Paragraph::new(Text::from(vec![summary, hint_line]))
        .block(Block::default().borders(Borders::ALL).title("プラン"));
    f.render_widget(w, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    let items: Vec<ListItem> = if candidates.is_empty() {
        if app.filter.trim().is_empty() {
            vec![
                ListItem::new(Line::from("実行可能なアクションがありません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: 2/3 を押すと R2/R3 のプレビューアクションを含めます。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        } else {
            vec![
                ListItem::new(Line::from("一致するアクションがありません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: '/' でフィルタを編集できます。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        }
    } else {
        candidates
            .iter()
            .filter_map(|idx| report.actions.get(*idx))
            .map(|action| {
                let checked = app.fix_selected.contains(&action.id);
                let checkbox = if checked { "[x]" } else { "[ ]" };
                let checkbox_style = if checked {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let risk_style = risk_style(action.risk_level, app.color);
                ListItem::new(Line::from(vec![
                    Span::styled(checkbox, checkbox_style),
                    Span::raw(" "),
                    Span::styled(
                        format!(
                            "{:>10}",
                            crate::ui::format_bytes(action.estimated_reclaimed_bytes)
                        ),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(" "),
                    Span::styled(action.risk_level.to_string(), risk_style),
                    Span::raw(" "),
                    Span::styled(
                        action.title.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("id={}", action.id),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect()
    };

    let title = format!("候補（最大リスク={}）", app.fix_max_risk);
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, body[0], &mut app.fix_state);

    let detail = if let Some(sel) = app.fix_state.selected() {
        candidates
            .get(sel)
            .and_then(|idx| report.actions.get(*idx))
            .map(|action| action_detail(action, report))
    } else {
        None
    }
    .unwrap_or_else(|| Text::from("アクションが選択されていません。"));

    let w = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title("詳細"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, body[1]);
}

fn draw_fix_confirm(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(confirm) = app.fix_confirm.as_ref() else {
        let w = Paragraph::new("確認状態がありません。")
            .block(Block::default().borders(Borders::ALL).title("適用"));
        f.render_widget(w, area);
        return;
    };

    let estimated_total: u64 = confirm
        .actions
        .iter()
        .map(|a| a.estimated_reclaimed_bytes)
        .sum();

    let prompt = match confirm.stage {
        FixConfirmStage::Yes => "続行するには 'yes' と入力してください: ",
        FixConfirmStage::Trash => "最終確認: 適用するには 'trash' と入力してください: ",
    };

    let mut header_lines = Vec::<Line>::new();
    header_lines.push(Line::from(vec![
        Span::styled(
            format!("適用（TRASH_MOVE, 最大リスク={}）", confirm.max_risk),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "（~/.Trash から復元可能）",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    header_lines.push(Line::from(vec![
        Span::styled("選択: ", Style::default().fg(Color::DarkGray)),
        Span::raw(confirm.selected_total.to_string()),
        Span::raw("  "),
        Span::styled("対象外: ", Style::default().fg(Color::DarkGray)),
        Span::raw(confirm.ignored_total.to_string()),
        Span::raw("  "),
        Span::styled("適用予定: ", Style::default().fg(Color::DarkGray)),
        Span::raw(confirm.actions.len().to_string()),
        Span::raw("  "),
        Span::styled("推定: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(estimated_total)),
    ]));
    if confirm.ignored_total > 0 {
        header_lines.push(Line::from(Span::styled(
            "注: 選択したうち、この操作で適用されるのは TRASH_MOVE のみです（RUN_CMD は別途実行）。",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if app.dry_run {
        header_lines.push(Line::from(Span::styled(
            "dry-run モードです。本来この画面には到達しないはずです。",
            Style::default().fg(Color::Red),
        )));
    }
    if let Some(err) = confirm.error.as_deref() {
        header_lines.push(Line::from(Span::styled(
            err,
            Style::default().fg(Color::Red),
        )));
    }
    header_lines.push(Line::from(""));
    header_lines.push(Line::from(vec![
        Span::styled(prompt, Style::default().fg(Color::DarkGray)),
        Span::styled(
            confirm.input.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(1)])
        .split(area);

    let header = Paragraph::new(Text::from(header_lines))
        .block(Block::default().borders(Borders::ALL).title("確認"))
        .wrap(Wrap { trim: false });
    f.render_widget(header, chunks[0]);

    let mut action_lines = Vec::<Line>::new();
    if confirm.actions.is_empty() {
        action_lines.push(Line::from("（アクションなし）"));
    } else {
        for action in confirm.actions.iter().take(18) {
            let path_count = match &action.kind {
                ActionKind::TrashMove { paths } => paths.len(),
                _ => 0,
            };
            action_lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{:>10}",
                        crate::ui::format_bytes(action.estimated_reclaimed_bytes)
                    ),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" "),
                Span::styled(
                    action.title.clone(),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("（{path_count}件）"),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("id={}", action.id),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        if confirm.actions.len() > 18 {
            action_lines.push(Line::from(Span::styled(
                "…（省略）",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let list = Paragraph::new(Text::from(action_lines))
        .block(Block::default().borders(Borders::ALL).title("適用予定"))
        .wrap(Wrap { trim: false });
    f.render_widget(list, chunks[1]);
}

fn draw_fix_run_cmd_confirm(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(confirm) = app.fix_run_cmd_confirm.as_ref() else {
        let w = Paragraph::new("確認状態がありません。")
            .block(Block::default().borders(Borders::ALL).title("RUN_CMD"));
        f.render_widget(w, area);
        return;
    };

    let estimated_total: u64 = confirm
        .actions
        .iter()
        .map(|a| a.estimated_reclaimed_bytes)
        .sum();

    let prompt = match confirm.stage {
        RunCmdConfirmStage::Token => {
            format!(
                "続行するには '{}' と入力してください: ",
                confirm.confirm_token
            )
        }
        RunCmdConfirmStage::Run => format!(
            "最終確認: 実行するには '{}' と入力してください: ",
            confirm.final_confirm_token
        ),
    };

    let mut header_lines = Vec::<Line>::new();
    header_lines.push(Line::from(vec![
        Span::styled(
            "RUN_CMD 実行（許可リストのみ）",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            "（影響が出る可能性があります）",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    header_lines.push(Line::from(vec![
        Span::styled("選択: ", Style::default().fg(Color::DarkGray)),
        Span::raw(confirm.selected_total.to_string()),
        Span::raw("  "),
        Span::styled("対象外: ", Style::default().fg(Color::DarkGray)),
        Span::raw(confirm.ignored_total.to_string()),
        Span::raw("  "),
        Span::styled("実行予定: ", Style::default().fg(Color::DarkGray)),
        Span::raw(confirm.actions.len().to_string()),
        Span::raw("  "),
        Span::styled("推定: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(estimated_total)),
    ]));
    if confirm.ignored_total > 0 {
        header_lines.push(Line::from(Span::styled(
            "注: 対象外アクションはプレビューのみです（許可リスト外）。",
            Style::default().fg(Color::DarkGray),
        )));
    }
    if app.dry_run {
        header_lines.push(Line::from(Span::styled(
            "dry-run モードです。本来この画面には到達しないはずです。",
            Style::default().fg(Color::Red),
        )));
    }
    if let Some(err) = confirm.error.as_deref() {
        header_lines.push(Line::from(Span::styled(
            err,
            Style::default().fg(Color::Red),
        )));
    }
    header_lines.push(Line::from(""));
    header_lines.push(Line::from(vec![
        Span::styled(prompt, Style::default().fg(Color::DarkGray)),
        Span::styled(
            confirm.input.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(9), Constraint::Min(1)])
        .split(area);

    let header = Paragraph::new(Text::from(header_lines))
        .block(Block::default().borders(Borders::ALL).title("確認"))
        .wrap(Wrap { trim: false });
    f.render_widget(header, chunks[0]);

    let mut action_lines = Vec::<Line>::new();
    if confirm.actions.is_empty() {
        action_lines.push(Line::from("（アクションなし）"));
    } else {
        for action in &confirm.actions {
            action_lines.push(Line::from(Span::styled(
                action.title.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            action_lines.push(Line::from(vec![
                Span::styled("id: ", Style::default().fg(Color::DarkGray)),
                Span::raw(action.id.clone()),
            ]));
            if let ActionKind::RunCmd { cmd, args } = &action.kind {
                action_lines.push(Line::from(format!("$ {cmd} {}", args.join(" "))));
            }
            if !action.notes.is_empty() {
                action_lines.push(Line::from(Span::styled(
                    "影響:",
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                for note in action.notes.iter().take(8) {
                    action_lines.push(Line::from(format!("- {note}")));
                }
                if action.notes.len() > 8 {
                    action_lines.push(Line::from(Span::styled(
                        "…（省略）",
                        Style::default().fg(Color::DarkGray),
                    )));
                }
            }
            action_lines.push(Line::from(""));
        }
    }

    let list = Paragraph::new(Text::from(action_lines))
        .block(Block::default().borders(Borders::ALL).title("実行予定"))
        .wrap(Wrap { trim: false });
    f.render_widget(list, chunks[1]);
}

fn draw_fix_result(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(result) = app.fix_apply_result.as_ref() else {
        let w = Paragraph::new("結果がありません。")
            .block(Block::default().borders(Borders::ALL).title("適用"));
        f.render_widget(w, area);
        return;
    };

    let log_hint = result
        .log_path
        .strip_prefix(&app.home_dir)
        .ok()
        .map(|p| format!("~/{p}", p = p.display()))
        .unwrap_or_else(|| result.log_path.display().to_string());

    let mut lines = Vec::<Line>::new();
    lines.push(Line::from(vec![
        Span::styled(
            "適用が完了しました",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("（アクション={}）", result.actions.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("最大リスク: ", Style::default().fg(Color::DarkGray)),
        Span::raw(result.max_risk.to_string()),
    ]));

    let moved = result.outcome.moved.len();
    let skipped = result.outcome.skipped_missing.len();
    let errors = result.outcome.errors.len();

    lines.push(Line::from(vec![
        Span::styled("移動: ", Style::default().fg(Color::DarkGray)),
        Span::raw(moved.to_string()),
        Span::raw("  "),
        Span::styled("スキップ(不存在): ", Style::default().fg(Color::DarkGray)),
        Span::raw(skipped.to_string()),
        Span::raw("  "),
        Span::styled("エラー: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            errors.to_string(),
            if errors == 0 {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            },
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled("ログ: ", Style::default().fg(Color::DarkGray)),
        Span::raw(log_hint),
    ]));

    if errors > 0 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "一部のパスをゴミ箱へ移動できませんでした。詳細はログを確認してください。",
            Style::default().fg(Color::Red),
        )));
        for e in result.outcome.errors.iter().take(6) {
            lines.push(Line::from(format!("- {}: {}", e.path.display(), e.error)));
        }
        if result.outcome.errors.len() > 6 {
            lines.push(Line::from(Span::styled(
                "…（省略）",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let w = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("結果"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, area);
}

fn draw_utilities(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let all_candidates = utilities_candidate_indices(app);
    let candidates = utilities_filtered_indices(app);
    App::move_list_selection(&mut app.utilities_state, candidates.len(), 0);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let hint = if app.dry_run {
        Span::styled(
            "(dry-run: RUN_CMD は無効です)",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "(x/Enter: 実行)",
            Style::default().fg(Color::DarkGray),
        )
    };

    let candidate_label = if app.filter.trim().is_empty() {
        candidates.len().to_string()
    } else {
        format!("{}/{}", candidates.len(), all_candidates.len())
    };
    let summary = Line::from(vec![
        Span::styled("最大リスク: ", Style::default().fg(Color::DarkGray)),
        Span::raw(app.fix_max_risk.to_string()),
        Span::raw("  "),
        Span::styled("候補: ", Style::default().fg(Color::DarkGray)),
        Span::raw(candidate_label),
        Span::raw("  "),
        hint,
    ]);
    let w = Paragraph::new(summary)
        .block(Block::default().borders(Borders::ALL).title("ユーティリティ"));
    f.render_widget(w, chunks[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(chunks[1]);

    let items: Vec<ListItem> = if candidates.is_empty() {
        if app.filter.trim().is_empty() {
            vec![
                ListItem::new(Line::from("実行可能なユーティリティがありません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: 2/3 を押すと R2/R3 を含めます。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        } else {
            vec![
                ListItem::new(Line::from("一致するユーティリティがありません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: '/' でフィルタを編集できます。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        }
    } else {
        candidates
            .iter()
            .filter_map(|idx| app.utilities_actions.get(*idx))
            .map(|action| {
                let risk_style = risk_style(action.risk_level, app.color);
                ListItem::new(Line::from(vec![
                    Span::styled(action.risk_level.to_string(), risk_style),
                    Span::raw(" "),
                    Span::styled(
                        action.title.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("id={}", action.id),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect()
    };

    let title = format!("一覧（最大リスク={}）", app.fix_max_risk);
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, body[0], &mut app.utilities_state);

    let fallback_report = Report {
        schema_version: "1.0".to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        os: crate::core::OsInfo {
            name: "unknown".to_string(),
            version: "unknown".to_string(),
        },
        generated_at: "unknown".to_string(),
        summary: crate::core::ReportSummary {
            estimated_total_bytes: 0,
            unobserved_bytes: 0,
            notes: vec![],
        },
        findings: vec![],
        actions: vec![],
    };
    let report = app.report.as_ref().unwrap_or(&fallback_report);

    let detail = if let Some(sel) = app.utilities_state.selected() {
        candidates
            .get(sel)
            .and_then(|idx| app.utilities_actions.get(*idx))
            .map(|action| action_detail(action, report))
    } else {
        None
    }
    .unwrap_or_else(|| Text::from("ユーティリティが選択されていません。"));

    let w = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title("詳細"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, body[1]);
}

fn draw_fix_run_cmd_result(f: &mut ratatui::Frame, area: Rect, app: &mut App) {
    let Some(result) = app.fix_run_cmd_result.as_ref() else {
        let w = Paragraph::new("結果がありません。")
            .block(Block::default().borders(Borders::ALL).title("RUN_CMD"));
        f.render_widget(w, area);
        return;
    };

    let mut lines = Vec::<Line>::new();
    lines.push(Line::from(vec![
        Span::styled(
            "RUN_CMD が完了しました",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            format!("（アクション={}）", result.results.len()),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    for r in &result.results {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            r.action.title.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(vec![
            Span::styled("id: ", Style::default().fg(Color::DarkGray)),
            Span::raw(r.action.id.clone()),
            Span::raw("  "),
            Span::styled("exit_code: ", Style::default().fg(Color::DarkGray)),
            Span::raw(
                r.exit_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "なし".to_string()),
            ),
        ]));
        if let ActionKind::RunCmd { cmd, args } = &r.action.kind {
            lines.push(Line::from(format!("$ {cmd} {}", args.join(" "))));
        }

        if let Some(path) = r.log_path.as_ref() {
            let hint = path
                .strip_prefix(&app.home_dir)
                .ok()
                .map(|p| format!("~/{p}", p = p.display()))
                .unwrap_or_else(|| path.display().to_string());
            lines.push(Line::from(vec![
                Span::styled("ログ: ", Style::default().fg(Color::DarkGray)),
                Span::raw(hint),
            ]));
        } else if let Some(err) = r.log_error.as_deref() {
            lines.push(Line::from(Span::styled(
                format!("ログエラー: {err}"),
                Style::default().fg(Color::Red),
            )));
        }

        if let Some(err) = r.error.as_deref() {
            lines.push(Line::from(Span::styled(
                format!("エラー: {err}"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));
            if !r.repair_actions.is_empty() {
                for (i, a) in r.repair_actions.iter().enumerate() {
                    let key = match i {
                        0 => 'f',
                        1 => 'g',
                        _ => continue,
                    };
                    let needs_risk_bump = a.risk_level > app.fix_max_risk;
                    let msg = if needs_risk_bump {
                        let key_hint = match a.risk_level {
                            RiskLevel::R0 => "0",
                            RiskLevel::R1 => "1",
                            RiskLevel::R2 => "2",
                            RiskLevel::R3 => "3",
                        };
                        format!(
                            "提案: {key} で「{title}」を実行できます（{risk}: Fix画面で {key_hint} を押して解禁）。",
                            title = a.title,
                            risk = a.risk_level
                        )
                    } else {
                        format!("提案: {key} で「{title}」を実行できます。", title = a.title)
                    };
                    lines.push(Line::from(Span::styled(
                        msg,
                        Style::default().fg(Color::Yellow),
                    )));
                }
            }
        } else if let Some(w) = r.warning.as_deref() {
            lines.push(Line::from(Span::styled(
                format!("警告: {w}"),
                Style::default().fg(Color::Yellow),
            )));
            lines.push(Line::from(Span::styled(
                "状態: OK（警告あり）",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "状態: OK",
                Style::default().fg(Color::Green),
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "ヒント: stdout/stderr を確認するには「ログ」を開いてください。",
        Style::default().fg(Color::DarkGray),
    )));

    let w = Paragraph::new(Text::from(lines))
        .block(Block::default().borders(Borders::ALL).title("結果"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, area);
}

fn draw_findings(
    f: &mut ratatui::Frame,
    left: Rect,
    right: Rect,
    report: &Report,
    filter: &str,
    state: &mut ListState,
    color: bool,
    show_evidence: bool,
) {
    let indices = report_filtered_finding_indices(report, filter);
    App::move_list_selection(state, indices.len(), 0);

    let items: Vec<ListItem> = if indices.is_empty() {
        if filter.trim().is_empty() {
            vec![ListItem::new(Line::from("所見がありません。"))]
        } else {
            vec![
                ListItem::new(Line::from("一致する所見がありません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: '/' でフィルタを編集できます。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        }
    } else {
        indices
            .iter()
            .filter_map(|idx| report.findings.get(*idx))
            .map(|finding| {
                let risk_style = risk_style(finding.risk_level, color);
                let line = Line::from(vec![
                    Span::styled(
                        format!("{:>10}", crate::ui::format_bytes(finding.estimated_bytes)),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(" "),
                    Span::styled(finding.risk_level.to_string(), risk_style),
                    Span::raw(" "),
                    Span::raw(finding.title.clone()),
                ]);
                ListItem::new(line)
            })
            .collect()
    };

    let title = if filter.trim().is_empty() {
        format!("所見（{}）", report.findings.len())
    } else {
        format!(
            "所見（{shown}/{total}）",
            shown = indices.len(),
            total = report.findings.len()
        )
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, left, state);

    let detail = if let Some(sel) = state.selected() {
        indices
            .get(sel)
            .and_then(|idx| report.findings.get(*idx))
            .map(|finding| finding_detail(finding, report, show_evidence))
    } else {
        None
    }
    .unwrap_or_else(|| Text::from("所見が選択されていません。"));

    let w = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title("詳細"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, right);
}

fn finding_detail(
    finding: &crate::core::Finding,
    report: &Report,
    show_evidence: bool,
) -> Text<'static> {
    let mut lines = Vec::<Line>::new();
    lines.push(Line::from(vec![
        Span::styled("id: ", Style::default().fg(Color::DarkGray)),
        Span::raw(finding.id.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("種類: ", Style::default().fg(Color::DarkGray)),
        Span::raw(finding.finding_type.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("リスク: ", Style::default().fg(Color::DarkGray)),
        Span::raw(finding.risk_level.to_string()),
        Span::raw("  "),
        Span::styled("確度: ", Style::default().fg(Color::DarkGray)),
        Span::raw(format!("{:.2}", finding.confidence)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("推定: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(finding.estimated_bytes)),
    ]));

    if !finding.recommended_actions.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "推奨アクション:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for a in &finding.recommended_actions {
            let title = report
                .actions
                .iter()
                .find(|x| x.id == a.id)
                .map(|x| x.title.clone())
                .unwrap_or_else(|| "（アクション不明）".to_string());
            lines.push(Line::from(format!("- {} — {}", a.id, title)));
        }
    }

    if show_evidence {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "根拠:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        if finding.evidence.is_empty() {
            lines.push(Line::from("（なし）"));
        } else {
            for ev in &finding.evidence {
                lines.push(Line::from(format!("- {:?}: {}", ev.kind, ev.value)));
            }
        }
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "ヒント: 'e' で根拠の表示を切り替えます。",
            Style::default().fg(Color::DarkGray),
        )));
    }

    Text::from(lines)
}

fn draw_actions(
    f: &mut ratatui::Frame,
    left: Rect,
    right: Rect,
    report: &Report,
    filter: &str,
    state: &mut ListState,
    color: bool,
) {
    let indices = report_filtered_action_indices(report, filter);
    App::move_list_selection(state, indices.len(), 0);

    let items: Vec<ListItem> = if indices.is_empty() {
        if filter.trim().is_empty() {
            vec![ListItem::new(Line::from("アクションがありません。"))]
        } else {
            vec![
                ListItem::new(Line::from("一致するアクションがありません。")),
                ListItem::new(Line::from(Span::styled(
                    "ヒント: '/' でフィルタを編集できます。",
                    Style::default().fg(Color::DarkGray),
                ))),
            ]
        }
    } else {
        indices
            .iter()
            .filter_map(|idx| report.actions.get(*idx))
            .map(|action| {
                let risk_style = risk_style(action.risk_level, color);
                let line = Line::from(vec![
                    Span::styled(
                        format!(
                            "{:>10}",
                            crate::ui::format_bytes(action.estimated_reclaimed_bytes)
                        ),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(" "),
                    Span::styled(action.risk_level.to_string(), risk_style),
                    Span::raw(" "),
                    Span::raw(action.title.clone()),
                ]);
                ListItem::new(line)
            })
            .collect()
    };

    let title = if filter.trim().is_empty() {
        format!("アクション（{}）", report.actions.len())
    } else {
        format!(
            "アクション（{shown}/{total}）",
            shown = indices.len(),
            total = report.actions.len()
        )
    };
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_stateful_widget(list, left, state);

    let detail = if let Some(sel) = state.selected() {
        indices
            .get(sel)
            .and_then(|idx| report.actions.get(*idx))
            .map(|action| action_detail(action, report))
    } else {
        None
    }
    .unwrap_or_else(|| Text::from("アクションが選択されていません。"));

    let w = Paragraph::new(detail)
        .block(Block::default().borders(Borders::ALL).title("詳細"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, right);
}

fn action_detail(action: &crate::core::ActionPlan, report: &Report) -> Text<'static> {
    let mut lines = Vec::<Line>::new();
    lines.push(Line::from(vec![
        Span::styled("id: ", Style::default().fg(Color::DarkGray)),
        Span::raw(action.id.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("リスク: ", Style::default().fg(Color::DarkGray)),
        Span::raw(action.risk_level.to_string()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("推定削減: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(action.estimated_reclaimed_bytes)),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "操作（この項目）:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    let is_cleanup_supported = matches!(
        action.id.as_str(),
        "xcode-archives-review" | "xcode-device-support-review" | "coresimulator-devices-xcrun"
    );
    if is_cleanup_supported {
        lines.push(Line::from(
            "- Enter / c: 個別削除を開く（候補を選択 → p でゴミ箱へ移動）",
        ));
    } else if action.risk_level == RiskLevel::R1
        && matches!(&action.kind, ActionKind::TrashMove { .. })
    {
        lines.push(Line::from("- Enter: この項目を適用（ゴミ箱へ移動）"));
        lines.push(Line::from("- Space で選択 → p でまとめて適用"));
    } else if crate::actions::allowlisted_run_cmd(action).is_some() {
        lines.push(Line::from("- Enter: この項目を実行（許可リスト RUN_CMD）"));
        lines.push(Line::from("- Space で選択 → x で実行（安全のため 1 つずつ）"));
    } else {
        match &action.kind {
            ActionKind::ShowInstructions { .. } => {
                lines.push(Line::from("- 手順表示のみです（TUIから自動実行しません）"));
            }
            ActionKind::RunCmd { .. } => {
                lines.push(Line::from("- RUN_CMD ですが許可リスト外のため実行できません"));
            }
            ActionKind::TrashMove { .. } => {
                lines.push(Line::from(
                    "- TRASH_MOVE ですがこの画面では適用できません（R2+は個別削除などを使用）",
                ));
            }
            ActionKind::OpenInFinder { .. } => {
                lines.push(Line::from("- Finder で開く操作は現状未対応です（手動で開いてください）"));
            }
            ActionKind::Delete { .. } => {
                lines.push(Line::from("- DELETE は安全のため TUI から実行できません"));
            }
        }
    }

    if !action.related_findings.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("対象: ", Style::default().fg(Color::DarkGray)),
            Span::raw(action.related_findings.join(",")),
        ]));
        lines.push(Line::from(Span::styled(
            "関連する所見:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for fid in action.related_findings.iter().take(8) {
            let title = report
                .findings
                .iter()
                .find(|f| &f.id == fid)
                .map(|f| f.title.as_str())
                .unwrap_or("（所見不明）");
            lines.push(Line::from(format!("- {fid} — {title}")));
        }
        if action.related_findings.len() > 8 {
            lines.push(Line::from(Span::styled(
                "…（省略）",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    lines.push(Line::from(""));

    match &action.kind {
        ActionKind::TrashMove { paths } => {
            lines.push(Line::from(Span::styled(
                "ゴミ箱へ移動（TRASH_MOVE）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for p in paths {
                lines.push(Line::from(format!("- {p}")));
            }
        }
        ActionKind::RunCmd { cmd, args } => {
            lines.push(Line::from(Span::styled(
                "コマンド実行（RUN_CMD）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!("$ {cmd} {}", args.join(" "))));
        }
        ActionKind::OpenInFinder { path } => {
            lines.push(Line::from(Span::styled(
                "Finderで開く（OPEN_IN_FINDER）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(path.clone()));
        }
        ActionKind::ShowInstructions { markdown } => {
            lines.push(Line::from(Span::styled(
                "手順表示（SHOW_INSTRUCTIONS）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            let head = markdown.lines().take(12).collect::<Vec<_>>().join("\n");
            lines.push(Line::from(head));
            if markdown.lines().count() > 12 {
                lines.push(Line::from(Span::styled(
                    "…（省略）",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        ActionKind::Delete { paths } => {
            lines.push(Line::from(Span::styled(
                "削除（DELETE）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for p in paths {
                lines.push(Line::from(format!("- {p}")));
            }
        }
    }

    if !action.notes.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "注記:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for n in &action.notes {
            lines.push(Line::from(format!("- {n}")));
        }
    }

    Text::from(lines)
}

fn cleanup_action_detail(action: &crate::core::ActionPlan) -> Text<'static> {
    let mut lines = Vec::<Line>::new();
    lines.push(Line::from(vec![
        Span::styled("id: ", Style::default().fg(Color::DarkGray)),
        Span::raw(action.id.clone()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("リスク: ", Style::default().fg(Color::DarkGray)),
        Span::raw(action.risk_level.to_string()),
    ]));
    lines.push(Line::from(vec![
        Span::styled("推定削減: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(action.estimated_reclaimed_bytes)),
    ]));

    lines.push(Line::from(""));

    match &action.kind {
        ActionKind::TrashMove { paths } => {
            lines.push(Line::from(Span::styled(
                "ゴミ箱へ移動（TRASH_MOVE）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for p in paths {
                lines.push(Line::from(format!("- {p}")));
            }
        }
        ActionKind::RunCmd { cmd, args } => {
            lines.push(Line::from(Span::styled(
                "コマンド実行（RUN_CMD）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!("$ {cmd} {}", args.join(" "))));
        }
        ActionKind::OpenInFinder { path } => {
            lines.push(Line::from(Span::styled(
                "Finderで開く（OPEN_IN_FINDER）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(path.clone()));
        }
        ActionKind::ShowInstructions { markdown } => {
            lines.push(Line::from(Span::styled(
                "手順表示（SHOW_INSTRUCTIONS）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            let head = markdown.lines().take(12).collect::<Vec<_>>().join("\n");
            lines.push(Line::from(head));
            if markdown.lines().count() > 12 {
                lines.push(Line::from(Span::styled(
                    "…（省略）",
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        ActionKind::Delete { paths } => {
            lines.push(Line::from(Span::styled(
                "削除（DELETE）",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for p in paths {
                lines.push(Line::from(format!("- {p}")));
            }
        }
    }

    if !action.notes.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "注記:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for n in &action.notes {
            lines.push(Line::from(format!("- {n}")));
        }
    }

    Text::from(lines)
}

fn fix_candidate_indices(report: &Report, max_risk: RiskLevel) -> Vec<usize> {
    let mut candidates: Vec<(usize, &crate::core::ActionPlan)> = report
        .actions
        .iter()
        .enumerate()
        .filter(|(_, a)| a.risk_level <= max_risk)
        .collect();

    candidates.sort_by(|(_, a), (_, b)| {
        (
            a.risk_level,
            std::cmp::Reverse(a.estimated_reclaimed_bytes),
            a.id.as_str(),
        )
            .cmp(&(
                b.risk_level,
                std::cmp::Reverse(b.estimated_reclaimed_bytes),
                b.id.as_str(),
            ))
    });

    candidates.into_iter().map(|(idx, _)| idx).collect()
}

fn filter_tokens(input: &str) -> Vec<String> {
    input
        .trim()
        .split_whitespace()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
        .collect()
}

fn matches_filter(haystack: &str, tokens: &[String]) -> bool {
    if tokens.is_empty() {
        return true;
    }
    let hay = haystack.to_ascii_lowercase();
    tokens.iter().all(|t| hay.contains(t))
}

fn logs_filtered_indices(entries: &[LogEntry], filter: &str) -> Vec<usize> {
    let tokens = filter_tokens(filter);
    if tokens.is_empty() {
        return (0..entries.len()).collect();
    }
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| matches_filter(&e.search_text, &tokens))
        .map(|(i, _)| i)
        .collect()
}

fn action_search_text(action: &crate::core::ActionPlan) -> String {
    let mut out = String::new();
    out.push_str(&action.id);
    out.push(' ');
    out.push_str(&action.title);
    out.push(' ');
    out.push_str(&action.risk_level.to_string());

    match &action.kind {
        ActionKind::TrashMove { paths } => {
            out.push_str(" TRASH_MOVE");
            for p in paths.iter().take(2) {
                out.push(' ');
                out.push_str(p);
            }
        }
        ActionKind::RunCmd { cmd, args } => {
            out.push_str(" RUN_CMD ");
            out.push_str(cmd);
            for a in args {
                out.push(' ');
                out.push_str(a);
            }
        }
        ActionKind::OpenInFinder { path } => {
            out.push_str(" OPEN_IN_FINDER ");
            out.push_str(path);
        }
        ActionKind::ShowInstructions { markdown } => {
            out.push_str(" SHOW_INSTRUCTIONS ");
            let head = markdown
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or("");
            out.push_str(head);
        }
        ActionKind::Delete { paths } => {
            out.push_str(" DELETE");
            for p in paths.iter().take(2) {
                out.push(' ');
                out.push_str(p);
            }
        }
    }

    for fid in &action.related_findings {
        out.push(' ');
        out.push_str(fid);
    }
    for note in &action.notes {
        out.push(' ');
        out.push_str(note);
    }

    out
}

fn utilities_candidate_indices(app: &App) -> Vec<usize> {
    app.utilities_actions
        .iter()
        .enumerate()
        .filter(|(_, a)| a.risk_level <= app.fix_max_risk)
        .map(|(i, _)| i)
        .collect()
}

fn cleanup_candidate_indices(app: &App) -> Vec<usize> {
    app.cleanup_actions
        .iter()
        .enumerate()
        .filter(|(_, a)| a.risk_level <= app.fix_max_risk)
        .map(|(i, _)| i)
        .collect()
}

fn utilities_filtered_indices(app: &App) -> Vec<usize> {
    let candidates = utilities_candidate_indices(app);
    let tokens = filter_tokens(&app.filter);
    if tokens.is_empty() {
        return candidates;
    }
    candidates
        .into_iter()
        .filter(|idx| {
            app.utilities_actions
                .get(*idx)
                .is_some_and(|a| matches_filter(&action_search_text(a), &tokens))
        })
        .collect()
}

fn cleanup_filtered_indices(app: &App) -> Vec<usize> {
    let candidates = cleanup_candidate_indices(app);
    let tokens = filter_tokens(&app.filter);
    if tokens.is_empty() {
        return candidates;
    }
    candidates
        .into_iter()
        .filter(|idx| {
            app.cleanup_actions
                .get(*idx)
                .is_some_and(|a| matches_filter(&action_search_text(a), &tokens))
        })
        .collect()
}

fn trim_utilities_selection(app: &mut App) {
    let indices = utilities_filtered_indices(app);
    App::move_list_selection(&mut app.utilities_state, indices.len(), 0);
}

fn trim_cleanup_selection(app: &mut App) {
    let indices = cleanup_filtered_indices(app);
    App::move_list_selection(&mut app.cleanup_state, indices.len(), 0);
}

fn finding_search_text(finding: &crate::core::Finding) -> String {
    let mut out = String::new();
    out.push_str(&finding.id);
    out.push(' ');
    out.push_str(&finding.finding_type);
    out.push(' ');
    out.push_str(&finding.risk_level.to_string());
    out.push(' ');
    out.push_str(&finding.title);
    for a in &finding.recommended_actions {
        out.push(' ');
        out.push_str(&a.id);
    }
    out
}

fn report_filtered_finding_indices(report: &Report, filter: &str) -> Vec<usize> {
    let tokens = filter_tokens(filter);
    report
        .findings
        .iter()
        .enumerate()
        .filter(|(_, f)| matches_filter(&finding_search_text(f), &tokens))
        .map(|(i, _)| i)
        .collect()
}

fn report_filtered_action_indices(report: &Report, filter: &str) -> Vec<usize> {
    let tokens = filter_tokens(filter);
    report
        .actions
        .iter()
        .enumerate()
        .filter(|(_, a)| matches_filter(&action_search_text(a), &tokens))
        .map(|(i, _)| i)
        .collect()
}

fn fix_filtered_candidate_indices(
    report: &Report,
    max_risk: RiskLevel,
    filter: &str,
) -> Vec<usize> {
    let candidates = fix_candidate_indices(report, max_risk);
    let tokens = filter_tokens(filter);
    if tokens.is_empty() {
        return candidates;
    }
    candidates
        .into_iter()
        .filter(|idx| {
            report
                .actions
                .get(*idx)
                .is_some_and(|a| matches_filter(&action_search_text(a), &tokens))
        })
        .collect()
}

fn toggle_fix_selected(app: &mut App) {
    let Some(report) = app.report.as_ref() else {
        return;
    };
    let candidates = fix_filtered_candidate_indices(report, app.fix_max_risk, &app.filter);
    let Some(sel) = app.fix_state.selected() else {
        return;
    };
    let Some(idx) = candidates.get(sel).copied() else {
        return;
    };
    let Some(action) = report.actions.get(idx) else {
        return;
    };

    if app.fix_selected.contains(&action.id) {
        app.fix_selected.remove(&action.id);
    } else {
        app.fix_selected.insert(action.id.clone());
    }
}

fn select_all_fix_candidates(app: &mut App) {
    let Some(report) = app.report.as_ref() else {
        return;
    };
    let candidates = fix_filtered_candidate_indices(report, app.fix_max_risk, &app.filter);
    for idx in candidates {
        if let Some(action) = report.actions.get(idx) {
            app.fix_selected.insert(action.id.clone());
        }
    }
}

fn toggle_cleanup_selected(app: &mut App) {
    let candidates = cleanup_filtered_indices(app);
    let Some(sel) = app.cleanup_state.selected() else {
        return;
    };
    let Some(idx) = candidates.get(sel).copied() else {
        return;
    };
    let Some(action) = app.cleanup_actions.get(idx) else {
        return;
    };

    if app.cleanup_selected.contains(&action.id) {
        app.cleanup_selected.remove(&action.id);
    } else {
        app.cleanup_selected.insert(action.id.clone());
    }
}

fn select_all_cleanup_candidates(app: &mut App) {
    let candidates = cleanup_filtered_indices(app);
    for idx in candidates {
        if let Some(action) = app.cleanup_actions.get(idx) {
            app.cleanup_selected.insert(action.id.clone());
        }
    }
}

fn trim_fix_selected(app: &mut App) {
    let Some(report) = app.report.as_ref() else {
        app.fix_selected.clear();
        app.fix_state.select(None);
        return;
    };

    let candidates = fix_candidate_indices(report, app.fix_max_risk);
    let allowed: HashSet<&str> = candidates
        .iter()
        .filter_map(|idx| report.actions.get(*idx))
        .map(|a| a.id.as_str())
        .collect();
    app.fix_selected.retain(|id| allowed.contains(id.as_str()));
    let visible = fix_filtered_candidate_indices(report, app.fix_max_risk, &app.filter);
    App::move_list_selection(&mut app.fix_state, visible.len(), 0);
}

fn draw_notes(f: &mut ratatui::Frame, area: Rect, report: &Report, filter: &str, scroll: u16) {
    let tokens = filter_tokens(filter);
    let filtered_notes: Vec<&String> = report
        .summary
        .notes
        .iter()
        .filter(|n| matches_filter(n, &tokens))
        .collect();

    let mut lines = Vec::<Line>::new();
    lines.push(Line::from(vec![
        Span::styled("推定合計: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(
            report.summary.estimated_total_bytes,
        )),
        Span::raw("  "),
        Span::styled("未観測≈: ", Style::default().fg(Color::DarkGray)),
        Span::raw(crate::ui::format_bytes(report.summary.unobserved_bytes)),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "注記:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    if filtered_notes.is_empty() && !filter.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            "（一致する注記がありません）",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for n in &filtered_notes {
            lines.push(Line::from(format!("- {n}")));
        }
    }
    let text = Text::from(lines);

    let title = if filter.trim().is_empty() {
        "概要".to_string()
    } else {
        format!(
            "概要（注記 {shown}/{total}）",
            shown = filtered_notes.len(),
            total = report.summary.notes.len()
        )
    };
    let w = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(w, area);
}

fn draw_error(f: &mut ratatui::Frame, area: Rect, app: &App) {
    let msg = app
        .error
        .as_deref()
        .unwrap_or("不明なエラーです。")
        .to_string();
    let w = Paragraph::new(msg)
        .block(Block::default().borders(Borders::ALL).title("エラー"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, area);
}

fn draw_help(f: &mut ratatui::Frame, area: Rect, _app: &App) {
    let text = Text::from(vec![
        Line::from(Span::styled(
            "macdiet UI（Phase 7）",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("ホーム:"),
        Line::from("  Enter : 選択中コマンドを実行"),
        Line::from("  :     : 検索入力の切替"),
        Line::from("  Esc   : 検索入力を閉じる"),
        Line::from("  ↑↓ / j/k: 選択を移動"),
        Line::from("  q     : 終了（検索入力中は文字入力）"),
        Line::from("  Ctrl-C: 強制終了（どの画面でも）"),
        Line::from(""),
        Line::from("レポート:"),
        Line::from("  Tab / Shift-Tab : タブ切替"),
        Line::from("  ↑↓ / j/k         : 選択移動 / 注記スクロール"),
        Line::from("  e               : 根拠の表示切替（所見）"),
        Line::from("  /               : フィルタ（所見/アクション/注記を絞り込み）"),
        Line::from("  r               : 再実行"),
        Line::from("  b               : 戻る"),
        Line::from(""),
        Line::from("掃除（dry-run）:"),
        Line::from("  ↑↓/j/k: 移動  Space: 選択  Enter: おすすめ  a: 全選択  n: 全解除"),
        Line::from(
            "  1/2/3: 最大リスク  p: 適用（ゴミ箱へ移動/R1）  x: 実行（許可リストRUN_CMD）  c: 個別削除（対応項目）  r: 更新  b: 戻る",
        ),
        Line::from("  /  : フィルタ（候補を絞り込み）"),
        Line::from("  （RUN_CMD結果）f/g: 修復（表示される場合）  r: 更新  b: 戻る"),
        Line::from(""),
        Line::from("ユーティリティ:"),
        Line::from("  ↑↓/j/k: 選択  1/2/3: 最大リスク  x/Enter: 実行（許可リスト RUN_CMD）"),
        Line::from("  /  : フィルタ（一覧を絞り込み）  b: 戻る"),
        Line::from(""),
        Line::from("スキャン（設定）:"),
        Line::from("  ↑↓/j/k: 項目選択  Enter: 編集  r: 実行  b: 戻る"),
        Line::from(""),
        Line::from("ログ:"),
        Line::from("  ↑↓/j/k: 選択  Enter: 開く  r: 更新  b: 戻る"),
        Line::from("  /  : フィルタ（一覧を絞り込み）"),
        Line::from("  （詳細）↑↓/j/k: スクロール  r: 再読込  b: 戻る"),
        Line::from(""),
        Line::from("フィルタ入力:"),
        Line::from("  Enter/Esc: 入力終了  Backspace: 削除  Ctrl-U: クリア"),
        Line::from(""),
        Line::from(Span::styled(
            "安全設計:",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  既定は読み取り専用です。適用には入力による確認が必要です（yes → trash）。"),
        Line::from(
            "  RUN_CMD の実行は厳格な許可リストに限定されます（例: `brew cleanup`, `npm cache clean --force`, `docker system prune`, `xcrun simctl delete unavailable`）。",
        ),
        Line::from("  --dry-run で起動した場合、破壊的操作は無効です。"),
        Line::from(""),
        Line::from("確認入力（typed confirm）:"),
        Line::from("  Enter: 送信  Backspace: 削除  Esc: キャンセル"),
    ]);

    let w = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("ヘルプ"))
        .wrap(Wrap { trim: false });
    f.render_widget(w, centered_rect(70, 70, area));
}

fn open_scan_config(app: &mut App) {
    app.scan_edit_mode = false;
    app.scan_config_state.select(Some(0));
    app.screen = Screen::ScanConfig;
}

fn open_logs(app: &mut App) {
    refresh_logs(app);
    if matches!(app.screen, Screen::Error) {
        return;
    }
    app.screen = Screen::LogsList;
}

fn open_utilities(app: &mut App) {
    trim_utilities_selection(app);
    app.screen = Screen::Utilities;
}

fn default_utilities_actions() -> Vec<crate::core::ActionPlan> {
    let mut out = vec![
        crate::core::ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "Homebrew cache を整理（`brew cleanup`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["homebrew-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![
                "注: `brew cleanup -s` はより積極的です。必要性を理解してから実行してください。"
                    .to_string(),
                "ヒント: Homebrew の操作中（install/upgrade 等）は避けてください。".to_string(),
            ],
        },
        crate::core::ActionPlan {
            id: "npm-cache-cleanup".to_string(),
            title: "npm cache を整理（`npm cache clean --force`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["npm-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "npm".to_string(),
                args: vec![
                    "cache".to_string(),
                    "clean".to_string(),
                    "--force".to_string(),
                ],
            },
            notes: vec![
                "影響: npm のキャッシュを削除します。次回 `npm install` が遅くなる可能性があります。"
                    .to_string(),
                "注: `--force` の意味を理解してから実行してください。".to_string(),
            ],
        },
        crate::core::ActionPlan {
            id: "yarn-cache-cleanup".to_string(),
            title: "Yarn cache を整理（`yarn cache clean`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["yarn-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "yarn".to_string(),
                args: vec!["cache".to_string(), "clean".to_string()],
            },
            notes: vec![
                "影響: Yarn のキャッシュを削除します。次回 `yarn install` が遅くなる可能性があります。"
                    .to_string(),
                "注: yarn の実行中（install 等）は避けてください。".to_string(),
            ],
        },
        crate::core::ActionPlan {
            id: "pnpm-store-prune".to_string(),
            title: "pnpm store を整理（`pnpm store prune`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["pnpm-store".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "pnpm".to_string(),
                args: vec!["store".to_string(), "prune".to_string()],
            },
            notes: vec![
                "影響: 未使用の store データを削除します。次回 `pnpm install` が遅くなる可能性があります。"
                    .to_string(),
                "注: pnpm の実行中（install 等）は避けてください。".to_string(),
            ],
        },
        crate::core::ActionPlan {
            id: "docker-storage-df".to_string(),
            title: "Docker の使用量を確認（`docker system df`）".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["docker-desktop-data".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "docker".to_string(),
                args: vec!["system".to_string(), "df".to_string()],
            },
            notes: vec![
                "注: これは読み取り専用の確認コマンドです（削除は行いません）。".to_string(),
                "ヒント: `docker system prune` は破壊的になり得るため慎重に。".to_string(),
            ],
        },
        crate::core::ActionPlan {
            id: "docker-builder-prune".to_string(),
            title: "Docker build cache を prune（`docker builder prune`）（R2）".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["docker-desktop-data".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "docker".to_string(),
                args: vec!["builder".to_string(), "prune".to_string()],
            },
            notes: vec![
                "影響: ビルドキャッシュを削除します（次回ビルドが遅くなる可能性があります）。"
                    .to_string(),
                "ヒント: 事前に `docker system df` で内訳を確認してください。".to_string(),
            ],
        },
        crate::core::ActionPlan {
            id: "docker-system-prune".to_string(),
            title: "Docker の未使用データを prune（`docker system prune`）（R2）".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["docker-desktop-data".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "docker".to_string(),
                args: vec!["system".to_string(), "prune".to_string()],
            },
            notes: vec![
                "影響: 未使用のコンテナ/ネットワーク/イメージ(dangling)/build cache を削除します。"
                    .to_string(),
                "ヒント: 何が削除されるか理解していない限り `--all` / `--volumes` は避けてください。"
                    .to_string(),
            ],
        },
        crate::core::ActionPlan {
            id: "coresimulator-simctl-delete-unavailable".to_string(),
            title: "利用できないシミュレータを削除（`xcrun simctl delete unavailable`）（R2）"
                .to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["coresimulator-devices".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "xcrun".to_string(),
                args: vec![
                    "simctl".to_string(),
                    "delete".to_string(),
                    "unavailable".to_string(),
                ],
            },
            notes: vec![
                "影響: 利用できないデバイスを削除します。シミュレータの状態が変わる可能性があります。"
                    .to_string(),
                "ヒント: 事前に `xcrun simctl list devices unavailable` で確認してください。"
                    .to_string(),
            ],
        },
    ];

    out.sort_by(|a, b| (a.risk_level, a.id.as_str()).cmp(&(b.risk_level, b.id.as_str())));
    out
}

fn scan_field_mut(app: &mut App) -> &mut String {
    match app.scan_config_state.selected().unwrap_or(0) {
        0 => &mut app.scan_scope_input,
        1 => &mut app.scan_max_depth_input,
        2 => &mut app.scan_top_dirs_input,
        3 => &mut app.scan_exclude_input,
        _ => &mut app.scan_scope_input,
    }
}

fn trim_scan_inputs(app: &mut App) {
    app.scan_scope_input = app.scan_scope_input.trim().to_string();
    app.scan_max_depth_input = app.scan_max_depth_input.trim().to_string();
    app.scan_top_dirs_input = app.scan_top_dirs_input.trim().to_string();
    app.scan_exclude_input = app.scan_exclude_input.trim().to_string();
}

fn start_scan_from_inputs(app: &mut App, engine: Engine) {
    let req = match parse_scan_request_from_inputs(app) {
        Ok(req) => req,
        Err(err) => {
            open_error(app, err.to_string());
            return;
        }
    };

    app.last_scan_request = Some(req);
    start_run(app, engine, CommandKind::ScanDeep);
}

fn parse_scan_request_from_inputs(app: &App) -> Result<ScanRequest> {
    let scope = app.scan_scope_input.trim().to_string();
    let scope = if scope.is_empty() { None } else { Some(scope) };

    let max_depth = app
        .scan_max_depth_input
        .trim()
        .parse::<usize>()
        .context("scan max_depth は数値で指定してください")?;
    let top_dirs = app
        .scan_top_dirs_input
        .trim()
        .parse::<usize>()
        .context("scan top_dirs は数値で指定してください")?;
    if top_dirs == 0 {
        return Err(anyhow::anyhow!(
            "scan top_dirs は 0 より大きい必要があります"
        ));
    }

    let mut exclude: Vec<String> = app
        .scan_exclude_input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    exclude.sort();
    exclude.dedup();

    crate::scan::validate_excludes(&exclude).context("scan exclude のパターンが不正です")?;

    Ok(ScanRequest {
        scope,
        deep: true,
        max_depth,
        top_dirs,
        exclude,
        show_progress: false,
    })
}

fn default_scan_request(app: &App) -> ScanRequest {
    let scope = app.scan_default_scope.trim().to_string();
    let scope = if scope.is_empty() { None } else { Some(scope) };
    ScanRequest {
        scope,
        deep: true,
        max_depth: 3,
        top_dirs: 20,
        exclude: app.scan_exclude.clone(),
        show_progress: false,
    }
}

fn refresh_logs(app: &mut App) {
    let indices = logs_filtered_indices(&app.logs_entries, &app.filter);
    let selected_name = app
        .logs_state
        .selected()
        .and_then(|i| indices.get(i))
        .and_then(|idx| app.logs_entries.get(*idx))
        .map(|e| e.file_name.clone());

    let entries = match load_logs_entries(&app.home_dir) {
        Ok(v) => v,
        Err(err) => {
            open_error(app, err.to_string());
            return;
        }
    };

    app.logs_entries = entries;
    app.logs_scroll = 0;
    app.logs_view = None;

    let indices = logs_filtered_indices(&app.logs_entries, &app.filter);
    if indices.is_empty() {
        app.logs_state.select(None);
        return;
    }

    let selected = selected_name
        .as_deref()
        .and_then(|name| app.logs_entries.iter().position(|e| e.file_name == name))
        .and_then(|idx| indices.iter().position(|i| *i == idx))
        .unwrap_or(0);
    app.logs_state.select(Some(selected));
    refresh_logs_preview(app);
}

fn refresh_logs_preview(app: &mut App) {
    let indices = logs_filtered_indices(&app.logs_entries, &app.filter);
    App::move_list_selection(&mut app.logs_state, indices.len(), 0);

    let Some(sel) = app.logs_state.selected() else {
        app.logs_view = None;
        return;
    };
    let Some(idx) = indices.get(sel).copied() else {
        app.logs_view = None;
        return;
    };
    let Some(entry) = app.logs_entries.get(idx) else {
        app.logs_view = None;
        return;
    };
    if app
        .logs_view
        .as_ref()
        .is_some_and(|v| v.entry.path == entry.path)
    {
        return;
    }

    match load_log_detail(&app.home_dir, entry, 256 * 1024) {
        Ok(detail) => app.logs_view = Some(detail),
        Err(err) => {
            open_error(app, err.to_string());
        }
    }
}

fn open_selected_log_detail(app: &mut App) {
    let indices = logs_filtered_indices(&app.logs_entries, &app.filter);
    App::move_list_selection(&mut app.logs_state, indices.len(), 0);

    let Some(sel) = app.logs_state.selected() else {
        return;
    };
    let Some(idx) = indices.get(sel).copied() else {
        return;
    };
    let Some(entry) = app.logs_entries.get(idx) else {
        return;
    };

    match load_log_detail(&app.home_dir, entry, 512 * 1024) {
        Ok(detail) => {
            app.logs_view = Some(detail);
            app.logs_scroll = 0;
            app.screen = Screen::LogsDetail;
        }
        Err(err) => {
            open_error(app, err.to_string());
        }
    }
}

fn reload_log_detail(app: &mut App) {
    let Some(view) = app.logs_view.as_ref() else {
        refresh_logs_preview(app);
        return;
    };

    match load_log_detail(&app.home_dir, &view.entry, 512 * 1024) {
        Ok(detail) => app.logs_view = Some(detail),
        Err(err) => {
            open_error(app, err.to_string());
        }
    }
}

fn load_logs_entries(home_dir: &std::path::Path) -> Result<Vec<LogEntry>> {
    let dir = crate::logs::logs_dir(home_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = Vec::<LogEntry>::new();
    for ent in std::fs::read_dir(&dir)
        .with_context(|| format!("ログディレクトリの読み取り: {}", dir.display()))?
    {
        let ent = ent.with_context(|| format!("ログエントリの読み取り: {}", dir.display()))?;
        let path = ent.path();
        let file_name = ent.file_name().to_string_lossy().to_string();

        if !file_name.ends_with(".json") {
            continue;
        }

        let md = ent
            .metadata()
            .with_context(|| format!("メタデータ取得: {}", path.display()))?;
        if !md.is_file() {
            continue;
        }

        let modified_unix_nanos = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos());

        let search_text = log_entry_search_text(home_dir, &file_name, &path);

        entries.push(LogEntry {
            file_name,
            path,
            size: md.len(),
            modified_unix_nanos,
            search_text,
        });
    }

    entries.sort_by(
        |a, b| match (a.modified_unix_nanos, b.modified_unix_nanos) {
            (Some(a), Some(b)) => b.cmp(&a),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => b.file_name.cmp(&a.file_name),
        },
    );

    Ok(entries)
}

fn log_entry_search_text(
    home_dir: &std::path::Path,
    file_name: &str,
    path: &std::path::Path,
) -> String {
    let mut out = String::new();
    out.push_str(file_name);
    out.push(' ');
    out.push_str(log_kind_label(file_name));

    if let Ok((content, _truncated)) = read_file_limited(path, 48 * 1024) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            for key in [
                "command",
                "status",
                "action_id",
                "risk_level",
                "requested_id",
            ] {
                if let Some(val) = v.get(key).and_then(|x| x.as_str()) {
                    out.push(' ');
                    out.push_str(val);
                }
            }

            if let Some(cmd) = v.pointer("/attempt/cmd").and_then(|x| x.as_str()) {
                out.push(' ');
                out.push_str(cmd);
            }
            if let Some(args) = v.pointer("/attempt/args").and_then(|x| x.as_array()) {
                for a in args.iter().filter_map(|x| x.as_str()).take(8) {
                    out.push(' ');
                    out.push_str(a);
                }
            }
        }
    }

    let masked = mask_home_path(path, Some(home_dir));
    out.push(' ');
    out.push_str(&masked);
    out
}

fn load_log_detail(
    home_dir: &std::path::Path,
    entry: &LogEntry,
    max_bytes: usize,
) -> Result<LogDetail> {
    let (content, truncated) = read_file_limited(&entry.path, max_bytes)?;

    let mut summary = Vec::<String>::new();
    let mut parse_error = None;

    if truncated {
        summary.push("省略: true".to_string());
    }

    match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(v) => {
            if let Some(cmd) = v.get("command").and_then(|x| x.as_str()) {
                summary.push(format!("コマンド: {cmd}"));
            }
            if let Some(status) = v.get("status").and_then(|x| x.as_str()) {
                summary.push(format!("状態: {status}"));
            }
            if let Some(started) = v.get("started_at").and_then(|x| x.as_str()) {
                summary.push(format!("開始: {started}"));
            }
            if let Some(finished) = v.get("finished_at").and_then(|x| x.as_str()) {
                summary.push(format!("終了: {finished}"));
            }

            if v.get("command").and_then(|x| x.as_str()) == Some("fix") {
                if let Some(max_risk) = v.get("max_risk").and_then(|x| x.as_str()) {
                    summary.push(format!("最大リスク: {max_risk}"));
                }
                let action_count = v
                    .get("actions")
                    .and_then(|x| x.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let moved = v
                    .pointer("/outcome/moved")
                    .and_then(|x| x.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let skipped_missing = v
                    .pointer("/outcome/skipped_missing")
                    .and_then(|x| x.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                let errors = v
                    .pointer("/outcome/errors")
                    .and_then(|x| x.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0);
                summary.push(format!(
                    "アクション: {action_count}  移動: {moved}  スキップ(不存在): {skipped_missing}  エラー: {errors}"
                ));
            } else if v.get("command").and_then(|x| x.as_str()) == Some("fix run_cmd") {
                if let Some(action_id) = v.get("action_id").and_then(|x| x.as_str()) {
                    summary.push(format!("アクションID: {action_id}"));
                }
                if let Some(risk_level) = v.get("risk_level").and_then(|x| x.as_str()) {
                    summary.push(format!("リスク: {risk_level}"));
                }
                if let Some(exit_code) = v.pointer("/attempt/exit_code").and_then(|x| x.as_i64()) {
                    summary.push(format!("exit_code: {exit_code}"));
                }
                if let Some(cmd) = v.pointer("/attempt/cmd").and_then(|x| x.as_str()) {
                    let args = v
                        .pointer("/attempt/args")
                        .and_then(|x| x.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str())
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    if args.is_empty() {
                        summary.push(format!("コマンド: {cmd}"));
                    } else {
                        summary.push(format!("コマンド: {cmd} {args}"));
                    }
                }
            } else if v.get("command").and_then(|x| x.as_str()) == Some("snapshots thin") {
                if let Some(bytes) = v.get("bytes").and_then(|x| x.as_u64()) {
                    summary.push(format!("容量: {}", crate::ui::format_bytes(bytes)));
                }
                if let Some(urgency) = v.get("urgency").and_then(|x| x.as_u64()) {
                    summary.push(format!("優先度: {urgency}"));
                }
                if let Some(exit_code) = v.pointer("/attempt/exit_code").and_then(|x| x.as_i64()) {
                    summary.push(format!("exit_code: {exit_code}"));
                }
            } else if v.get("command").and_then(|x| x.as_str()) == Some("snapshots delete") {
                if let Some(requested) = v.get("requested_id").and_then(|x| x.as_str()) {
                    summary.push(format!("指定ID: {requested}"));
                }
                if let Some(uuid) = v.get("resolved_uuid").and_then(|x| x.as_str()) {
                    summary.push(format!("解決UUID: {uuid}"));
                }
                if let Some(exit_code) = v
                    .pointer("/list_attempt/exit_code")
                    .and_then(|x| x.as_i64())
                {
                    summary.push(format!("listのexit_code: {exit_code}"));
                }
                if let Some(exit_code) = v
                    .pointer("/delete_attempt/exit_code")
                    .and_then(|x| x.as_i64())
                {
                    summary.push(format!("deleteのexit_code: {exit_code}"));
                }
            }
        }
        Err(err) => {
            parse_error = Some(err.to_string());
        }
    }

    let masked_path = mask_home_path(&entry.path, Some(home_dir));
    if masked_path != entry.path.display().to_string() {
        summary.push(format!("ログパス: {masked_path}"));
    }

    Ok(LogDetail {
        entry: LogEntry {
            file_name: entry.file_name.clone(),
            path: entry.path.clone(),
            size: entry.size,
            modified_unix_nanos: entry.modified_unix_nanos,
            search_text: entry.search_text.clone(),
        },
        summary,
        content,
        truncated,
        parse_error,
    })
}

fn read_file_limited(path: &std::path::Path, max_bytes: usize) -> Result<(String, bool)> {
    use std::io::Read as _;

    let f = std::fs::File::open(path)
        .with_context(|| format!("ログを開けません: {}", path.display()))?;
    let mut buf = Vec::new();
    let mut limited = f.take((max_bytes as u64).saturating_add(1));
    limited
        .read_to_end(&mut buf)
        .with_context(|| format!("ログを読み取れません: {}", path.display()))?;

    let truncated = buf.len() > max_bytes;
    if truncated {
        buf.truncate(max_bytes);
    }

    Ok((String::from_utf8_lossy(&buf).to_string(), truncated))
}

fn log_kind_label(file_name: &str) -> &'static str {
    if file_name.starts_with("fix-apply-") {
        "掃除"
    } else if file_name.starts_with("fix-run-cmd-") {
        "RUN_CMD"
    } else if file_name.starts_with("snapshots-thin-") {
        "スナップショット(thin)"
    } else if file_name.starts_with("snapshots-delete-") {
        "スナップショット(delete)"
    } else {
        "ログ"
    }
}

fn mask_home_path(path: &std::path::Path, home_dir: Option<&std::path::Path>) -> String {
    let Some(home_dir) = home_dir else {
        return path.display().to_string();
    };
    let Ok(stripped) = path.strip_prefix(home_dir) else {
        return path.display().to_string();
    };
    let stripped = stripped.display().to_string();
    if stripped.is_empty() {
        "~".to_string()
    } else {
        format!("~/{stripped}")
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn risk_style(risk: RiskLevel, enabled: bool) -> Style {
    if !enabled {
        return Style::default();
    }
    match risk {
        RiskLevel::R0 => Style::default().fg(Color::DarkGray),
        RiskLevel::R1 => Style::default().fg(Color::Green),
        RiskLevel::R2 => Style::default().fg(Color::Yellow),
        RiskLevel::R3 => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{ActionKind, ActionPlan, ActionRef, Finding, OsInfo, ReportSummary};
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_HOME_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempHomeDir {
        path: PathBuf,
    }

    impl TempHomeDir {
        fn new() -> Self {
            let pid = std::process::id();
            let n = TEMP_HOME_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("macdiet-test-home-{pid}-{n}"));
            std::fs::create_dir_all(&path).expect("テスト用ホームディレクトリ作成");
            Self { path }
        }
    }

    impl Drop for TempHomeDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn test_engine() -> Engine {
        Engine::new(crate::engine::EngineOptions {
            timeout: Duration::from_secs(1),
            privacy_mask_home: false,
            include_evidence: false,
            show_progress: false,
        })
        .expect("テスト用 Engine の初期化")
    }

    fn report_with_actions(actions: Vec<ActionPlan>) -> Report {
        Report {
            schema_version: "1.0".to_string(),
            tool_version: "test".to_string(),
            os: OsInfo {
                name: "macOS".to_string(),
                version: "test".to_string(),
            },
            generated_at: "test".to_string(),
            summary: ReportSummary {
                estimated_total_bytes: 0,
                unobserved_bytes: 0,
                notes: vec![],
            },
            findings: vec![],
            actions,
        }
    }

    fn report_with_findings_and_actions(
        findings: Vec<Finding>,
        actions: Vec<ActionPlan>,
    ) -> Report {
        Report {
            schema_version: "1.0".to_string(),
            tool_version: "test".to_string(),
            os: OsInfo {
                name: "macOS".to_string(),
                version: "test".to_string(),
            },
            generated_at: "test".to_string(),
            summary: ReportSummary {
                estimated_total_bytes: 0,
                unobserved_bytes: 0,
                notes: vec![],
            },
            findings,
            actions,
        }
    }

    fn finding(
        id: &str,
        risk: RiskLevel,
        finding_type: &str,
        title: &str,
        actions: &[&str],
    ) -> Finding {
        Finding {
            id: id.to_string(),
            finding_type: finding_type.to_string(),
            title: title.to_string(),
            estimated_bytes: 0,
            confidence: 1.0,
            risk_level: risk,
            evidence: vec![],
            recommended_actions: actions
                .iter()
                .map(|id| ActionRef {
                    id: (*id).to_string(),
                })
                .collect(),
        }
    }

    fn action(id: &str, risk: RiskLevel, bytes: u64) -> ActionPlan {
        ActionPlan {
            id: id.to_string(),
            title: format!("title-{id}"),
            risk_level: risk,
            estimated_reclaimed_bytes: bytes,
            related_findings: vec![],
            kind: ActionKind::ShowInstructions {
                markdown: "test".to_string(),
            },
            notes: vec![],
        }
    }

    fn trash_action(id: &str, risk: RiskLevel, bytes: u64) -> ActionPlan {
        ActionPlan {
            id: id.to_string(),
            title: format!("title-{id}"),
            risk_level: risk,
            estimated_reclaimed_bytes: bytes,
            related_findings: vec![],
            kind: ActionKind::TrashMove {
                paths: vec!["~/Library/Developer/Xcode/DerivedData".to_string()],
            },
            notes: vec![],
        }
    }

    #[test]
    fn fix_candidate_indices_filters_and_sorts() {
        let report = report_with_actions(vec![
            action("b", RiskLevel::R1, 10),
            action("a", RiskLevel::R1, 20),
            action("c", RiskLevel::R0, 5),
            action("d", RiskLevel::R2, 100),
            action("e", RiskLevel::R1, 20),
        ]);

        assert_eq!(fix_candidate_indices(&report, RiskLevel::R0), vec![2]);
        assert_eq!(
            fix_candidate_indices(&report, RiskLevel::R1),
            vec![2, 1, 4, 0]
        );
    }

    #[test]
    fn trim_fix_selected_removes_non_candidates_and_clamps_selection() {
        let report = report_with_actions(vec![
            action("r1", RiskLevel::R1, 1),
            action("r2", RiskLevel::R2, 2),
        ]);

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.report = Some(report);
        app.fix_selected.insert("r1".to_string());
        app.fix_selected.insert("r2".to_string());
        app.fix_state.select(Some(10));

        trim_fix_selected(&mut app);

        assert!(app.fix_selected.contains("r1"));
        assert!(!app.fix_selected.contains("r2"));
        assert_eq!(app.fix_state.selected(), Some(0));
    }

    #[test]
    fn selected_fix_apply_actions_includes_only_r1_trashmove() {
        let report = report_with_actions(vec![
            trash_action("ok", RiskLevel::R1, 10),
            trash_action("skip_r2", RiskLevel::R2, 99),
            action("skip_kind", RiskLevel::R1, 99),
        ]);

        let selected: HashSet<String> = ["ok", "skip_r2", "skip_kind"]
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let (actions, selected_total, ignored_total) =
            selected_fix_apply_actions(&report, &selected);
        assert_eq!(selected_total, 3);
        assert_eq!(ignored_total, 2);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, "ok");
    }

    #[test]
    fn fix_run_cmd_confirm_allows_b_input() {
        let engine = test_engine();

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.screen = Screen::FixRunCmdConfirm;
        app.fix_run_cmd_confirm = Some(FixRunCmdConfirm {
            stage: RunCmdConfirmStage::Token,
            input: String::new(),
            actions: vec![],
            selected_total: 0,
            ignored_total: 0,
            confirm_token: "unavailable".to_string(),
            final_confirm_token: "run".to_string(),
            error: None,
            return_to: Screen::FixView,
            result_return_to: Screen::FixView,
        });

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::FixRunCmdConfirm);
        assert_eq!(
            app.fix_run_cmd_confirm.as_ref().expect("confirm").input,
            "b"
        );
    }

    #[test]
    fn fix_run_cmd_confirm_esc_cancels() {
        let engine = test_engine();

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.screen = Screen::FixRunCmdConfirm;
        app.fix_run_cmd_confirm = Some(FixRunCmdConfirm {
            stage: RunCmdConfirmStage::Token,
            input: "un".to_string(),
            actions: vec![],
            selected_total: 0,
            ignored_total: 0,
            confirm_token: "unavailable".to_string(),
            final_confirm_token: "run".to_string(),
            error: None,
            return_to: Screen::FixView,
            result_return_to: Screen::FixView,
        });

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::FixView);
        assert!(app.fix_run_cmd_confirm.is_none());
    }

    #[test]
    fn fix_run_cmd_result_can_start_repair_flow_with_f() {
        let engine = test_engine();

        let original = ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "Homebrew cache を整理（`brew cleanup`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["homebrew-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![],
        };
        let repair = ActionPlan {
            id: "homebrew-cellar-permissions-chmod".to_string(),
            title: "Homebrew Cellar の権限を修復（`chmod -R u+rwX`）（R2）".to_string(),
            risk_level: RiskLevel::R2,
            estimated_reclaimed_bytes: 0,
            related_findings: vec!["homebrew-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "chmod".to_string(),
                args: vec![
                    "-R".to_string(),
                    "u+rwX".to_string(),
                    "/opt/homebrew/Cellar/python@3.13/3.13.2".to_string(),
                ],
            },
            notes: vec![],
        };

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R2,
            false,
            "dev".to_string(),
            vec![],
        );
        app.screen = Screen::FixRunCmdResult;
        app.fix_run_cmd_result = Some(FixRunCmdResult {
            results: vec![FixRunCmdActionResult {
                action: original,
                exit_code: Some(1),
                warning: None,
                error: Some("fail".to_string()),
                repair_actions: vec![repair],
                log_path: None,
                log_error: None,
            }],
            return_to: Screen::Utilities,
        });

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::FixRunCmdConfirm);
        let confirm = app.fix_run_cmd_confirm.as_ref().expect("confirm");
        assert_eq!(confirm.return_to, Screen::FixRunCmdResult);
        assert_eq!(confirm.result_return_to, Screen::Utilities);
        assert_eq!(confirm.confirm_token, "chmod");
        assert_eq!(confirm.final_confirm_token, "run");
    }

    #[test]
    fn home_query_mode_allows_q_input() {
        let engine = test_engine();

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.query_mode = true;

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.query, "q");
    }

    #[test]
    fn home_jk_navigates_when_not_query_mode() {
        let engine = test_engine();

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        assert_eq!(app.command_state.selected(), Some(0));

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.command_state.selected(), Some(1));

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.command_state.selected(), Some(0));
    }

    #[test]
    fn home_query_mode_allows_j_input() {
        let engine = test_engine();

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.query_mode = true;

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.query, "j");
    }

    #[test]
    fn error_screen_returns_to_fix_view_after_ineligible_apply() {
        let engine = test_engine();

        let report = report_with_actions(vec![
            ActionPlan {
                id: "homebrew-cache-cleanup-info".to_string(),
                title: "Homebrew cache を整理（手順）".to_string(),
                risk_level: RiskLevel::R1,
                estimated_reclaimed_bytes: 0,
                related_findings: vec!["homebrew-cache".to_string()],
                kind: ActionKind::ShowInstructions {
                    markdown: "test".to_string(),
                },
                notes: vec![],
            },
            ActionPlan {
                id: "homebrew-cache-trash".to_string(),
                title: "Homebrew cache をゴミ箱へ移動（R1）".to_string(),
                risk_level: RiskLevel::R1,
                estimated_reclaimed_bytes: 100,
                related_findings: vec!["homebrew-cache".to_string()],
                kind: ActionKind::TrashMove {
                    paths: vec!["~/Library/Caches/Homebrew".to_string()],
                },
                notes: vec![],
            },
        ]);

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.screen = Screen::FixView;
        app.report = Some(report);
        app.fix_selected
            .insert("homebrew-cache-cleanup-info".to_string());

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::Error);
        assert_eq!(app.error_return_to, Screen::FixView);
        assert!(
            app.error
                .as_deref()
                .is_some_and(|s| s.contains("homebrew-cache-trash")),
            "error={:?}",
            app.error
        );

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::FixView);
        assert!(app.error.is_none());
    }

    #[test]
    fn fix_apply_p_routes_to_run_cmd_confirm_when_allowlisted_run_cmd_selected() {
        let engine = test_engine();

        let report = report_with_actions(vec![ActionPlan {
            id: "homebrew-cache-cleanup".to_string(),
            title: "Homebrew cache を整理（`brew cleanup`）".to_string(),
            risk_level: RiskLevel::R1,
            estimated_reclaimed_bytes: 100,
            related_findings: vec!["homebrew-cache".to_string()],
            kind: ActionKind::RunCmd {
                cmd: "brew".to_string(),
                args: vec!["cleanup".to_string()],
            },
            notes: vec![],
        }]);

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.screen = Screen::FixView;
        app.report = Some(report);
        app.fix_selected
            .insert("homebrew-cache-cleanup".to_string());

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::FixRunCmdConfirm);

        let confirm = app.fix_run_cmd_confirm.as_ref().expect("confirm");
        assert_eq!(confirm.confirm_token, "cleanup");
        assert_eq!(confirm.final_confirm_token, "run");
    }

    #[test]
    fn fix_result_b_triggers_refresh_before_returning_to_fix_view() {
        let engine = test_engine();

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.screen = Screen::FixResult;

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::Running);
        assert!(
            app.pending
                .as_ref()
                .is_some_and(|p| p.kind == CommandKind::FixDryRun),
            "pending={:?}",
            app.pending.as_ref().map(|p| p.kind)
        );
    }

    #[test]
    fn fix_run_cmd_result_b_triggers_refresh_before_returning_to_fix_view() {
        let engine = test_engine();

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.screen = Screen::FixRunCmdResult;
        app.fix_run_cmd_result = Some(FixRunCmdResult {
            results: vec![],
            return_to: Screen::FixView,
        });

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::Running);
        assert!(
            app.pending
                .as_ref()
                .is_some_and(|p| p.kind == CommandKind::FixDryRun),
            "pending={:?}",
            app.pending.as_ref().map(|p| p.kind)
        );
    }

    #[test]
    fn fix_run_cmd_result_b_returns_to_utilities_without_refresh() {
        let engine = test_engine();

        let mut app = App::new(
            false,
            PathBuf::from("/Users/test"),
            RiskLevel::R1,
            false,
            "dev".to_string(),
            vec![],
        );
        app.screen = Screen::FixRunCmdResult;
        app.fix_run_cmd_result = Some(FixRunCmdResult {
            results: vec![],
            return_to: Screen::Utilities,
        });

        let quit = handle_key(
            &mut app,
            &engine,
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE),
        )
        .expect("handle_key");
        assert!(!quit);
        assert_eq!(app.screen, Screen::Utilities);
        assert!(app.pending.is_none());
    }

    #[test]
    fn report_filtered_indices_match_case_insensitively() {
        let report = report_with_findings_and_actions(
            vec![
                finding(
                    "f0",
                    RiskLevel::R1,
                    "xcode",
                    "DerivedData が巨大です",
                    &["a-trash"],
                ),
                finding("f1", RiskLevel::R2, "docker", "Docker images", &["a-run"]),
            ],
            vec![
                ActionPlan {
                    id: "a-trash".to_string(),
                    title: "DerivedData を移動".to_string(),
                    risk_level: RiskLevel::R1,
                    estimated_reclaimed_bytes: 0,
                    related_findings: vec!["f0".to_string()],
                    kind: ActionKind::TrashMove {
                        paths: vec!["~/Library/Developer/Xcode/DerivedData".to_string()],
                    },
                    notes: vec![],
                },
                ActionPlan {
                    id: "a-run".to_string(),
                    title: "利用できないシミュレータを削除".to_string(),
                    risk_level: RiskLevel::R2,
                    estimated_reclaimed_bytes: 0,
                    related_findings: vec!["f1".to_string()],
                    kind: ActionKind::RunCmd {
                        cmd: "xcrun".to_string(),
                        args: vec![
                            "simctl".to_string(),
                            "delete".to_string(),
                            "unavailable".to_string(),
                        ],
                    },
                    notes: vec![],
                },
            ],
        );

        assert_eq!(report_filtered_finding_indices(&report, "DOCKER"), vec![1]);
        assert_eq!(report_filtered_finding_indices(&report, "a-trash"), vec![0]);
        assert_eq!(
            report_filtered_action_indices(&report, "xcrun unavailable"),
            vec![1]
        );
    }

    #[test]
    fn logs_filtered_indices_filters_by_search_text() {
        let entries = vec![
            LogEntry {
                file_name: "fix-apply-1.json".to_string(),
                path: PathBuf::from("/tmp/fix-apply-1.json"),
                size: 1,
                modified_unix_nanos: None,
                search_text: "fix apply status OK".to_string(),
            },
            LogEntry {
                file_name: "snapshots-thin-1.json".to_string(),
                path: PathBuf::from("/tmp/snapshots-thin-1.json"),
                size: 1,
                modified_unix_nanos: None,
                search_text: "snapshots thin urgency 9".to_string(),
            },
        ];

        assert_eq!(logs_filtered_indices(&entries, ""), vec![0, 1]);
        assert_eq!(logs_filtered_indices(&entries, "snapshots 9"), vec![1]);
        assert_eq!(
            logs_filtered_indices(&entries, "missing"),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn parse_simctl_unavailable_devices_extracts_uuid_and_name() {
        let stdout = "== Devices ==\n-- iOS 17.0 --\n    iPhone 14 (AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE) (Shutdown) (unavailable, runtime profile not found)\n";
        assert_eq!(
            parse_simctl_unavailable_devices(stdout),
            vec![(
                "iPhone 14".to_string(),
                "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".to_string()
            )]
        );
    }

    #[test]
    fn build_cleanup_actions_xcode_archives_detects_xcarchive_dirs() {
        let home = TempHomeDir::new();
        let base = home
            .path
            .join("Library/Developer/Xcode/Archives/2026-01-01");
        std::fs::create_dir_all(&base).expect("archives base");
        let a = base.join("App 2026-01-01 00.00.00.xcarchive");
        std::fs::create_dir_all(&a).expect("xcarchive");
        let b = home
            .path
            .join("Library/Developer/Xcode/Archives/Single.xcarchive");
        std::fs::create_dir_all(&b).expect("xcarchive direct");

        let actions =
            build_cleanup_actions(CleanupKind::XcodeArchives, &home.path, Duration::from_secs(0))
                .expect("build");
        assert_eq!(actions.len(), 2);
        for a in actions {
            assert_eq!(a.risk_level, RiskLevel::R2);
            assert!(a.id.starts_with("cleanup-xcode-archives-"));
            let ActionKind::TrashMove { paths } = &a.kind else {
                panic!("expected TrashMove");
            };
            assert_eq!(paths.len(), 1);
            assert!(paths[0].starts_with("~/Library/Developer/Xcode/Archives/"));
        }
    }

    #[test]
    fn build_cleanup_actions_device_support_detects_subdirs() {
        let home = TempHomeDir::new();
        let base = home
            .path
            .join("Library/Developer/Xcode/iOS DeviceSupport/17.0 (21A000)");
        std::fs::create_dir_all(&base).expect("devicesupport");

        let actions = build_cleanup_actions(
            CleanupKind::XcodeDeviceSupport,
            &home.path,
            Duration::from_secs(0),
        )
        .expect("build");
        assert_eq!(actions.len(), 1);
        assert!(actions[0].id.starts_with("cleanup-xcode-device-support-"));
    }

    #[test]
    fn build_cleanup_actions_core_simulator_unavailable_errors_when_timeout_is_zero() {
        let home = TempHomeDir::new();
        let base = home
            .path
            .join("Library/Developer/CoreSimulator/Devices/AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE");
        std::fs::create_dir_all(&base).expect("coresim devices");

        let err = build_cleanup_actions(
            CleanupKind::CoreSimulatorUnavailable,
            &home.path,
            Duration::from_secs(0),
        )
        .expect_err("should error");
        assert!(err
            .to_string()
            .contains("タイムアウト予算が 0 のため"));
    }
}
