mod app;
mod bookmark;
mod candidate_match;
mod config;
mod dashboard;
mod export;
mod filter_model;
mod fuzzy;
mod help;
mod highlight_model;
mod ingest;
mod input;
mod keymap;
mod model;
mod path_complete;
mod picker;
mod preset;
mod preview;
mod recent;
mod scan;
mod source_panel;
mod store;
mod text_field;
mod theme;
mod time_panel;
mod ui;
mod vocab;

use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

use std::path::PathBuf;

use clap::Parser;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Position;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Terminal;

use app::App;
use filter_model::{Group, GroupList, TimeBound};
use fuzzy::SameFieldOp;

/// Page size for Ctrl-d/Ctrl-u paging in the log list. `App` doesn't track
/// the rendered viewport height (that's `ui.rs`'s job at render time), so a
/// fixed page size is the simplest correct approach.
const PAGE_SIZE: isize = 10;
const LEVEL_CANDIDATES: &[&str] = &["V", "D", "I", "W", "E", "F"];

#[inline]
fn km_code(app: &App, id: keymap::ActionId, code: KeyCode) -> bool {
    app.keymap.matches_code(id, code)
}

#[inline]
fn km_event(app: &App, id: keymap::ActionId, key: event::KeyEvent) -> bool {
    app.keymap.matches_event(id, key)
}

fn level_field_candidates(query: &str) -> Vec<String> {
    crate::fuzzy::fuzzy_str_labels(LEVEL_CANDIDATES, query)
}

#[derive(Parser)]
#[command(
    name = "alnav",
    about = "Interactive vim-style App/Android Log Navigator (TUI)"
)]
struct TuiCli {
    /// Log file to browse
    #[arg(short, long)]
    file: Option<String>,

    #[arg(short, long, value_name = "REGEX")]
    tag: Vec<String>,

    #[arg(short, long, value_name = "REGEX")]
    msg: Vec<String>,

    #[arg(short, long, value_name = "LEVEL")]
    level: Option<String>,

    #[arg(long, value_name = "REGEX")]
    pkg: Vec<String>,

    #[arg(long, value_name = "REGEX")]
    pid: Vec<String>,

    #[arg(long, value_name = "REGEX")]
    tid: Vec<String>,

    #[arg(long, value_name = "TIME")]
    since: Option<String>,

    #[arg(long, value_name = "TIME")]
    until: Option<String>,

    /// Accepted for CLI compatibility; TUI filter/search are always case-insensitive.
    #[arg(short = 'i', long)]
    ignore_case: bool,

    /// Max in-memory lines before the oldest are evicted (live mode only; ignored for `-f`)
    #[arg(long, default_value_t = 500_000)]
    max_lines: usize,

    /// Capture logs directly from hdc hilog (HarmonyOS device)
    #[arg(long)]
    hdc: bool,

    /// Capture logs directly from adb logcat (Android device)
    #[arg(long)]
    adb: bool,

    /// Device serial number (for --hdc or --adb with multiple devices)
    #[arg(long, value_name = "SERIAL")]
    device: Option<String>,

    /// Config directory override (reads `theme.toml`/`config.toml`/`keymap.toml`; default: `$ALNAV_HOME` or `~/.config/alnav`)
    #[arg(long, value_name = "DIR")]
    config_path: Option<PathBuf>,

    /// Write default `config.toml` and `keymap.toml` into the config directory, then exit.
    #[arg(long)]
    init: bool,

    /// With `--init`, overwrite existing config/keymap files.
    #[arg(long)]
    force: bool,
}

fn validate_source(cli: &TuiCli) -> Result<(), String> {
    if cli.hdc && cli.adb {
        return Err("--hdc and --adb are mutually exclusive".into());
    }
    if (cli.hdc || cli.adb) && cli.file.is_some() {
        return Err("--hdc/--adb cannot be combined with -f".into());
    }
    if cli.device.is_some() && !cli.hdc && !cli.adb {
        return Err("--device requires --hdc or --adb".into());
    }
    // No source → Dashboard (deferred bind). Filters may still be provided.
    Ok(())
}

/// Startup chip Filter group (no time — time goes to [`App::time_bound`]).
fn initial_group(cli: &TuiCli) -> Result<GroupList, String> {
    // TUI always matches case-insensitively (CLI `-i` is a no-op).
    // Startup CLI multi-values keep OR (`msg:a|b` label); interactive chips use And.
    if cli.tag.is_empty()
        && cli.msg.is_empty()
        && cli.pkg.is_empty()
        && cli.pid.is_empty()
        && cli.tid.is_empty()
        && cli.level.is_none()
    {
        return Ok(GroupList::default());
    }
    if let Some(l) = &cli.level {
        if alnav::parser::Level::from_str(l).is_none() {
            return Err(format!("unknown level '{}', expected V/D/I/W/E/F", l));
        }
    }
    let mut chips = Vec::new();
    for v in &cli.tag {
        chips.push(input::Chip {
            field: input::ChipField::Tag,
            value: v.clone(),
        });
    }
    for v in &cli.msg {
        chips.push(input::Chip {
            field: input::ChipField::Msg,
            value: v.clone(),
        });
    }
    for v in &cli.pkg {
        chips.push(input::Chip {
            field: input::ChipField::Pkg,
            value: v.clone(),
        });
    }
    for v in &cli.pid {
        chips.push(input::Chip {
            field: input::ChipField::Pid,
            value: v.clone(),
        });
    }
    for v in &cli.tid {
        chips.push(input::Chip {
            field: input::ChipField::Tid,
            value: v.clone(),
        });
    }
    if let Some(l) = &cli.level {
        chips.push(input::Chip {
            field: input::ChipField::Level,
            value: l.clone(),
        });
    }
    let mut label_parts = Vec::new();
    if !cli.tag.is_empty() {
        label_parts.push(format!("tag:{}", cli.tag.join("|")));
    }
    if !cli.msg.is_empty() {
        label_parts.push(format!("msg:{}", cli.msg.join("|")));
    }
    if !cli.pkg.is_empty() {
        label_parts.push(format!("pkg:{}", cli.pkg.join("|")));
    }
    if !cli.pid.is_empty() {
        label_parts.push(format!("pid:{}", cli.pid.join("|")));
    }
    if !cli.tid.is_empty() {
        label_parts.push(format!("tid:{}", cli.tid.join("|")));
    }
    if let Some(l) = &cli.level {
        label_parts.push(format!("level:{l}"));
    }
    let label = if label_parts.is_empty() {
        "(startup filter)".to_string()
    } else {
        label_parts.join(" AND ")
    };
    Ok(GroupList {
        groups: vec![Group {
            label,
            chips,
            enabled: true,
            same_field_op: SameFieldOp::Or,
        }],
        excludes: Vec::new(),
    })
}

/// Startup `--since`/`--until` → global session time window (not a Filter group).
fn initial_time_bound(cli: &TuiCli) -> Option<TimeBound> {
    if cli.since.is_none() && cli.until.is_none() {
        return None;
    }
    Some(TimeBound {
        since: cli.since.clone(),
        until: cli.until.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> TuiCli {
        let mut full = vec!["alnav"];
        full.extend_from_slice(args);
        TuiCli::parse_from(full)
    }

    #[test]
    fn test_initial_group_empty_cli_yields_empty_group_list() {
        let c = cli(&["-f", "app.log"]);
        let groups = initial_group(&c).unwrap();
        assert!(groups.groups.is_empty());
    }

    #[test]
    fn adb_source_is_accepted() {
        let cli = cli(&["--adb", "--device", "ANDROID"]);
        assert!(cli.adb);
        assert!(!cli.hdc);
        assert_eq!(cli.device.as_deref(), Some("ANDROID"));
    }

    #[test]
    fn live_source_validation_rejects_conflicts() {
        assert_eq!(
            validate_source(&cli(&["--adb", "--hdc"])).unwrap_err(),
            "--hdc and --adb are mutually exclusive"
        );
        assert_eq!(
            validate_source(&cli(&["--adb", "-f", "app.log"])).unwrap_err(),
            "--hdc/--adb cannot be combined with -f"
        );
        assert_eq!(
            validate_source(&cli(&["--device", "ANDROID"])).unwrap_err(),
            "--device requires --hdc or --adb"
        );
    }

    #[test]
    fn no_source_is_allowed_for_dashboard() {
        assert!(validate_source(&cli(&[])).is_ok());
        assert!(validate_source(&cli(&["--tag", "Foo"])).is_ok());
    }

    #[test]
    fn test_initial_group_pid_only_label_contains_pid() {
        let c = cli(&["-f", "app.log", "--pid", "1234"]);
        let groups = initial_group(&c).unwrap();
        assert_eq!(groups.groups.len(), 1);
        assert!(
            groups.groups[0].label.contains("pid:1234"),
            "label was: {}",
            groups.groups[0].label
        );
    }

    #[test]
    fn test_initial_group_multiple_fields_label_contains_all() {
        let c = cli(&[
            "-f",
            "app.log",
            "-t",
            "TagA",
            "-m",
            "boom",
            "--pkg",
            "com.example",
            "--pid",
            "111",
            "--tid",
            "222",
            "-l",
            "E",
            "--since",
            "10:00:00",
            "--until",
            "10:01:00",
        ]);
        let groups = initial_group(&c).unwrap();
        assert_eq!(groups.groups.len(), 1);
        let label = &groups.groups[0].label;
        assert!(label.contains("tag:TagA"), "label was: {label}");
        assert!(label.contains("msg:boom"), "label was: {label}");
        assert!(label.contains("pkg:com.example"), "label was: {label}");
        assert!(label.contains("pid:111"), "label was: {label}");
        assert!(label.contains("tid:222"), "label was: {label}");
        assert!(label.contains("level:E"), "label was: {label}");
        // Time is global, not on the Filter group label.
        assert!(!label.contains("since:"), "label was: {label}");
        assert!(!label.contains("until:"), "label was: {label}");
        let bound = initial_time_bound(&c).unwrap();
        assert_eq!(bound.since.as_deref(), Some("10:00:00"));
        assert_eq!(bound.until.as_deref(), Some("10:01:00"));
    }

    #[test]
    fn test_initial_time_only_yields_empty_groups() {
        let c = cli(&["-f", "app.log", "--since", "10:00:00"]);
        let groups = initial_group(&c).unwrap();
        assert!(groups.groups.is_empty());
        let bound = initial_time_bound(&c).unwrap();
        assert_eq!(bound.since.as_deref(), Some("10:00:00"));
        assert!(bound.until.is_none());
    }
}

/// Fixed backoff between live reconnect spawn attempts.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(2);

/// After spawn, wait briefly and reject sessions whose child already exited
/// (common when `hdc`/`adb` starts then immediately fails without a device).
const RECONNECT_HEALTH_WAIT: Duration = Duration::from_millis(150);

/// Ensures the live child process is killed no matter which exit path
/// `main()` takes, including an early `?` bail-out between binding this
/// guard and the end of `main()`. Manual kill/wait calls threaded into every
/// fallible line are easy to miss when new fallible calls are added later;
/// `Drop` closes that gap structurally instead.
struct LiveChildGuard(Option<std::process::Child>);

impl LiveChildGuard {
    fn new(child: Option<std::process::Child>) -> Self {
        Self(child)
    }

    /// Kill/wait the previous child (if any), then take ownership of `child`.
    fn replace(&mut self, child: Option<std::process::Child>) {
        if let Some(mut old) = self.0.take() {
            let _ = old.kill();
            let _ = old.wait();
        }
        self.0 = child;
    }
}

impl Drop for LiveChildGuard {
    fn drop(&mut self) {
        self.replace(None);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveBackend {
    Hdc,
    Adb,
}

/// Replaceable live ingest session for `--hdc` / `--adb` TUI mode.
struct LiveIngestCtl {
    backend: LiveBackend,
    device: Option<String>,
    ingest: Option<ingest::IngestHandle>,
    child: LiveChildGuard,
    last_reconnect_at: Option<Instant>,
}

impl LiveIngestCtl {
    fn new(
        backend: LiveBackend,
        device: Option<String>,
        ingest: ingest::IngestHandle,
        child: std::process::Child,
    ) -> Self {
        Self {
            backend,
            device,
            ingest: Some(ingest),
            child: LiveChildGuard::new(Some(child)),
            last_reconnect_at: None,
        }
    }

    /// When `app.ingest_done`, attempt respawn subject to [`RECONNECT_BACKOFF`].
    /// On success: swap ring/child, [`App::mark_live_reconnected`], keep buffers.
    /// `spawn` is injectable for tests; production uses [`Self::spawn_for_reconnect`].
    ///
    /// `last_reconnect_at` is always stamped on an attempt (success or fail) so a
    /// short-lived false session cannot immediately re-flash `RECONNECTED`.
    fn try_reconnect<F>(&mut self, app: &mut App, now: Instant, spawn: F) -> bool
    where
        F: FnOnce() -> Result<alnav::live::LiveSession, String>,
    {
        if !app.ingest_done {
            return false;
        }
        if let Some(at) = self.last_reconnect_at {
            if now.duration_since(at) < RECONNECT_BACKOFF {
                return false;
            }
        }
        self.last_reconnect_at = Some(now);
        match spawn() {
            Ok(session) => match ensure_capture_alive(session, RECONNECT_HEALTH_WAIT) {
                Ok(session) => {
                    let (ring, child) = ingest::spawn_live_ingest(session);
                    self.ingest = Some(ingest::IngestHandle::Ring(ring));
                    self.child.replace(Some(child));
                    app.mark_live_reconnected();
                    true
                }
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    /// Probe device reachability, then spawn. Spawn alone is not enough: `hdc`
    /// / `adb` often start successfully with no device and exit immediately.
    fn spawn_for_reconnect(
        backend: LiveBackend,
        device: Option<&str>,
    ) -> Result<alnav::live::LiveSession, String> {
        match backend {
            LiveBackend::Hdc => {
                if alnav::hdc::now_marker(device).is_none() {
                    return Err("hdc device unreachable".into());
                }
                alnav::hdc::spawn_hilog(device)
            }
            LiveBackend::Adb => {
                if alnav::adb::now_marker(device).is_none() {
                    return Err("adb device unreachable".into());
                }
                alnav::adb::spawn_logcat(device)
            }
        }
    }

    fn try_reconnect_now(&mut self, app: &mut App, now: Instant) -> bool {
        let backend = self.backend;
        let device = self.device.clone();
        self.try_reconnect(app, now, || {
            Self::spawn_for_reconnect(backend, device.as_deref())
        })
    }
}

fn spawn_live_ctl(backend: LiveBackend, device: Option<String>) -> Result<LiveIngestCtl, String> {
    let session = LiveIngestCtl::spawn_for_reconnect(backend, device.as_deref())?;
    let (ring, child) = ingest::spawn_live_ingest(session);
    Ok(LiveIngestCtl::new(
        backend,
        device,
        ingest::IngestHandle::Ring(ring),
        child,
    ))
}

/// Bind a file source. When `switching`, resets non-F/E/H state after open succeeds.
fn bind_file_source(app: &mut App, path: &str, switching: bool) -> Result<(), String> {
    let path_buf = std::path::PathBuf::from(path);
    if !path_buf.exists() {
        return Err(format!("file not found: {path}"));
    }
    if path_buf.is_dir() {
        return Err(format!("not a file: {path}"));
    }
    let file_store = store::FileStore::open(&path_buf).map_err(|e| e.to_string())?;
    if switching {
        app.reset_for_source_switch();
    }
    app.set_file_store(file_store);
    app.export_source = export::ExportSource::File(path_buf.display().to_string());
    app.dashboard = None;
    app.close_source_panels();
    app.record_recent_file(&path_buf);
    if app.filter_active() {
        app.rebuild_visible();
    }
    Ok(())
}

/// Bind HDC/ADB. Clears `time_bound` (live has no interactive time window).
fn bind_live_source(
    app: &mut App,
    live: &mut Option<LiveIngestCtl>,
    backend: LiveBackend,
    switching: bool,
) -> Result<(), String> {
    let ctl = spawn_live_ctl(backend, None)?;
    if switching {
        *live = None;
        app.reset_for_source_switch();
    }
    app.time_bound = None;
    app.export_source = match backend {
        LiveBackend::Hdc => export::ExportSource::Hdc { device: None },
        LiveBackend::Adb => export::ExportSource::Adb { device: None },
    };
    *live = Some(ctl);
    app.dashboard = None;
    app.close_source_panels();
    app.ingest_done = false;
    if app.filter_active() {
        app.rebuild_visible();
    }
    Ok(())
}

/// Reject capture children that die during the post-spawn grace window.
fn ensure_capture_alive(
    mut session: alnav::live::LiveSession,
    wait: Duration,
) -> Result<alnav::live::LiveSession, String> {
    std::thread::sleep(wait);
    match session.child.try_wait() {
        Ok(None) => Ok(session),
        Ok(Some(status)) => {
            let _ = session.child.kill();
            let _ = session.child.wait();
            Err(format!("capture exited immediately ({status})"))
        }
        Err(e) => {
            let _ = session.child.kill();
            let _ = session.child.wait();
            Err(format!("capture health check failed: {e}"))
        }
    }
}

fn argv0_basename() -> String {
    std::env::args_os()
        .next()
        .map(|a| {
            PathBuf::from(a)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .unwrap_or_else(|| "alnav".into())
}

fn main() -> Result<(), String> {
    let bin = argv0_basename();
    match bin.as_str() {
        "aloggrep" | "alg" => {
            let cli = alnav::Cli::parse();
            alnav::run_cli(cli);
            Ok(())
        }
        "aloggrep-tui" => run_tui(TuiCli::parse()),
        _ => {
            // Default product entry: `alnav` (and cargo-run target names).
            let args: Vec<String> = std::env::args().collect();
            if args.get(1).map(String::as_str) == Some("grep") {
                let mut cli_args = vec!["alnav grep".to_string()];
                cli_args.extend(args.into_iter().skip(2));
                let cli = alnav::Cli::parse_from(cli_args);
                alnav::run_cli(cli);
                Ok(())
            } else {
                run_tui(TuiCli::parse())
            }
        }
    }
}

fn run_tui(cli: TuiCli) -> Result<(), String> {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            event::DisableMouseCapture
        );
        default_panic(info);
    }));

    let config_dir = config::resolve_config_dir(cli.config_path.as_deref());
    if cli.init {
        for line in keymap::init_config_dir(&config_dir, cli.force)? {
            println!("{line}");
        }
        return Ok(());
    }

    if let Err(error) = validate_source(&cli) {
        eprintln!("alnav: {error}");
        std::process::exit(2);
    }

    if !io::stdout().is_terminal() {
        eprintln!("alnav: stdout is not a terminal, refusing to start");
        std::process::exit(2);
    }

    let theme_status = config::load_theme(&config_dir);
    let (app_config, config_status) = config::load_config(&config_dir);
    let (keymap_store, keymap_status) = keymap::load_keymap(&config_dir);

    let groups = initial_group(&cli)?;
    let time_bound = initial_time_bound(&cli);
    let mut app = App::new(cli.max_lines);
    app.config = app_config;
    app.keymap = keymap_store;
    app.config_dir = config_dir.clone();
    app.recent = recent::RecentFiles::load(&config_dir);
    app.groups = groups;
    app.time_bound = time_bound;
    if let Some(hint) = theme_status.status_hint() {
        app.set_flash(hint);
    }
    if let Some(hint) = config_status.status_hint() {
        app.set_flash(hint);
    }
    if let Some(hint) = keymap_status.status_hint() {
        app.set_flash(hint);
    } else if let Some(hint) = keymap::KeymapLoadStatus::warning_hint(&app.keymap.warnings) {
        app.set_flash(hint);
    }

    let mut live = if cli.hdc || cli.adb {
        let backend = if cli.hdc {
            LiveBackend::Hdc
        } else {
            LiveBackend::Adb
        };
        app.export_source = if cli.hdc {
            export::ExportSource::Hdc {
                device: cli.device.clone(),
            }
        } else {
            export::ExportSource::Adb {
                device: cli.device.clone(),
            }
        };
        match spawn_live_ctl(backend, cli.device.clone()) {
            Ok(ctl) => Some(ctl),
            Err(error) => {
                eprintln!("alnav: {error}");
                std::process::exit(2);
            }
        }
    } else if let Some(path) = cli.file.clone() {
        match bind_file_source(&mut app, &path, false) {
            Ok(()) => None,
            Err(error) => {
                eprintln!("alnav: {error}");
                std::process::exit(2);
            }
        }
    } else {
        // Preserve startup time_bound until a source is chosen; stream bind clears it.
        app.dashboard = Some(dashboard::DashboardState::new(app.recent.clone()));
        None
    };

    enable_raw_mode().map_err(|e| e.to_string())?;
    let setup: Result<Terminal<CrosstermBackend<io::Stdout>>, String> = (|| {
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            event::EnableMouseCapture
        )
        .map_err(|e| e.to_string())?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
        // Hide until a draft surface requests a hardware caret via
        // `Frame::set_cursor_position` (ratatui shows only when set).
        terminal.hide_cursor().map_err(|e| e.to_string())?;
        Ok(terminal)
    })();
    let mut terminal = match setup {
        Ok(t) => t,
        Err(e) => {
            let _ = disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                LeaveAlternateScreen,
                event::DisableMouseCapture
            );
            // `live` drops here, killing the child if any
            return Err(e);
        }
    };

    let mut input = input::InputBox::default();
    let _ = cli.ignore_case; // retained for CLI compat; TUI always ignore-case
    let result = run(&mut terminal, &mut app, &mut input, &mut live);

    let disable_result = disable_raw_mode().map_err(|e| e.to_string());
    let leave_result = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )
    .map_err(|e| e.to_string());
    // `live` drops here, killing the child if not already killed
    disable_result.and(leave_result)?;

    result
}

/// Anchors the candidate dropdown just below the centered Input/Search modal.
fn popup_rect(modal: Rect, frame_area: Rect, match_count: usize) -> Rect {
    ui::candidate_popup_rect(modal, frame_area, match_count)
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text.to_string()))
        .map_err(|e| e.to_string())
}

fn apply_yank(app: &mut App, text: String) {
    app.record_yank(text.clone());
    match copy_to_clipboard(&text) {
        Ok(()) => app.set_flash("YANKED (approx)"),
        Err(e) => app.set_flash(format!("YANK FAILED: {e}")),
    }
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    input: &mut input::InputBox,
    live: &mut Option<LiveIngestCtl>,
) -> Result<(), String> {
    use app::Mode;

    // P4: minimum draw interval during active ingest to avoid thrashing the
    // renderer while the background thread floods the channel.
    // After ingest completes (ingest_done=true) or after any user event,
    // we always draw immediately.
    const MIN_INGEST_DRAW_MS: u64 = 50;
    let mut last_draw = Instant::now();
    let mut force_draw = true; // first frame always draws

    while !app.should_quit {
        if let Some(ctl) = live.as_mut() {
            if let Some(ingest) = ctl.ingest.as_ref() {
                app.drain(ingest);
            }
            if app.ingest_done && ctl.try_reconnect_now(app, Instant::now()) {
                if let Some(ingest) = ctl.ingest.as_ref() {
                    app.drain(ingest);
                }
            }
        }
        app.poll_file_store();
        if let Some(panel) = app.open_file_panel.as_mut() {
            panel.poll_preview();
        }
        app.ensure_vocab_candidates();
        app.poll_vocab_match();
        app.poll_summary_job();
        app.tick_flash();
        // P1: recompute highlight match stats once per frame (O(n) scan).
        // All mutation paths set match_stats_stale=true; here is the single
        // amortised recompute point so render_status_bar just reads a cached value.
        app.recompute_match_stats_if_stale();

        // P4: throttle draws during active file ingest to ~20 FPS.
        // User events (force_draw=true) and post-ingest mode always draw immediately.
        let elapsed_ms = last_draw.elapsed().as_millis() as u64;
        let should_draw = force_draw || app.ingest_done || elapsed_ms >= MIN_INGEST_DRAW_MS;

        if should_draw {
            force_draw = false;
            last_draw = Instant::now();

            terminal
                .draw(|frame| {
                    let frame_area = frame.area();
                    let mut hw_cursor: Option<Position> = None;

                    if app.dashboard.is_some() && app.open_file_panel.is_none() {
                        ui::render_dashboard(app, frame, frame_area);
                        if let Some(pos) = hw_cursor {
                            frame.set_cursor_position(pos);
                        }
                        return;
                    }

                    let outer_w = frame_area.width;
                    let filter_h = ui::filter_strip_height(app, outer_w);
                    let exclude_h = ui::exclude_strip_height(app, outer_w);
                    let search_h = ui::highlight_strip_height(app, outer_w);
                    let [filter_area, exclude_area, highlight_strip_area, log_area, status_area] =
                        Layout::vertical([
                            Constraint::Length(filter_h),
                            Constraint::Length(exclude_h),
                            Constraint::Length(search_h),
                            Constraint::Fill(1),
                            Constraint::Length(1),
                        ])
                        .areas(frame_area);
                    ui::render_chip_strip(app, frame, filter_area);
                    ui::render_exclude_chip_strip(app, frame, exclude_area);
                    ui::render_highlight_chip_strip(app, frame, highlight_strip_area);
                    ui::render_log_list(app, frame, log_area);
                    ui::render_status_bar(app, frame, status_area);

                    let modal_w = ui::modal_width(frame_area.width);
                    let preview_limit =
                        ui::picker_preview_capacity(frame_area, ui::PICKER_PREVIEW_LEFT_RATIO);
                    let preview_inner_w =
                        ui::picker_preview_inner_width(frame_area, ui::PICKER_PREVIEW_LEFT_RATIO);
                    if let Some(panel) = app.open_file_panel.as_ref() {
                        hw_cursor = ui::render_open_file_panel(
                            panel,
                            app.config.picker_left_ratio,
                            frame,
                            frame_area,
                        );
                    } else if let Some(panel) = app.stream_source_panel.as_ref() {
                        ui::render_stream_source_panel(panel, frame, frame_area);
                    } else if let Some(data) =
                        picker_render_data(app, preview_limit, preview_inner_w)
                    {
                        let picker_area = ui::picker_frame_rect(frame_area, data.show_preview);
                        let right_pane = match (&data.detail_row, &data.preset_preview) {
                            (Some(row), _) => ui::PickerRightPane::Detail(row.as_ref()),
                            (None, Some(lines)) => ui::PickerRightPane::ChipRules(lines),
                            (None, None) => ui::PickerRightPane::Hits(&data.preview),
                        };
                        hw_cursor = ui::render_picker(
                            &data.title,
                            &data.mode,
                            &data.text,
                            data.caret,
                            &data.match_query,
                            &data.chips,
                            data.exclude_chips,
                            data.draft_field,
                            &data.labels,
                            &data.styles,
                            &data.checked,
                            &data.actions,
                            data.selected,
                            &data.empty_msg,
                            right_pane,
                            app.config.picker_left_ratio,
                            data.show_preview,
                            data.prompt_icon,
                            frame,
                            frame_area,
                        );
                        if let Some(confirm) = app
                            .picker
                            .as_ref()
                            .and_then(|session| session.confirm.as_ref())
                        {
                            ui::render_confirm_dialog(confirm, frame, picker_area);
                            // Don't let the search caret poke through the dialog.
                            hw_cursor = None;
                        }
                    // Search / Input use top stack: modal → candidates → Preview (H1).
                    } else if app.highlight_box.editing {
                        let area =
                            ui::top_modal_rect(frame_area, modal_w, ui::search_modal_height());
                        hw_cursor = ui::render_highlight_modal(&app.highlight_box, frame, area);
                        let cand = app
                            .highlight_box
                            .candidate_indices(&app.highlight_groups.groups)
                            .len();
                        let mut stack_bottom = area;
                        if cand > 0 {
                            let rect = popup_rect(area, frame_area, cand);
                            ui::render_highlight_popup(
                                &app.highlight_box,
                                &app.highlight_groups.groups,
                                frame,
                                rect,
                            );
                            stack_bottom = rect;
                        }
                        let prev = ui::preview_popup_rect(stack_bottom, frame_area);
                        if prev.height > 0 {
                            let limit = ui::preview_content_capacity(prev);
                            let preview_lines =
                                preview::preview_search_lines(app, limit).unwrap_or_default();
                            ui::render_preview(
                                "Preview",
                                &preview_lines,
                                "输入以预览",
                                frame,
                                prev,
                            );
                        }
                    } else if app.focus == app::Focus::Input {
                        let input_area = ui::top_modal_rect(frame_area, modal_w, 3);
                        hw_cursor = ui::render_input_modal(input, app.mode, frame, input_area);
                        let mut stack_bottom = input_area;
                        if input.field_popup_visible() {
                            // `.max(1)` so empty-match state still gets a row for「无匹配字段」.
                            let count = input.field_candidates().len().max(1);
                            let rect = popup_rect(input_area, frame_area, count);
                            ui::render_popup(input, frame, rect);
                            stack_bottom = rect;
                        }
                        let prev = ui::preview_popup_rect(stack_bottom, frame_area);
                        if prev.height > 0 {
                            let limit = ui::preview_content_capacity(prev);
                            let preview_lines = app.preview_filter_throttled(input, limit);
                            ui::render_preview("Preview", &preview_lines, "无匹配行", frame, prev);
                        }
                    } else if app.time_panel.is_some() {
                        let h = ui::time_panel_height(frame_area);
                        let area = ui::top_modal_rect(frame_area, modal_w.max(56), h);
                        hw_cursor = ui::render_time_panel(app, frame, area);
                    } else if app.help_open {
                        let content_rows = crate::help::help_body_lines(app).len().max(1);
                        let h = ui::help_modal_height(frame_area, content_rows);
                        let area = ui::top_modal_rect(frame_area, modal_w.max(56), h);
                        ui::render_help_panel(app, frame, area);
                    } else if app.detail_open() {
                        let inner_w = modal_w.saturating_sub(2).max(1);
                        let content_rows = ui::detail_content_lines(app, inner_w).len().max(1);
                        let h = ui::detail_modal_height(frame_area, content_rows);
                        let area = ui::top_modal_rect(frame_area, modal_w, h);
                        ui::render_detail(app, frame, area);
                    } else if app.summary_open() {
                        let content_rows = ui::summary_content_row_count(app);
                        let h = ui::summary_modal_height(frame_area, content_rows);
                        let area = ui::top_modal_rect(frame_area, modal_w.max(56), h);
                        ui::render_summary_panel(app, frame, area);
                    }
                    // Preset name dialog paints above picker (rename) or alone (save).
                    if let Some(dialog) = app.preset_name.as_ref() {
                        hw_cursor = ui::render_preset_name_dialog(dialog, frame, frame_area);
                    }
                    if let Some(pos) = hw_cursor {
                        frame.set_cursor_position(pos);
                    }
                })
                .map_err(|e| e.to_string())?;
        } // end if should_draw

        // Poll for user events. When we skipped the draw (ingest active, drew
        // recently) use a short timeout so we return quickly for the next draw.
        let poll_ms = if should_draw {
            100u64 // normal: wait up to 100ms for the next event
        } else {
            MIN_INGEST_DRAW_MS
                .saturating_sub(last_draw.elapsed().as_millis() as u64)
                .max(1)
        };
        if !event::poll(Duration::from_millis(poll_ms)).map_err(|e| e.to_string())? {
            continue;
        }
        // Got an event: always force a draw on the next iteration.
        force_draw = true;
        let read_event = event::read().map_err(|e| e.to_string())?;
        let key = match read_event {
            Event::Key(key) => key,
            // Mouse wheel always scrolls the log list, independent of `app.focus` —
            // see `handle_mouse_event`'s doc comment for why (no click-to-focus yet).
            Event::Mouse(mouse) => {
                handle_mouse_event(app, mouse);
                continue;
            }
            _ => continue,
        };

        // Name dialog / Picker / Search-box / time panel before Ctrl+C / Normal/Insert
        // so Ctrl+C cancels the draft like Esc, instead of quitting in Normal.
        if app.preset_name.is_some() {
            handle_preset_name_key(app, key);
            continue;
        }
        if app.open_file_panel.is_some() {
            handle_open_file_panel_key(app, live, key);
            continue;
        }
        if app.stream_source_panel.is_some() {
            handle_stream_source_panel_key(app, live, key);
            continue;
        }
        if app.dashboard.is_some() {
            handle_dashboard_key(app, live, key);
            continue;
        }
        if app.picker.is_some() {
            handle_picker_key(app, key);
            continue;
        }
        if app.time_panel.is_some() {
            handle_time_panel_key(app, key);
            continue;
        }
        if app.highlight_box.editing {
            handle_highlight_box_key(app, key);
            continue;
        }
        if app.help_open {
            handle_help_key(app, key);
            continue;
        }
        if app.summary_open() {
            handle_summary_key(app, key);
            continue;
        }
        // Ctrl+C: quit from Normal (like `q`), but only cancel in-progress
        // input from Insert (like Esc) — mirrors the shell/readline
        // "abort current line" convention instead of nuking the session.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
            handle_ctrl_c(app, input);
            continue;
        }

        // Page / clear-live chords (may include Ctrl) — resolved via keymap.
        // Intercepted here rather than threading KeyEvent into `handle_normal_key`.
        if app.mode == Mode::Normal && app.focus == app::Focus::LogList {
            if km_event(app, keymap::ActionId::LogListPageDown, key) {
                app.move_cursor_manual(PAGE_SIZE);
                continue;
            }
            if km_event(app, keymap::ActionId::LogListPageUp, key) {
                app.move_cursor_manual(-PAGE_SIZE);
                continue;
            }
            if km_event(app, keymap::ActionId::LogListClearLive, key) {
                try_handle_ctrl_l(app);
                continue;
            }
        }

        match app.mode {
            Mode::Normal => handle_normal_key(app, input, key.code),
            Mode::Insert => handle_insert_key(app, input, key.code)?,
        }
    }
    Ok(())
}

struct PickerRenderData {
    title: String,
    mode: crate::picker::PickerMode,
    text: String,
    /// Char-index caret into `text` for the search prompt.
    caret: usize,
    /// Query used to color substring matches in the candidate list.
    match_query: String,
    chips: Vec<input::Chip>,
    exclude_chips: bool,
    /// Confirmed field awaiting a value (`tag:` prefix); Filter/Exclude New·Edit only.
    draft_field: Option<input::ChipField>,
    labels: Vec<String>,
    styles: Vec<ratatui::style::Style>,
    checked: Vec<bool>,
    actions: Vec<crate::ui::ActionKind>,
    selected: usize,
    empty_msg: String,
    preview: Vec<preview::PreviewHit>,
    /// Whether the right preview pane should be rendered (layout-level toggle).
    show_preview: bool,
    /// Bookmark panel: right pane shows Fields detail for this row (`None` = stale).
    /// When set, overrides [`Self::preview`] hits rendering.
    detail_row: Option<Option<crate::model::EntryRow>>,
    /// Preset panel: chip-strip Preview lines (overrides hits when set).
    preset_preview: Option<Vec<ratatui::text::Line<'static>>>,
    /// Search-line leading nerdfont; `None` derives from [`PickerMode`].
    prompt_icon: Option<&'static str>,
}

fn picker_render_data(
    app: &App,
    preview_limit: usize,
    preview_inner_width: u16,
) -> Option<PickerRenderData> {
    use crate::picker::{PickerKind, PickerMode, PickerSession, UnifiedKind};

    let session = app.picker.as_ref()?;
    let (title, mode, text, match_query, caret) = match &session.kind {
        PickerKind::ActionList { .. } => (
            "Create".to_string(),
            session.mode.clone(),
            session.query.to_string(),
            session.query.to_string(),
            session.query.cursor(),
        ),
        _ => {
            let base_title = match &session.kind {
                PickerKind::Unified => "Manage",
                PickerKind::Filter => "Filter",
                PickerKind::Highlight => "Highlight",
                PickerKind::Bookmark => "Bookmark",
                PickerKind::Exclude => "Exclude",
                PickerKind::Preset => "Preset",
                PickerKind::MsgChip { .. } => "Message",
                PickerKind::ActionList { .. } => unreachable!(),
            };
            let mode_name = match session.mode {
                PickerMode::Manage => "Search",
                PickerMode::New => "New",
                PickerMode::Edit { .. } => "Edit",
            };
            let title = format!("{base_title} · {mode_name}");
            let mode = session.mode.clone();
            let text = match session.mode {
                PickerMode::Manage => session.query.to_string(),
                PickerMode::New | PickerMode::Edit { .. } => match &session.kind {
                    PickerKind::Filter | PickerKind::Exclude => session
                        .input
                        .as_ref()
                        .map(|input| input.draft.to_string())
                        .unwrap_or_default(),
                    _ => session.draft.to_string(),
                },
            };
            let match_query = text.clone();
            let caret = match session.mode {
                PickerMode::Manage => session.query.cursor(),
                PickerMode::New | PickerMode::Edit { .. } => match &session.kind {
                    PickerKind::Filter | PickerKind::Exclude => session
                        .input
                        .as_ref()
                        .map(|input| input.draft.cursor())
                        .unwrap_or(0),
                    _ => session.draft.cursor(),
                },
            };
            (title, mode, text, match_query, caret)
        }
    };
    let mut show_preview = app.config.picker_preview_enabled
        && !matches!(
            session.kind,
            PickerKind::Unified | PickerKind::ActionList { .. }
        );
    let mut selected = session.selected;
    let mut chips = Vec::new();
    let mut exclude_chips = false;
    let mut draft_field = None;
    let mut labels = Vec::new();
    let mut styles = Vec::new();
    let mut checked = Vec::new();
    let mut actions = Vec::new();
    let mut preview_lines = Vec::new();
    let mut detail_row: Option<Option<crate::model::EntryRow>> = None;
    let mut empty_msg = "无项目".to_string();
    let mut preset_preview: Option<Vec<ratatui::text::Line<'static>>> = None;

    match session.mode {
        PickerMode::Manage => match &session.kind {
            PickerKind::ActionList { .. } => {
                // Substring filter only (not fuzzy) — two fixed choices.
                let visible =
                    PickerSession::contains_indices(&session.choices, session.query.as_str());
                labels = visible
                    .iter()
                    .map(|&i| session.choices[i].clone())
                    .collect();
                styles = vec![theme::muted(); labels.len()];
                checked = vec![false; labels.len()];
                actions = vec![crate::ui::ActionKind::None; labels.len()];
                empty_msg = "选择创建方式".to_string();
                show_preview = false;
            }
            PickerKind::Bookmark => {
                // Bookmark-only panel: no Tab multi-select; Jump icons;
                // right pane shows Fields detail for the selected bookmark.
                use crate::bookmark::bookmark_list_label;
                let vis = bookmark_visible_indices(app);
                labels = vis
                    .iter()
                    .map(|&i| bookmark_list_label(&app.bookmarks.items[i].label).to_string())
                    .collect();
                styles = vis.iter().map(|_| theme::bookmark_label_style()).collect();
                checked = vec![false; vis.len()];
                actions = vec![crate::ui::ActionKind::Jump; vis.len()];
                empty_msg = "无书签".to_string();
                if show_preview {
                    detail_row = Some(
                        vis.get(session.selected)
                            .and_then(|&i| app.row_by_id(app.bookmarks.items[i].row_id)),
                    );
                }
                let _ = selected; // bookmark panel reuses session.selected as-is
            }
            PickerKind::Preset => {
                let vis = preset_visible_indices(app);
                labels = vis
                    .iter()
                    .map(|&i| app.preset_catalog[i].name.clone())
                    .collect();
                styles = vec![theme::muted(); labels.len()];
                checked = vec![false; labels.len()];
                actions = vec![crate::ui::ActionKind::Jump; labels.len()];
                empty_msg = "无规则".to_string();
                if show_preview {
                    if let Some(&idx) = vis.get(session.selected) {
                        preset_preview = Some(ui::preset_preview_lines(
                            &app.preset_catalog[idx],
                            preview_inner_width,
                        ));
                    }
                }
            }
            _ => {
                // Unified panel: Filter/Highlight/Exclude only (bookmark arm removed).
                let all = unified_picker_items(app);
                let all_labels: Vec<String> = all.iter().map(|item| item.label.clone()).collect();
                let visible = PickerSession::filtered_indices(&all_labels, session.query.as_str());
                labels = visible
                    .iter()
                    .map(|&index| all[index].label.clone())
                    .collect();
                styles = visible
                    .iter()
                    .map(|&index| {
                        let item = &all[index];
                        if item.enabled {
                            theme::unified_kind_style(item.id.kind)
                        } else {
                            theme::disabled_chip_style()
                        }
                    })
                    .collect();
                checked = visible
                    .iter()
                    .map(|&index| session.checked.contains(&all[index].id))
                    .collect();
                actions = visible
                    .iter()
                    .map(|&index| crate::ui::ActionKind::Toggle {
                        enabled: all[index].enabled,
                    })
                    .collect();

                if show_preview {
                    if let Some(&src) = visible.get(session.selected) {
                        let item = &all[src];
                        match item.id.kind {
                            UnifiedKind::Highlight => {
                                preview_lines = preview::preview_highlight_pattern_lines(
                                    app,
                                    &app.highlight_groups.groups[item.id.source_index].pattern,
                                    preview_limit,
                                )
                                .unwrap_or_default();
                            }
                            UnifiedKind::Filter => {
                                let input = input::InputBox {
                                    chips: app.groups.groups[item.id.source_index].chips.clone(),
                                    ..input::InputBox::default()
                                };
                                preview_lines = app.preview_filter_throttled(&input, preview_limit);
                            }
                            UnifiedKind::Exclude => {
                                let input = input::InputBox {
                                    chips: vec![app.groups.excludes[item.id.source_index]
                                        .chip
                                        .clone()],
                                    exclude_mode: true,
                                    ..input::InputBox::default()
                                };
                                preview_lines = app.preview_filter_throttled(&input, preview_limit);
                            }
                        }
                    }
                }
            }
        },
        PickerMode::New | PickerMode::Edit { .. } => match &session.kind {
            PickerKind::Highlight => {
                if !session.draft.is_empty() {
                    labels = app.vocab_match.display_labels().to_vec();
                    styles = vec![theme::muted(); labels.len()];
                } else {
                    let highlight_box = crate::highlight_model::HighlightBox {
                        draft: session.draft.clone(),
                        editing: true,
                        selected: session.selected,
                    };
                    let indices = highlight_box.candidate_indices(&app.highlight_groups.groups);
                    labels = indices
                        .iter()
                        .map(|&index| app.highlight_groups.groups[index].pattern.clone())
                        .collect();
                    styles = vec![theme::muted(); labels.len()];
                }
                preview_lines = preview::preview_highlight_pattern_lines(
                    app,
                    session.draft.as_str(),
                    preview_limit,
                )
                .unwrap_or_default();
                empty_msg = "输入高亮词".to_string();
            }
            PickerKind::Filter | PickerKind::Exclude => {
                if let Some(input) = session.input.as_ref() {
                    chips = input.chips.clone();
                    exclude_chips = input.exclude_mode;
                    draft_field = input.draft_field;
                    match input.draft_field {
                        None => {
                            let fields = input.field_candidates();
                            labels = fields.iter().map(|f| f.keyword().to_string()).collect();
                            styles = fields
                                .iter()
                                .map(|f| {
                                    ratatui::style::Style::default().fg(theme::field_color(*f))
                                })
                                .collect();
                            selected = input.field_selected;
                        }
                        Some(field) => {
                            use crate::input::ChipField;
                            labels = match field {
                                ChipField::Tag | ChipField::Pkg | ChipField::Msg => {
                                    app.vocab_match.display_labels().to_vec()
                                }
                                ChipField::Level => level_field_candidates(input.draft.as_str()),
                                ChipField::Pid | ChipField::Tid => vec![],
                            };
                            styles = vec![
                                ratatui::style::Style::default()
                                    .fg(theme::field_color(field));
                                labels.len()
                            ];
                        }
                    }
                    if input.draft_field.is_some() && !input.draft.is_empty() {
                        preview_lines = app.preview_filter_throttled(input, preview_limit);
                    }
                }
                empty_msg = "Enter 收 pill / 提交".to_string();
            }
            PickerKind::Bookmark | PickerKind::Preset => {}
            PickerKind::MsgChip { .. } => {
                let visible =
                    PickerSession::filtered_indices(&session.choices, session.draft.as_str());
                labels = visible
                    .iter()
                    .map(|&index| session.choices[index].clone())
                    .collect();
                styles = vec![
                    ratatui::style::Style::default()
                        .fg(theme::field_color(crate::input::ChipField::Msg));
                    labels.len()
                ];
                empty_msg = "输入消息片段".to_string();
            }
            PickerKind::Unified | PickerKind::ActionList { .. } => {}
        },
    }

    let prompt_icon = match &session.kind {
        PickerKind::Bookmark => Some(theme::GLYPH_BOOKMARK),
        PickerKind::Preset => Some(theme::GLYPH_GROUP_ON),
        _ => None,
    };

    Some(PickerRenderData {
        title,
        mode,
        text,
        caret,
        match_query,
        chips,
        exclude_chips,
        draft_field,
        labels,
        styles,
        checked,
        actions,
        selected,
        empty_msg,
        preview: preview_lines,
        show_preview,
        detail_row,
        preset_preview,
        prompt_icon,
    })
}

/// Visible indices into `app.preset_catalog` for Preset Manage (fuzzy by name).
fn preset_visible_indices(app: &App) -> Vec<usize> {
    use crate::picker::PickerSession;
    let session = match app.picker.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let labels: Vec<String> = app.preset_catalog.iter().map(|p| p.name.clone()).collect();
    PickerSession::filtered_indices(&labels, session.query.as_str())
}
/// Aggregate Filter → Highlight → Exclude for Manage (bookmark segment removed, F2).
fn unified_picker_items(app: &App) -> Vec<crate::picker::UnifiedItem> {
    use crate::picker::{UnifiedId, UnifiedItem, UnifiedKind};

    let mut items = Vec::new();
    for (source_index, group) in app.groups.groups.iter().enumerate() {
        items.push(UnifiedItem {
            id: UnifiedId {
                kind: UnifiedKind::Filter,
                source_index,
            },
            label: format!("[{}]: {}", UnifiedKind::Filter.tag(), group.label),
            enabled: group.enabled,
        });
    }
    for (source_index, group) in app.highlight_groups.groups.iter().enumerate() {
        items.push(UnifiedItem {
            id: UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index,
            },
            label: format!("[{}]: {}", UnifiedKind::Highlight.tag(), group.pattern),
            enabled: group.enabled,
        });
    }
    for (source_index, entry) in app.groups.excludes.iter().enumerate() {
        let body = format!("{}:{}", entry.chip.field.keyword(), entry.chip.value);
        items.push(UnifiedItem {
            id: UnifiedId {
                kind: UnifiedKind::Exclude,
                source_index,
            },
            label: format!("[{}]: {body}", UnifiedKind::Exclude.tag()),
            enabled: entry.enabled,
        });
    }
    items
}

fn unified_visible_ids(app: &App) -> Vec<crate::picker::UnifiedId> {
    use crate::picker::PickerSession;

    let session = match app.picker.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };
    // Bookmark / Preset panels key off their own helpers; never enter unified path.
    if matches!(
        session.kind,
        crate::picker::PickerKind::Bookmark | crate::picker::PickerKind::Preset
    ) {
        return Vec::new();
    }
    let all = unified_picker_items(app);
    let labels: Vec<String> = all.iter().map(|item| item.label.clone()).collect();
    let visible = PickerSession::filtered_indices(&labels, session.query.as_str());
    visible.iter().map(|&i| all[i].id).collect()
}

fn unified_selected_id(app: &App) -> Option<crate::picker::UnifiedId> {
    let session = app.picker.as_ref()?;
    if matches!(
        session.kind,
        crate::picker::PickerKind::Bookmark | crate::picker::PickerKind::Preset
    ) {
        return None;
    }
    let ids = unified_visible_ids(app);
    ids.get(session.selected).copied()
}

/// Visible indices into `app.bookmarks.items` (newest-first display order)
/// for the bookmark Manage panel, filtered by the session query (F2).
fn bookmark_visible_indices(app: &App) -> Vec<usize> {
    use crate::bookmark::bookmark_list_label;
    use crate::picker::PickerSession;
    let session = match app.picker.as_ref() {
        Some(s) => s,
        None => return Vec::new(),
    };
    let len = app.bookmarks.items.len();
    // Newest-first display: rev the labels, filter, then map back to real index.
    // Search uses the same single-line label shown in the candidate list.
    let labels: Vec<String> = app
        .bookmarks
        .items
        .iter()
        .rev()
        .map(|b| bookmark_list_label(&b.label).to_string())
        .collect();
    let vis = PickerSession::filtered_indices(&labels, session.query.as_str());
    vis.iter().map(|&i| len - 1 - i).collect()
}

/// The real `app.bookmarks.items` index currently selected in the bookmark panel.
#[allow(dead_code)]
fn bookmark_selected_index(app: &App) -> Option<usize> {
    let vis = bookmark_visible_indices(app);
    let session = app.picker.as_ref()?;
    vis.get(session.selected).copied()
}

fn confirm_picker_delete(app: &mut App) {
    use crate::picker::ConfirmKind;

    let Some(confirm) = app
        .picker
        .as_ref()
        .and_then(|session| session.confirm.clone())
    else {
        return;
    };
    match confirm {
        ConfirmKind::DeleteMany { mut items } => {
            // Delete high indices first within each kind so earlier indices stay valid.
            items.sort_by(|a, b| {
                a.kind
                    .tag()
                    .cmp(b.kind.tag())
                    .then(b.source_index.cmp(&a.source_index))
            });
            for id in &items {
                app.delete_unified_at(id.kind, id.source_index);
            }
            let count = unified_picker_items(app).len();
            if let Some(session) = app.picker.as_mut() {
                session.cancel_confirm();
                session.checked.clear();
                session.selected = session.selected.min(count.saturating_sub(1));
            }
        }
        ConfirmKind::DeleteBookmark { index } => {
            app.delete_bookmark_at_index(index);
            let count = bookmark_visible_indices(app).len();
            if let Some(session) = app.picker.as_mut() {
                session.cancel_confirm();
                session.selected = session.selected.min(count.saturating_sub(1));
            }
        }
        ConfirmKind::DeletePreset { name } => {
            app.delete_preset_named(&name);
            let count = preset_visible_indices(app).len();
            if count == 0 {
                app.close_picker();
            } else if let Some(session) = app.picker.as_mut() {
                session.cancel_confirm();
                session.selected = session.selected.min(count.saturating_sub(1));
            }
        }
    }
}

fn replace_last_token(draft: &str, replacement: &str) -> String {
    match draft.rfind(' ') {
        Some(pos) => format!("{} {}", &draft[..pos], replacement),
        None => replacement.to_string(),
    }
}

fn submit_highlight_picker(app: &mut App) {
    use crate::picker::PickerMode;

    let Some((mode, draft, candidate_selected)) = app.picker.as_ref().map(|session| {
        (
            session.mode.clone(),
            session.draft.clone(),
            session.selected,
        )
    }) else {
        return;
    };
    let mut highlight_box = crate::highlight_model::HighlightBox {
        draft,
        editing: true,
        selected: candidate_selected,
    };
    let Ok(Some(group)) = highlight_box.confirm_or_submit(&app.highlight_groups.groups) else {
        return;
    };
    let selected = match mode {
        PickerMode::New => app.push_or_find_highlight_group(group),
        PickerMode::Edit { index } => {
            if !app.update_search_group(index, &group.pattern) {
                app.set_flash("EXISTS");
                return;
            }
            app.active_highlight = Some(index);
            index
        }
        PickerMode::Manage => return,
    };
    app.active_highlight = Some(selected);
    app.jump_first_match_of(selected);
    app.close_picker();
}

fn submit_filter_picker(app: &mut App) {
    use crate::picker::{PickerKind, PickerMode};

    let Some((kind, mode)) = app
        .picker
        .as_ref()
        .map(|session| (session.kind.clone(), session.mode.clone()))
    else {
        return;
    };

    {
        let Some(input) = app
            .picker
            .as_mut()
            .and_then(|session| session.input.as_mut())
        else {
            return;
        };
        if input.confirm_field_candidate_on_enter() {
            return;
        }
        if input.has_pending_draft() {
            input.commit_draft_as_chip();
            return;
        }
        if input.is_empty() {
            return;
        }
    }
    if kind == PickerKind::Exclude
        && app
            .picker
            .as_ref()
            .and_then(|session| session.input.as_ref())
            .is_some_and(|input| input.chips.len() != 1)
    {
        app.set_flash("EXCLUDE NEEDS ONE CHIP");
        return;
    }

    let selected = match (&kind, mode) {
        (PickerKind::Filter, PickerMode::New) => {
            let group = {
                let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
                match input.build_group(true) {
                    Ok(Some(group)) => group,
                    Ok(None) => return,
                    Err(error) => {
                        app.set_flash(error);
                        return;
                    }
                }
            };
            if let Some(index) = app.groups.groups.iter().position(|g| g.same_as(&group)) {
                index
            } else {
                app.push_filter_group(group);
                app.rebuild_visible();
                app.groups.groups.len() - 1
            }
        }
        (PickerKind::Filter, PickerMode::Edit { index }) => {
            let group = {
                let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
                match input.build_group(true) {
                    Ok(Some(group)) => group,
                    Ok(None) => return,
                    Err(error) => {
                        app.set_flash(error);
                        return;
                    }
                }
            };
            if !app.update_filter_group(index, group) {
                app.set_flash("EXISTS");
                return;
            }
            index
        }
        (PickerKind::Exclude, PickerMode::New) => {
            let chips = {
                let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
                std::mem::take(&mut input.chips)
            };
            let before = app.groups.excludes.len();
            for chip in chips {
                app.push_exclude_chip(chip);
            }
            if app.groups.excludes.is_empty() {
                return;
            }
            if app.groups.excludes.len() == before {
                before.saturating_sub(1)
            } else {
                app.groups.excludes.len() - 1
            }
        }
        (PickerKind::Exclude, PickerMode::Edit { index }) => {
            let group = {
                let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
                match input.build_group(true) {
                    Ok(Some(group)) => group,
                    Ok(None) => return,
                    Err(error) => {
                        app.set_flash(error);
                        return;
                    }
                }
            };
            if !app.update_exclude_group(index, group) {
                app.set_flash("EXISTS");
                return;
            }
            index
        }
        _ => return,
    };
    match &kind {
        PickerKind::Filter => app.group_cursor = selected,
        PickerKind::Exclude => app.exclude_cursor = selected,
        _ => {}
    }
    app.close_picker();
}

/// Shared readline-subset keys for a [`text_field::TextField`].
/// Does not handle Ctrl-Backspace or Delete (mode-specific).
fn apply_text_field_key(
    field: &mut crate::text_field::TextField,
    code: KeyCode,
    ctrl: bool,
) -> bool {
    match code {
        KeyCode::Left => {
            field.move_left();
            true
        }
        KeyCode::Right => {
            field.move_right();
            true
        }
        KeyCode::Home => {
            field.home();
            true
        }
        KeyCode::End => {
            field.end();
            true
        }
        KeyCode::Char('a') if ctrl => {
            field.home();
            true
        }
        KeyCode::Char('e') if ctrl => {
            field.end();
            true
        }
        KeyCode::Char('u') if ctrl => {
            field.kill_to_start();
            true
        }
        KeyCode::Backspace if !ctrl => {
            let _ = field.backspace();
            true
        }
        KeyCode::Char(c) if !ctrl => {
            field.insert(c);
            true
        }
        _ => false,
    }
}

fn manage_request_delete_selected(app: &mut App) {
    let session = app.picker.as_ref().unwrap();
    let items: Vec<crate::picker::UnifiedId> = if session.checked.is_empty() {
        unified_selected_id(app).into_iter().collect()
    } else {
        session.checked.iter().copied().collect()
    };
    if !items.is_empty() {
        app.picker.as_mut().unwrap().request_delete_many(items);
    }
}

fn manage_enter_edit_selected(app: &mut App) {
    if app.picker.as_ref().is_some_and(|s| s.checked.len() > 1) {
        app.set_flash("NO BULK EDIT");
        return;
    }
    let Some(id) = (|| {
        let session = app.picker.as_ref()?;
        if session.checked.len() == 1 {
            session.checked.iter().next().copied()
        } else {
            unified_selected_id(app)
        }
    })() else {
        return;
    };
    use crate::picker::UnifiedKind;
    match id.kind {
        UnifiedKind::Highlight => {
            let pattern = app.highlight_groups.groups[id.source_index].pattern.clone();
            let session = app.picker.as_mut().unwrap();
            session.kind = id.kind.as_picker_kind();
            session.enter_edit(id.source_index, pattern);
        }
        UnifiedKind::Filter => {
            let input = crate::input::InputBox {
                chips: app.groups.groups[id.source_index].chips.clone(),
                ..crate::input::InputBox::default()
            };
            let session = app.picker.as_mut().unwrap();
            session.kind = id.kind.as_picker_kind();
            session.enter_edit_input(id.source_index, input);
        }
        UnifiedKind::Exclude => {
            let input = crate::input::InputBox {
                chips: vec![app.groups.excludes[id.source_index].chip.clone()],
                exclude_mode: true,
                ..crate::input::InputBox::default()
            };
            let session = app.picker.as_mut().unwrap();
            session.kind = id.kind.as_picker_kind();
            session.enter_edit_input(id.source_index, input);
        }
    }
}

fn handle_picker_key(app: &mut App, key: event::KeyEvent) {
    use crate::picker::{PickerKind, PickerMode, PickerSession};

    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if app
        .picker
        .as_ref()
        .is_some_and(|session| session.confirm.is_some())
    {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') => confirm_picker_delete(app),
            KeyCode::Esc | KeyCode::Char('n') => {
                if let Some(session) = app.picker.as_mut() {
                    session.cancel_confirm();
                }
            }
            _ => {}
        }
        return;
    }

    let is_ctrl_c = key.code == KeyCode::Char('c') && ctrl;
    if key.code == KeyCode::Esc || is_ctrl_c {
        // Any mode: close the picker (confirm dialog handled above).
        app.close_picker();
        return;
    }

    let Some((kind, mode)) = app
        .picker
        .as_ref()
        .map(|session| (session.kind.clone(), session.mode.clone()))
    else {
        return;
    };

    if matches!(mode, PickerMode::Manage) {
        match kind {
            PickerKind::ActionList { .. } => match key.code {
                KeyCode::Enter | KeyCode::Tab => {
                    let _ = app.confirm_msg_action_list();
                }
                KeyCode::Up => {
                    let selected = app.picker.as_ref().unwrap().selected.saturating_sub(1);
                    app.picker.as_mut().unwrap().selected = selected;
                }
                KeyCode::Down => {
                    let count = {
                        let session = app.picker.as_ref().unwrap();
                        PickerSession::contains_indices(&session.choices, session.query.as_str())
                            .len()
                    };
                    let session = app.picker.as_mut().unwrap();
                    session.selected = (session.selected + 1).min(count.saturating_sub(1));
                }
                code => {
                    let session = app.picker.as_mut().unwrap();
                    if apply_text_field_key(&mut session.query, code, ctrl) {
                        session.selected = 0;
                    }
                }
            },
            PickerKind::Bookmark => {
                // Bookmark panel (F2): Tab no-op, Ctrl-X flash, Delete/Ctrl-Backspace delete,
                // Enter jump-to-row + close + focus LogList.
                let vis = bookmark_visible_indices(app);
                let is_delete = matches!(key.code, KeyCode::Delete)
                    || (matches!(key.code, KeyCode::Backspace) && ctrl);
                match key.code {
                    KeyCode::Up => {
                        app.picker.as_mut().unwrap().selected =
                            app.picker.as_ref().unwrap().selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let session = app.picker.as_mut().unwrap();
                        session.selected = (session.selected + 1).min(vis.len().saturating_sub(1));
                    }
                    KeyCode::Tab => { /* no-op: bookmark panel has no multi-select */ }
                    KeyCode::Char('x') if ctrl => {
                        app.set_flash("BOOKMARKS NOT EDITABLE");
                    }
                    _ if is_delete => {
                        if let Some(&idx) = vis.get(app.picker.as_ref().unwrap().selected) {
                            app.picker.as_mut().unwrap().confirm =
                                Some(crate::picker::ConfirmKind::DeleteBookmark { index: idx });
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(&idx) = vis.get(app.picker.as_ref().unwrap().selected) {
                            let row_id = app.bookmarks.items[idx].row_id;
                            match app.jump_to_bookmark(row_id) {
                                crate::bookmark::JumpResult::Ok => {
                                    app.close_picker();
                                }
                                crate::bookmark::JumpResult::Evicted => {
                                    app.set_flash("BOOKMARK EVICTED");
                                }
                                crate::bookmark::JumpResult::Filtered => {
                                    app.set_flash("BOOKMARK NOT VISIBLE");
                                }
                            }
                        }
                    }
                    code => {
                        let session = app.picker.as_mut().unwrap();
                        if apply_text_field_key(&mut session.query, code, ctrl) {
                            session.selected = 0;
                        }
                    }
                }
                return;
            }
            PickerKind::Preset => {
                let vis = preset_visible_indices(app);
                let is_delete = matches!(key.code, KeyCode::Delete)
                    || (matches!(key.code, KeyCode::Backspace) && ctrl);
                match key.code {
                    KeyCode::Up => {
                        app.picker.as_mut().unwrap().selected =
                            app.picker.as_ref().unwrap().selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let session = app.picker.as_mut().unwrap();
                        session.selected = (session.selected + 1).min(vis.len().saturating_sub(1));
                    }
                    KeyCode::Tab => {}
                    KeyCode::Char('x') if ctrl => {
                        if let Some(&idx) = vis.get(app.picker.as_ref().unwrap().selected) {
                            let name = app.preset_catalog[idx].name.clone();
                            app.preset_name = Some(crate::preset::PresetNameDialog::rename(&name));
                        }
                    }
                    _ if is_delete => {
                        if let Some(&idx) = vis.get(app.picker.as_ref().unwrap().selected) {
                            let name = app.preset_catalog[idx].name.clone();
                            app.picker.as_mut().unwrap().confirm =
                                Some(crate::picker::ConfirmKind::DeletePreset { name });
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(&idx) = vis.get(app.picker.as_ref().unwrap().selected) {
                            let preset = app.preset_catalog[idx].clone();
                            match app.apply_preset(&preset) {
                                Ok(()) => {
                                    app.close_picker();
                                    app.set_flash("PRESET APPLIED");
                                }
                                Err(e) => app.set_flash(&e),
                            }
                        }
                    }
                    code => {
                        let session = app.picker.as_mut().unwrap();
                        if apply_text_field_key(&mut session.query, code, ctrl) {
                            session.selected = 0;
                        }
                    }
                }
                return;
            }
            _ => {
                // Unified panel (Filter/Highlight/Exclude).
                let ids = unified_visible_ids(app);
                let is_delete = matches!(key.code, KeyCode::Delete)
                    || (matches!(key.code, KeyCode::Backspace) && ctrl);
                match key.code {
                    KeyCode::Up => {
                        app.picker.as_mut().unwrap().selected =
                            app.picker.as_ref().unwrap().selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        let session = app.picker.as_mut().unwrap();
                        session.selected = (session.selected + 1).min(ids.len().saturating_sub(1));
                    }
                    KeyCode::Tab => {
                        let Some(id) = unified_selected_id(app) else {
                            return;
                        };
                        let session = app.picker.as_mut().unwrap();
                        if session.checked.contains(&id) {
                            session.checked.remove(&id);
                        } else {
                            session.checked.insert(id);
                        }
                        let count = ids.len();
                        session.selected = (session.selected + 1).min(count.saturating_sub(1));
                    }
                    KeyCode::Char('x') if ctrl => manage_enter_edit_selected(app),
                    _ if is_delete => manage_request_delete_selected(app),
                    KeyCode::Enter => {
                        let session = app.picker.as_ref().unwrap();
                        if !session.checked.is_empty() {
                            let items: Vec<_> = session.checked.iter().copied().collect();
                            for id in items {
                                app.toggle_unified_enabled(id.kind, id.source_index);
                            }
                            return;
                        }
                        let Some(id) = unified_selected_id(app) else {
                            return;
                        };
                        app.toggle_unified_enabled(id.kind, id.source_index);
                    }
                    code => {
                        let session = app.picker.as_mut().unwrap();
                        if apply_text_field_key(&mut session.query, code, ctrl) {
                            session.selected = 0;
                        }
                    }
                }
            }
        }
        return;
    }

    // New / Edit modes
    match kind {
        PickerKind::Highlight => match key.code {
            KeyCode::Enter => submit_highlight_picker(app),
            KeyCode::Up => {
                let selected = app.picker.as_ref().unwrap().selected.saturating_sub(1);
                app.picker.as_mut().unwrap().selected = selected;
            }
            KeyCode::Down => {
                let (draft, sel) = {
                    let s = app.picker.as_ref().unwrap();
                    (s.draft.to_string(), s.selected)
                };
                if !draft.is_empty() {
                    let n = app.vocab_match.display_labels().len();
                    app.picker.as_mut().unwrap().selected = (sel + 1).min(n.saturating_sub(1));
                } else {
                    let mut highlight_box = crate::highlight_model::HighlightBox {
                        draft: crate::text_field::TextField::from_text(draft),
                        editing: true,
                        selected: sel,
                    };
                    highlight_box.move_selection(&app.highlight_groups.groups, 1);
                    app.picker.as_mut().unwrap().selected = highlight_box.selected;
                }
            }
            KeyCode::Tab => {
                let (draft, selected) = {
                    let session = app.picker.as_ref().unwrap();
                    (session.draft.to_string(), session.selected)
                };
                if !draft.is_empty() {
                    let replacement = app.vocab_match.display_labels().get(selected).cloned();
                    if let Some(replacement) = replacement {
                        let new_draft = replace_last_token(&draft, &replacement);
                        let session = app.picker.as_mut().unwrap();
                        session.draft = crate::text_field::TextField::from_text(new_draft);
                        session.selected = 0;
                    }
                }
            }
            KeyCode::Backspace if ctrl => {
                let session = app.picker.as_mut().unwrap();
                session.draft.kill_word_back();
                session.selected = 0;
            }
            KeyCode::Delete => {}
            code => {
                let session = app.picker.as_mut().unwrap();
                if apply_text_field_key(&mut session.draft, code, ctrl) {
                    session.selected = 0;
                }
            }
        },
        PickerKind::Filter | PickerKind::Exclude => match key.code {
            KeyCode::Enter => submit_filter_picker(app),
            KeyCode::Tab => {
                let has_field = app
                    .picker
                    .as_ref()
                    .unwrap()
                    .input
                    .as_ref()
                    .map(|i| i.draft_field.is_some())
                    .unwrap_or(false);

                if has_field {
                    let selected = app.picker.as_ref().unwrap().selected;
                    let labels = {
                        let session = app.picker.as_ref().unwrap();
                        let input = session.input.as_ref().unwrap();
                        use crate::input::ChipField;
                        match input.draft_field.unwrap() {
                            ChipField::Tag | ChipField::Pkg | ChipField::Msg => {
                                app.vocab_match.display_labels().to_vec()
                            }
                            ChipField::Level => level_field_candidates(input.draft.as_str()),
                            ChipField::Pid | ChipField::Tid => vec![],
                        }
                    };
                    if let Some(value) = labels.into_iter().nth(selected) {
                        if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                            input.draft = crate::text_field::TextField::from_text(value);
                        }
                        app.picker.as_mut().unwrap().selected = 0;
                    }
                } else {
                    let confirmed = app
                        .picker
                        .as_mut()
                        .unwrap()
                        .input
                        .as_mut()
                        .map(|i| i.confirm_field_candidate())
                        .unwrap_or(false);
                    if confirmed {
                        app.picker.as_mut().unwrap().selected = 0;
                    }
                }
            }
            KeyCode::Up => {
                let has_field = app
                    .picker
                    .as_ref()
                    .unwrap()
                    .input
                    .as_ref()
                    .map(|i| i.draft_field.is_some())
                    .unwrap_or(false);
                if has_field {
                    let sel = app.picker.as_ref().unwrap().selected;
                    app.picker.as_mut().unwrap().selected = sel.saturating_sub(1);
                } else if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.move_field_selection(-1);
                }
            }
            KeyCode::Down => {
                let has_field = app
                    .picker
                    .as_ref()
                    .unwrap()
                    .input
                    .as_ref()
                    .map(|i| i.draft_field.is_some())
                    .unwrap_or(false);
                if has_field {
                    let labels_len = {
                        let session = app.picker.as_ref().unwrap();
                        let input = session.input.as_ref().unwrap();
                        use crate::input::ChipField;
                        match input.draft_field.unwrap() {
                            ChipField::Tag | ChipField::Pkg | ChipField::Msg => {
                                app.vocab_match.display_labels().len()
                            }
                            ChipField::Level => level_field_candidates(input.draft.as_str()).len(),
                            ChipField::Pid | ChipField::Tid => 0,
                        }
                    };
                    let sel = app.picker.as_ref().unwrap().selected;
                    app.picker.as_mut().unwrap().selected =
                        (sel + 1).min(labels_len.saturating_sub(1));
                } else if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.move_field_selection(1);
                }
            }
            KeyCode::Backspace if ctrl => {
                if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.draft.kill_word_back();
                    input.field_selected = 0;
                }
            }
            KeyCode::Delete => {}
            KeyCode::Backspace => {
                if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.backspace();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                let exclude_has_chip = kind == PickerKind::Exclude
                    && app
                        .picker
                        .as_ref()
                        .and_then(|session| session.input.as_ref())
                        .is_some_and(|input| !input.chips.is_empty());
                if exclude_has_chip {
                    app.set_flash("EXCLUDE NEEDS ONE CHIP");
                } else if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.push_char(c);
                }
            }
            code => {
                if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    let _ = apply_text_field_key(&mut input.draft, code, ctrl);
                }
            }
        },
        PickerKind::MsgChip { .. } => match key.code {
            KeyCode::Enter | KeyCode::Tab => {
                if let Some(text) = app.confirm_msg_token_picker() {
                    apply_yank(app, text);
                }
            }
            KeyCode::Up => {
                let selected = app.picker.as_ref().unwrap().selected.saturating_sub(1);
                app.picker.as_mut().unwrap().selected = selected;
            }
            KeyCode::Down => {
                let session = app.picker.as_ref().unwrap();
                let count =
                    PickerSession::filtered_indices(&session.choices, session.draft.as_str()).len();
                app.picker.as_mut().unwrap().selected =
                    (session.selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Backspace if ctrl => {
                let session = app.picker.as_mut().unwrap();
                session.draft.kill_word_back();
                session.selected = 0;
            }
            KeyCode::Delete => {}
            code => {
                let session = app.picker.as_mut().unwrap();
                if apply_text_field_key(&mut session.draft, code, ctrl) {
                    session.selected = 0;
                }
            }
        },
        PickerKind::Unified | PickerKind::ActionList { .. } => {}
        PickerKind::Bookmark | PickerKind::Preset => {}
    }
}

fn handle_dashboard_key(app: &mut App, live: &mut Option<LiveIngestCtl>, key: event::KeyEvent) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
        app.should_quit = true;
        return;
    }
    let Some(dash) = app.dashboard.as_mut() else {
        return;
    };
    let action = dashboard::handle_key(dash, key.code);
    match action {
        Some(dashboard::DashboardAction::Quit) => app.should_quit = true,
        Some(dashboard::DashboardAction::BindHdc) => {
            if let Err(e) = bind_live_source(app, live, LiveBackend::Hdc, false) {
                app.set_flash(e);
            }
        }
        Some(dashboard::DashboardAction::BindAdb) => {
            if let Err(e) = bind_live_source(app, live, LiveBackend::Adb, false) {
                app.set_flash(e);
            }
        }
        Some(dashboard::DashboardAction::OpenFilePicker) => {
            app.open_file_source_panel(true);
        }
        Some(dashboard::DashboardAction::OpenRecent(path)) => {
            *live = None;
            if let Err(e) = bind_file_source(app, &path, false) {
                app.set_flash(e);
                // Stay on dashboard; drop missing recent entry.
                app.recent.remove(&path);
                let _ = app.recent.save(&app.config_dir);
                if let Some(d) = app.dashboard.as_mut() {
                    d.recent = app.recent.clone();
                    d.clamp_cursor();
                }
            }
        }
        None => {}
    }
}

fn handle_open_file_panel_key(
    app: &mut App,
    live: &mut Option<LiveIngestCtl>,
    key: event::KeyEvent,
) {
    use crossterm::event::KeyModifiers;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if (key.code == KeyCode::Char('c') && ctrl) || key.code == KeyCode::Esc {
        app.open_file_panel = None;
        return;
    }

    let switching = app.dashboard.is_none();
    let recent = app.recent.clone();
    let mut confirm_path: Option<String> = None;
    let mut flash: Option<&'static str> = None;
    {
        let Some(panel) = app.open_file_panel.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Enter => {
                if let Some(path) = panel
                    .choices
                    .get(panel.selected)
                    .and_then(|c| c.path_to_open())
                {
                    confirm_path = Some(path.display().to_string());
                } else {
                    let draft = panel.draft.as_str().trim();
                    if draft.is_empty() {
                        flash = Some("NO FILE");
                    } else {
                        confirm_path =
                            Some(path_complete::expand_user(draft).display().to_string());
                    }
                }
            }
            // Arrows only — `j`/`k` must type into the path draft.
            KeyCode::Up => panel.move_sel(-1),
            KeyCode::Down => panel.move_sel(1),
            KeyCode::Tab => panel.apply_tab_complete(&recent),
            code => {
                if apply_text_field_key(&mut panel.draft, code, ctrl) {
                    panel.refresh_choices(&recent);
                }
            }
        }
    }
    if let Some(msg) = flash {
        app.set_flash(msg);
    }
    if let Some(path) = confirm_path {
        *live = None;
        if let Err(e) = bind_file_source(app, &path, switching) {
            app.set_flash(e);
        }
    }
}

fn handle_stream_source_panel_key(
    app: &mut App,
    live: &mut Option<LiveIngestCtl>,
    key: event::KeyEvent,
) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL)
        || key.code == KeyCode::Esc
    {
        app.stream_source_panel = None;
        return;
    }
    let switching = app.dashboard.is_none();
    let bind = {
        let Some(panel) = app.stream_source_panel.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                panel.move_by(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                panel.move_by(-1);
                None
            }
            KeyCode::Char('h') => Some(LiveBackend::Hdc),
            KeyCode::Char('a') => Some(LiveBackend::Adb),
            KeyCode::Enter => {
                if panel.is_hdc() {
                    Some(LiveBackend::Hdc)
                } else {
                    Some(LiveBackend::Adb)
                }
            }
            _ => None,
        }
    };
    if let Some(backend) = bind {
        if let Err(e) = bind_live_source(app, live, backend, switching) {
            app.set_flash(e);
        }
    }
}

fn handle_ctrl_c(app: &mut App, input: &mut input::InputBox) {
    use app::Mode;

    if app.pending_m {
        app.cancel_bookmark_op();
        return;
    }
    if app.pending_chip || app.pending_exclude {
        app.cancel_chip_from_cursor();
        return;
    }
    if app.pending_lock {
        app.cancel_lock_pending();
        return;
    }
    if app.pending_time {
        app.cancel_time_pending();
        return;
    }
    if app.pending_open {
        app.cancel_open_pending();
        return;
    }

    match app.mode {
        Mode::Normal => app.should_quit = true,
        Mode::Insert => {
            // Field popup is draft-driven; cancel always resets Input like Esc.
            *input = input::InputBox::default();
            app.mode = Mode::Normal;
            focus_loglist(app);
        }
    }
}

/// Live-mode Ctrl-L: clear buffered logs when no detail overlay is open.
/// File mode and detail-open are silent no-ops. Caller must already gate on
/// Normal + LogList (and Picker / HighlightBox are handled earlier in the loop).
fn try_handle_ctrl_l(app: &mut App) {
    if !app.export_source.is_live() || app.detail_open() {
        return;
    }
    app.clear_buffered_logs();
}

/// Return keyboard focus to the log list without changing cursor / offset /
/// following. Used when leaving ChipStrip/HighlightStrip/Input/HighlightBox.
fn focus_loglist(app: &mut App) {
    app.focus = app::Focus::LogList;
}

/// Return focus to the log list and resume live follow (pin to bottom).
/// Reserved for Esc on LogList, Visual Esc, and successful filter-group submit.
fn focus_loglist_and_follow(app: &mut App) {
    app.focus = app::Focus::LogList;
    app.resume_following();
}

/// Keep the retired standalone Input path testable until its remaining model
/// and renderer coverage can be removed; production entry points use Picker.
#[cfg(test)]
fn focus_input_insert(app: &mut App) {
    app.focus = app::Focus::Input;
    app.mode = app::Mode::Insert;
}

/// Mouse wheel step size for the log list, independent of `PAGE_SIZE`
/// (Ctrl-d/Ctrl-u) and the `Shift+J`/`Shift+K` fast-move binding — a wheel
/// "notch" is a smaller, more frequent unit than either of those.
const MOUSE_SCROLL_STEP: isize = 3;

/// LogList / Help `J`/`K` step — keep in sync via `help::FAST_SCROLL_STEP`.
const FAST_SCROLL_STEP: isize = help::FAST_SCROLL_STEP;

/// Only scroll wheel events move the log list; clicks/drags/moves are
/// ignored in this pass (no click-to-focus yet — see the design doc's
/// "Mouse wheel scrolling" section for why that's out of scope here). The
/// wheel always targets the log list regardless of which region currently
/// has keyboard focus. While visual-line is active, scrolling extends the
/// selection the same way `j`/`k` do.
fn handle_mouse_event(app: &mut App, mouse: event::MouseEvent) {
    match mouse.kind {
        event::MouseEventKind::ScrollDown => app.move_cursor_manual(MOUSE_SCROLL_STEP),
        event::MouseEventKind::ScrollUp => app.move_cursor_manual(-MOUSE_SCROLL_STEP),
        _ => {}
    }
}

/// Independent of `handle_normal_key`/`handle_insert_key`/`handle_ctrl_c`:
/// dispatched before any of them while `app.highlight_box.editing`. Ctrl+C
/// here cancels the draft (like Esc), not quit. Invalid regex on Enter is
/// silently ignored so a typo can't end the session or drop existing groups.
/// Enter/Tab with history candidates mirrors Input field-popup confirm.
fn handle_highlight_box_key(app: &mut App, key: event::KeyEvent) {
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    let is_ctrl_c = key.code == KeyCode::Char('c') && ctrl;
    match key.code {
        KeyCode::Up => app
            .highlight_box
            .move_selection(&app.highlight_groups.groups, -1),
        KeyCode::Down => app
            .highlight_box
            .move_selection(&app.highlight_groups.groups, 1),
        KeyCode::Enter | KeyCode::Tab => {
            match app
                .highlight_box
                .confirm_or_submit(&app.highlight_groups.groups)
            {
                Ok(Some(group)) => {
                    let idx = app.push_or_find_highlight_group(group);
                    app.highlight_box.clear();
                    let _ = app.jump_first_match_of(idx);
                    app.focus = app::Focus::LogList;
                }
                Ok(None) => {
                    // empty Enter/Tab: no-op (stay editing)
                }
                Err(()) => {
                    // bad regex: exit editing, keep prior search groups
                    app.highlight_box.clear();
                    focus_loglist(app);
                }
            }
        }
        KeyCode::Esc => {
            app.highlight_box.clear();
            focus_loglist(app);
        }
        _ if is_ctrl_c => {
            app.highlight_box.clear();
            focus_loglist(app);
        }
        KeyCode::Backspace if ctrl => {
            app.highlight_box.draft.kill_word_back();
            app.highlight_box.selected = 0;
        }
        KeyCode::Delete => {}
        code => {
            if apply_text_field_key(&mut app.highlight_box.draft, code, ctrl) {
                app.highlight_box.selected = 0;
            }
        }
    }
}

fn handle_strip_d_chord(app: &mut App, kind: app::StripKind, code: KeyCode) -> bool {
    use app::StripKind;
    use keymap::ActionId;
    if !matches!(
        (kind, app.focus),
        (StripKind::Filter, app::Focus::ChipStrip)
            | (StripKind::Exclude, app::Focus::ExcludeStrip)
            | (StripKind::Highlight, app::Focus::HighlightStrip)
    ) {
        return false;
    }
    if app.pending_d {
        if km_code(app, ActionId::StripDDelete, code) {
            app.delete_focused_strip_group(kind);
            app.pending_d = false;
            return true;
        }
        if km_code(app, ActionId::StripDDisable, code) {
            app.toggle_disable_focused(kind);
            app.pending_d = false;
            return true;
        }
        app.pending_d = false;
        return false;
    }
    if km_code(app, ActionId::StripPendingD, code) {
        app.pending_leader = false;
        app.pending_d = true;
        return true;
    }
    false
}

fn handle_preset_name_key(app: &mut App, key: event::KeyEvent) {
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    let is_ctrl_c = key.code == KeyCode::Char('c') && ctrl;
    let confirming = app
        .preset_name
        .as_ref()
        .is_some_and(|d| d.confirm_overwrite);

    if confirming {
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') => {
                let _ = app.submit_preset_name(true);
            }
            KeyCode::Esc | KeyCode::Char('n') => {
                if let Some(d) = app.preset_name.as_mut() {
                    d.confirm_overwrite = false;
                }
            }
            KeyCode::Char('c') if ctrl => {
                if let Some(d) = app.preset_name.as_mut() {
                    d.confirm_overwrite = false;
                }
            }
            _ => {}
        }
        return;
    }

    if key.code == KeyCode::Esc || is_ctrl_c {
        app.preset_name = None;
        return;
    }
    if key.code == KeyCode::Enter {
        let _ = app.submit_preset_name(false);
        return;
    }
    if let Some(dialog) = app.preset_name.as_mut() {
        let _ = apply_text_field_key(&mut dialog.field, key.code, ctrl);
    }
}

fn handle_leader_key(app: &mut App, code: KeyCode) -> bool {
    use keymap::ActionId;
    if app.pending_leader {
        app.pending_leader = false;
        if km_code(app, ActionId::LeaderManage, code) {
            app.open_unified_picker();
        } else if km_code(app, ActionId::LeaderPresetSave, code) {
            app.begin_preset_save();
        } else if km_code(app, ActionId::LeaderPresetOpen, code) {
            app.begin_preset_open();
        } else if km_code(app, ActionId::LeaderSummary, code) {
            app.open_summary_panel();
        } else if km_code(app, ActionId::LeaderCancel, code) {
            // cancel
        } else {
            app.set_flash("UNKNOWN LEADER");
        }
        return true;
    }

    if km_code(app, ActionId::LogListLeader, code) {
        app.clear_visual();
        app.pending_d = false;
        app.pending_yank = false;
        app.pending_chip = false;
        app.pending_exclude = false;
        app.pending_lock = false;
        app.pending_time = false;
        app.pending_m = false;
        app.pending_open = false;
        app.pending_leader = true;
        return true;
    }
    false
}

/// Read-only Help panel keys. Esc / `?` / Ctrl+C close without resuming follow.
fn handle_help_key(app: &mut App, key: event::KeyEvent) {
    use keymap::ActionId;
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if key.code == KeyCode::Char('c') && ctrl {
        app.close_help();
        return;
    }
    if km_event(app, ActionId::HelpClose, key) || km_event(app, ActionId::HelpToggle, key) {
        app.close_help();
    } else if km_event(app, ActionId::HelpScrollDown, key) || key.code == KeyCode::Down {
        app.scroll_help(1);
    } else if km_event(app, ActionId::HelpScrollUp, key) || key.code == KeyCode::Up {
        app.scroll_help(-1);
    } else if km_event(app, ActionId::HelpJumpDown, key) {
        app.scroll_help(FAST_SCROLL_STEP);
    } else if km_event(app, ActionId::HelpJumpUp, key) {
        app.scroll_help(-FAST_SCROLL_STEP);
    } else if km_event(app, ActionId::HelpBottom, key) {
        let n = crate::help::help_body_lines(app).len();
        app.help_scroll = n.saturating_sub(1);
    } else if km_event(app, ActionId::HelpTop, key) {
        app.help_scroll = 0;
    }
}

/// Read-only summary panel keys (Leader `i`). `j`/`k`/`Down`/`Up` scroll the
/// body; `Esc`/Ctrl+C close without resuming follow (same convention as
/// Detail/Help). Re-pressing the `Leader i` chord while open also closes it
/// (toggle semantics) — this is the only place `pending_leader` is armed
/// while the panel owns key routing.
fn handle_summary_key(app: &mut App, key: event::KeyEvent) {
    use keymap::ActionId;
    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if key.code == KeyCode::Char('c') && ctrl {
        app.close_summary_panel();
        return;
    }
    if app.pending_leader {
        app.pending_leader = false;
        if km_code(app, ActionId::LeaderSummary, key.code) {
            app.close_summary_panel();
        }
        return;
    }
    if key.code == KeyCode::Esc {
        app.close_summary_panel();
        return;
    }
    if km_code(app, ActionId::LogListLeader, key.code) {
        app.pending_leader = true;
        return;
    }
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => app.scroll_summary(1),
        KeyCode::Char('k') | KeyCode::Up => app.scroll_summary(-1),
        KeyCode::Char('J') => app.scroll_summary(FAST_SCROLL_STEP),
        KeyCode::Char('K') => app.scroll_summary(-FAST_SCROLL_STEP),
        _ => {}
    }
}

/// Route keys to the open `tt` time panel. Esc / Ctrl+C cancel without applying
/// and do not resume following (same draft-cancel convention as Picker).
fn handle_time_panel_key(app: &mut App, key: event::KeyEvent) {
    use time_panel::TimePanelOutcome;

    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if key.code == KeyCode::Esc || (key.code == KeyCode::Char('c') && ctrl) {
        app.close_time_panel();
        return;
    }

    let Some(panel) = app.time_panel.as_mut() else {
        return;
    };
    match panel.handle_key(key.code) {
        TimePanelOutcome::Continue => {}
        TimePanelOutcome::Cancel => {
            app.close_time_panel();
        }
        TimePanelOutcome::Submit(bound) => {
            app.apply_time_bound(bound);
        }
        TimePanelOutcome::Flash(msg) => {
            app.set_flash(msg);
        }
    }
}

fn handle_normal_key(app: &mut App, _input: &mut input::InputBox, code: KeyCode) {
    use app::{Focus, StripKind, YankField};
    use keymap::ActionId;

    // Visual-line: handle selection motions / yank before anything else.
    if app.visual_anchor.is_some() && app.focus == Focus::LogList {
        if km_code(app, ActionId::VisualMoveDown, code) {
            app.move_cursor_manual(1);
            return;
        }
        if km_code(app, ActionId::VisualMoveUp, code) {
            app.move_cursor_manual(-1);
            return;
        }
        if km_code(app, ActionId::VisualJumpDown, code) {
            app.move_cursor_manual(FAST_SCROLL_STEP);
            return;
        }
        if km_code(app, ActionId::VisualJumpUp, code) {
            app.move_cursor_manual(-FAST_SCROLL_STEP);
            return;
        }
        if km_code(app, ActionId::VisualYankRaw, code) {
            if let Some((lo, hi)) = app.selection_range() {
                if let Some(text) = app.yank_range(lo, hi, YankField::Raw) {
                    apply_yank(app, text);
                }
            }
            app.clear_visual();
            return;
        }
        if km_code(app, ActionId::VisualYankMsg, code) {
            if let Some((lo, hi)) = app.selection_range() {
                if let Some(text) = app.yank_range(lo, hi, YankField::Msg) {
                    apply_yank(app, text);
                }
            }
            app.clear_visual();
            return;
        }
        if km_code(app, ActionId::VisualCancel, code) {
            app.clear_visual();
            focus_loglist_and_follow(app);
            return;
        }
        app.clear_visual();
        // fall through so the key still does its Normal action
    }

    if handle_leader_key(app, code) {
        return;
    }

    // Open/switch source operator pending (`o` + `f`/`s`).
    if app.pending_open {
        app.pending_open = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::OpenCancel, code) {
                return;
            }
            if km_code(app, ActionId::OpenFile, code) {
                app.open_file_source_panel(false);
                return;
            }
            if km_code(app, ActionId::OpenStream, code) {
                app.open_stream_source_panel(false);
                return;
            }
            app.set_flash("of=file  os=stream");
        }
        return;
    }

    // Yank operator pending: consume the second key (or Esc) and return.
    if app.pending_yank {
        app.pending_yank = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::YankCancel, code) {
                return;
            }
            if km_code(app, ActionId::YankCli, code) {
                let cmd = app.export_cli_command();
                apply_yank(app, cmd);
                return;
            }
            if km_code(app, ActionId::YankMsg, code) {
                app.begin_msg_token_picker(crate::picker::MsgChipPurpose::Yank);
                return;
            }
            let field = if km_code(app, ActionId::YankTag, code) {
                Some(YankField::Tag)
            } else if km_code(app, ActionId::YankPkg, code) {
                Some(YankField::Pkg)
            } else if km_code(app, ActionId::YankPid, code) {
                Some(YankField::Pid)
            } else if km_code(app, ActionId::YankTid, code) {
                Some(YankField::Tid)
            } else if km_code(app, ActionId::YankLevel, code) {
                Some(YankField::Level)
            } else if km_code(app, ActionId::YankRaw, code)
                || km_code(app, ActionId::YankLine, code)
            {
                // Historical: both `y` and `r` yank the raw line.
                Some(YankField::Raw)
            } else if km_code(app, ActionId::YankTime, code) {
                Some(YankField::Timestamp)
            } else {
                None
            };
            if let Some(field) = field {
                if let Some(text) = app.yank_field(field) {
                    apply_yank(app, text);
                }
            }
        }
        return;
    }

    // Chip-from-cursor operator pending.
    if app.pending_chip {
        app.pending_chip = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::ChipFieldCancel, code) {
                app.cancel_chip_from_cursor();
                return;
            }
            if let Some(field) = chip_field_from_keymap(app, code) {
                match field {
                    input::ChipField::Msg => {
                        app.begin_msg_token_picker(crate::picker::MsgChipPurpose::Chip {
                            exclude: false,
                        });
                    }
                    other => {
                        let _ = app.push_chip_from_field(other);
                    }
                }
            } else if chip_field_unsupported_key(app, code) {
                app.set_flash("NO RAW/TIMESTAMP");
            } else {
                app.set_flash("UNKNOWN FIELD");
            }
        }
        return;
    }

    // Exclude-from-cursor operator pending.
    if app.pending_exclude {
        app.pending_exclude = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::ChipFieldCancel, code) {
                app.cancel_chip_from_cursor();
                return;
            }
            if let Some(field) = chip_field_from_keymap(app, code) {
                match field {
                    input::ChipField::Msg => {
                        app.begin_msg_token_picker(crate::picker::MsgChipPurpose::Chip {
                            exclude: true,
                        });
                    }
                    other => {
                        let _ = app.push_exclude_from_field(other);
                    }
                }
            } else if chip_field_unsupported_key(app, code) {
                app.set_flash("NO RAW/TIMESTAMP");
            } else {
                app.set_flash("UNKNOWN FIELD");
            }
        }
        return;
    }

    // Session lock / view-focus operator pending.
    if app.pending_lock {
        app.pending_lock = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::LockCancel, code) {
                app.cancel_lock_pending();
            } else if km_code(app, ActionId::LockPid, code) {
                app.apply_session_lock(app::LockKind::Pid);
            } else if km_code(app, ActionId::LockTid, code) {
                app.apply_session_lock(app::LockKind::Tid);
            } else if km_code(app, ActionId::LockClear, code) {
                app.clear_session_lock();
            } else if km_code(app, ActionId::LockViewHighlight, code) {
                app.toggle_view_focus(app::ViewFocusKind::Highlight);
            } else if km_code(app, ActionId::LockViewSevere, code) {
                app.toggle_view_focus(app::ViewFocusKind::Severe);
            } else {
                app.set_flash("UNKNOWN");
            }
        }
        return;
    }

    // Global time-window operator pending (file mode).
    if app.pending_time {
        app.pending_time = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::TimeCancel, code) {
                app.cancel_time_pending();
            } else if km_code(app, ActionId::TimeSet, code) {
                let _ = app.open_time_panel();
            } else if km_code(app, ActionId::TimeClear, code) {
                app.clear_time_bound();
            } else {
                app.set_flash("UNKNOWN");
            }
        }
        return;
    }

    // Bookmark operator pending.
    if app.pending_m {
        app.pending_m = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::BookmarkCancel, code) {
                app.cancel_bookmark_op();
            } else if km_code(app, ActionId::BookmarkAdd, code) {
                app.bookmark_add_current();
            } else if km_code(app, ActionId::BookmarkRemove, code) {
                app.bookmark_remove_current();
            } else if km_code(app, ActionId::BookmarkManage, code) {
                app.open_picker(crate::picker::PickerKind::Bookmark);
            } else {
                app.set_flash("UNKNOWN");
            }
        }
        return;
    }

    // Filter / Exclude / Highlight strip: dd delete, di toggle disable.
    if handle_strip_d_chord(app, StripKind::Filter, code) {
        return;
    }
    if handle_strip_d_chord(app, StripKind::Exclude, code) {
        return;
    }
    if handle_strip_d_chord(app, StripKind::Highlight, code) {
        return;
    }
    if !km_code(app, ActionId::StripPendingD, code) {
        app.pending_d = false;
    }

    if km_code(app, ActionId::GlobalQuit, code) {
        app.should_quit = true;
        return;
    }
    if km_code(app, ActionId::GlobalFocusNext, code) {
        app.cycle_visible_focus_forward();
        return;
    }
    if km_code(app, ActionId::GlobalFocusPrev, code) {
        app.cycle_visible_focus_backward();
        return;
    }
    if km_code(app, ActionId::GlobalFocusFilter, code) {
        app.focus = Focus::ChipStrip;
        return;
    }
    if km_code(app, ActionId::GlobalFocusExclude, code) {
        app.focus = Focus::ExcludeStrip;
        return;
    }
    if km_code(app, ActionId::GlobalFocusHighlight, code) {
        app.focus = Focus::HighlightStrip;
        return;
    }
    if km_code(app, ActionId::GlobalFocusLog, code) {
        app.focus = Focus::LogList;
        return;
    }
    if km_code(app, ActionId::GlobalFocusInput, code) {
        app.open_unified_picker();
        return;
    }

    // Esc / resume: detail close, follow, or return to log list.
    let esc = km_code(app, ActionId::LogListResumeFollow, code)
        || km_code(app, ActionId::StripResumeFollow, code)
        || km_code(app, ActionId::DetailClose, code);
    if esc {
        if app.detail_open() {
            app.close_detail();
            app.focus = Focus::LogList;
        } else if app.focus == Focus::LogList {
            focus_loglist_and_follow(app);
        } else {
            focus_loglist(app);
        }
        return;
    }

    if app.focus == Focus::LogList {
        if km_code(app, ActionId::LogListMoveDown, code) {
            app.move_cursor_manual(1);
            return;
        }
        if km_code(app, ActionId::LogListMoveUp, code) {
            app.move_cursor_manual(-1);
            return;
        }
        if km_code(app, ActionId::LogListJumpDown, code) {
            app.move_cursor_manual(FAST_SCROLL_STEP);
            return;
        }
        if km_code(app, ActionId::LogListJumpUp, code) {
            app.move_cursor_manual(-FAST_SCROLL_STEP);
            return;
        }
        if km_code(app, ActionId::LogListJumpTop, code) {
            app.following = false;
            app.jump_top();
            return;
        }
        if km_code(app, ActionId::LogListJumpBottom, code) {
            app.resume_following();
            return;
        }
        if km_code(app, ActionId::LogListDetailFields, code) {
            app.toggle_detail_fields();
            return;
        }
        if km_code(app, ActionId::LogListDetailPretty, code) {
            app.toggle_detail_pretty();
            return;
        }
        if km_code(app, ActionId::LogListWrapToggle, code) {
            app.toggle_collapsed_view();
            return;
        }
        if km_code(app, ActionId::LogListChip, code) {
            app.begin_chip_from_cursor();
            return;
        }
        if km_code(app, ActionId::LogListExcludeChip, code) {
            app.begin_exclude_from_cursor();
            return;
        }
        if km_code(app, ActionId::LogListLock, code) {
            app.begin_lock_from_cursor();
            return;
        }
        if km_code(app, ActionId::LogListTime, code) && app.is_file_mode() {
            app.begin_time_op();
            return;
        }
        if km_code(app, ActionId::LogListBookmark, code) {
            app.begin_bookmark_op();
            return;
        }
        if km_code(app, ActionId::LogListYank, code) {
            app.pending_chip = false;
            app.pending_exclude = false;
            app.pending_lock = false;
            app.pending_time = false;
            app.pending_m = false;
            app.pending_leader = false;
            app.pending_open = false;
            app.pending_yank = true;
            return;
        }
        if km_code(app, ActionId::LogListOpen, code) {
            app.begin_open_op();
            return;
        }
        if km_code(app, ActionId::LogListYankMsgLine, code) {
            if let Some(text) = app.yank_field(YankField::Msg) {
                apply_yank(app, text);
            }
            return;
        }
        if km_code(app, ActionId::LogListVisualLine, code) {
            app.enter_visual_line();
            return;
        }
        if km_code(app, ActionId::LogListNextMatch, code) {
            if matches!(app.find_match(1), app::FindJumpResult::NoMore) {
                app.set_flash("NO MORE");
            }
            return;
        }
        if km_code(app, ActionId::LogListPrevMatch, code) {
            if matches!(app.find_match(-1), app::FindJumpResult::NoMore) {
                app.set_flash("NO MORE");
            }
            return;
        }
        if km_code(app, ActionId::LogListNextSevere, code) {
            match app.find_severe(1) {
                app::FindJumpResult::None => app.set_flash("NO ERROR"),
                app::FindJumpResult::NoMore => app.set_flash("NO MORE"),
                app::FindJumpResult::Moved => {}
            }
            return;
        }
        if km_code(app, ActionId::LogListPrevSevere, code) {
            match app.find_severe(-1) {
                app::FindJumpResult::None => app.set_flash("NO ERROR"),
                app::FindJumpResult::NoMore => app.set_flash("NO MORE"),
                app::FindJumpResult::Moved => {}
            }
            return;
        }
    }

    if matches!(
        app.focus,
        Focus::ChipStrip | Focus::ExcludeStrip | Focus::HighlightStrip
    ) {
        let kind = match app.focus {
            Focus::ChipStrip => StripKind::Filter,
            Focus::ExcludeStrip => StripKind::Exclude,
            Focus::HighlightStrip => StripKind::Highlight,
            _ => unreachable!(),
        };
        if km_code(app, ActionId::StripPrevGroup, code) {
            app.move_strip_cursor(kind, -1);
            return;
        }
        if km_code(app, ActionId::StripNextGroup, code) {
            app.move_strip_cursor(kind, 1);
            return;
        }
    }

    if km_code(app, ActionId::GlobalFilterNew, code) {
        app.open_picker_new(crate::picker::PickerKind::Filter);
        return;
    }
    if km_code(app, ActionId::GlobalHighlightNew, code) {
        app.open_picker_new(crate::picker::PickerKind::Highlight);
        return;
    }
    if km_code(app, ActionId::GlobalExcludeNew, code) {
        app.open_picker_new(crate::picker::PickerKind::Exclude);
        return;
    }
    if km_code(app, ActionId::GlobalOpenHelp, code) || km_code(app, ActionId::StripOpenHelp, code) {
        if matches!(
            app.focus,
            Focus::LogList | Focus::ChipStrip | Focus::ExcludeStrip | Focus::HighlightStrip
        ) && crate::help::help_available(app)
        {
            app.open_help();
        }
    }
}

fn chip_field_from_keymap(app: &App, code: KeyCode) -> Option<input::ChipField> {
    use input::ChipField;
    use keymap::ActionId;
    if km_code(app, ActionId::ChipFieldTag, code) {
        Some(ChipField::Tag)
    } else if km_code(app, ActionId::ChipFieldMsg, code) {
        Some(ChipField::Msg)
    } else if km_code(app, ActionId::ChipFieldPkg, code) {
        Some(ChipField::Pkg)
    } else if km_code(app, ActionId::ChipFieldPid, code) {
        Some(ChipField::Pid)
    } else if km_code(app, ActionId::ChipFieldTid, code) {
        Some(ChipField::Tid)
    } else if km_code(app, ActionId::ChipFieldLevel, code) {
        Some(ChipField::Level)
    } else {
        None
    }
}

/// Keys that are valid yank fields but not chip fields (`r`/`y`/`s` by default).
fn chip_field_unsupported_key(app: &App, code: KeyCode) -> bool {
    use keymap::ActionId;
    km_code(app, ActionId::YankRaw, code)
        || km_code(app, ActionId::YankLine, code)
        || km_code(app, ActionId::YankTime, code)
}

fn handle_insert_key(
    app: &mut App,
    input: &mut input::InputBox,
    code: KeyCode,
) -> Result<(), String> {
    // Field candidates are draft-driven (no `/` open). Align with Search:
    // Up/Down move selection; Enter/Tab confirm when candidates exist.
    // Tab/BackTab cycle focus when there are no field candidates.
    // Enter two-step (no candidates): pending draft → commit pill; chips ready
    // → submit group; empty input → jump focus to LogList.
    // `/` and Space are literal draft chars. Filter chips always ignore-case.
    // Note: legacy Insert modal has no KeyEvent modifiers here — Ctrl chords
    // are unavailable; arrows/Home/End still edit the draft caret.
    match code {
        KeyCode::Esc => {
            *input = input::InputBox::default();
            app.mode = app::Mode::Normal;
            focus_loglist(app);
        }
        KeyCode::Up => {
            if input.field_popup_visible() {
                input.move_field_selection(-1);
            }
        }
        KeyCode::Down => {
            if input.field_popup_visible() {
                input.move_field_selection(1);
            }
        }
        KeyCode::Tab => {
            if !input.confirm_field_candidate() {
                app.cycle_focus_forward();
                app.mode = if app.focus == app::Focus::Input {
                    app::Mode::Insert
                } else {
                    app::Mode::Normal
                };
            }
        }
        KeyCode::BackTab => {
            app.cycle_focus_backward();
            app.mode = if app.focus == app::Focus::Input {
                app::Mode::Insert
            } else {
                app::Mode::Normal
            };
        }
        KeyCode::Enter => {
            if input.confirm_field_candidate_on_enter() {
                // picked field; draft cleared
            } else if input.has_pending_draft() {
                input.commit_draft_as_chip();
            } else if input.is_empty() {
                app.mode = app::Mode::Normal;
                app.focus = app::Focus::LogList;
            } else if input.exclude_mode {
                // H9: all pills become global excludes (not a Filter group).
                let chips = std::mem::take(&mut input.chips);
                let mut any = false;
                for chip in chips {
                    if app.push_exclude_chip(chip) {
                        any = true;
                    }
                }
                input.exclude_mode = false;
                if !any && app.status_msg.is_none() {
                    app.set_flash("EXISTS");
                }
                app.mode = app::Mode::Normal;
                focus_loglist_and_follow(app);
            } else if let Some(group) = input.build_group(true)? {
                if app.push_filter_group(group) {
                    app.rebuild_visible();
                }
                app.mode = app::Mode::Normal;
                focus_loglist_and_follow(app);
            }
        }
        KeyCode::Backspace => input.backspace(),
        KeyCode::Left => input.draft.move_left(),
        KeyCode::Right => input.draft.move_right(),
        KeyCode::Home => input.draft.home(),
        KeyCode::End => input.draft.end(),
        KeyCode::Char('!') if input.is_empty() => {
            if !input.toggle_exclude_mode() {
                input.push_char('!');
            }
        }
        KeyCode::Char(c) => input.push_char(c),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use alnav::live::{LiveFilter, LiveSession};
    use crossterm::event::{KeyEvent, KeyModifiers};
    use std::io::BufRead;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn sleep_child() -> std::process::Child {
        Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep")
    }

    fn live_session_from_sh(script: &str) -> LiveSession {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(script)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sh live session");
        let stdout = child.stdout.take().expect("piped stdout");
        LiveSession {
            child,
            lines: LiveFilter {
                inner: std::io::BufReader::new(stdout).lines(),
                start_marker: None,
            },
            used_history_fallback: true,
        }
    }

    fn dummy_ctl() -> LiveIngestCtl {
        let session = live_session_from_sh("sleep 30");
        let (ring, child) = ingest::spawn_live_ingest(session);
        LiveIngestCtl::new(
            LiveBackend::Hdc,
            None,
            ingest::IngestHandle::Ring(ring),
            child,
        )
    }

    #[test]
    fn dashboard_invalid_last_recent_is_removed_and_cursor_is_clamped() {
        let dir = tempfile::TempDir::new().unwrap();
        let valid = dir.path().join("valid.log");
        std::fs::write(&valid, "line\n").unwrap();
        let missing = dir.path().join("missing.log");
        let recent = recent::RecentFiles {
            paths: vec![valid.display().to_string(), missing.display().to_string()],
        };
        let mut app = App::new(100);
        app.config_dir = dir.path().to_path_buf();
        app.recent = recent.clone();
        app.dashboard = Some(dashboard::DashboardState::new(recent));
        app.dashboard.as_mut().unwrap().cursor = 4;
        let mut live = None;

        handle_dashboard_key(
            &mut app,
            &mut live,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

        let dash = app.dashboard.as_ref().unwrap();
        assert_eq!(dash.cursor, 3);
        assert_eq!(dash.recent.paths, vec![valid.display().to_string()]);
        assert!(matches!(
            dash.selected(),
            Some(dashboard::DashboardItem::Recent { path, .. })
                if path == valid.display().to_string()
        ));
        assert!(app.status_msg.is_some());
    }

    #[test]
    fn dashboard_ctrl_c_still_quits() {
        let mut app = App::new(100);
        app.dashboard = Some(dashboard::DashboardState::new(
            recent::RecentFiles::default(),
        ));
        let mut live = None;

        handle_dashboard_key(
            &mut app,
            &mut live,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );

        assert!(app.should_quit);
    }

    #[test]
    fn live_child_guard_replace_kills_previous() {
        let mut guard = LiveChildGuard::new(Some(sleep_child()));
        let old_id = guard.0.as_ref().unwrap().id();
        guard.replace(Some(Command::new("true").spawn().unwrap()));
        // Old sleep should no longer be running (best-effort: waitpid already done).
        let still = Command::new("kill")
            .args(["-0", &old_id.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(!still, "replaced child should be killed");
        assert!(guard.0.is_some());
    }

    #[test]
    fn try_reconnect_skips_when_not_disconnected() {
        let mut app = App::new(100);
        app.ingest_done = false;
        let mut ctl = dummy_ctl();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_c = Arc::clone(&calls);
        let ok = ctl.try_reconnect(&mut app, Instant::now(), || {
            calls_c.fetch_add(1, Ordering::SeqCst);
            Err("should not spawn".into())
        });
        assert!(!ok);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn try_reconnect_respects_backoff() {
        let mut app = App::new(100);
        app.ingest_done = true;
        let mut ctl = dummy_ctl();
        let t0 = Instant::now();
        ctl.last_reconnect_at = Some(t0);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_c = Arc::clone(&calls);
        let ok = ctl.try_reconnect(&mut app, t0 + Duration::from_millis(500), || {
            calls_c.fetch_add(1, Ordering::SeqCst);
            Err("no".into())
        });
        assert!(!ok);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(app.ingest_done);
    }

    #[test]
    fn try_reconnect_success_keeps_buffer_and_clears_done() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Keep    : a").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.rows().len(), 1);
        app.ingest_done = true;

        let mut ctl = dummy_ctl();
        let t0 = Instant::now();
        let ok = ctl.try_reconnect(&mut app, t0, || {
            // Stay alive past RECONNECT_HEALTH_WAIT so health check passes.
            Ok(live_session_from_sh(
                "printf '04-02 10:00:01.000  1  1 I New     : b\\n'; sleep 2",
            ))
        });
        assert!(ok);
        assert!(!app.ingest_done);
        assert_eq!(app.status_msg.as_deref(), Some("RECONNECTED"));
        assert_eq!(
            app.rows().len(),
            1,
            "reconnect must not clear prior buffer before drain"
        );
        // Backoff stamp retained so a dying session cannot immediately re-flash.
        assert_eq!(ctl.last_reconnect_at, Some(t0));

        // Drain new session lines into the same buffer.
        let deadline = Instant::now() + Duration::from_secs(2);
        while app.rows().len() < 2 && Instant::now() < deadline {
            if let Some(ingest) = ctl.ingest.as_ref() {
                app.drain(ingest);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.rows().len(), 2);
        assert_eq!(app.rows()[0].tag, "Keep");
        assert_eq!(app.rows()[1].tag, "New");
    }

    #[test]
    fn try_reconnect_rejects_immediate_exit_without_flash() {
        let mut app = App::new(100);
        app.ingest_done = true;
        let mut ctl = dummy_ctl();
        let t0 = Instant::now();
        let ok = ctl.try_reconnect(&mut app, t0, || Ok(live_session_from_sh("true")));
        assert!(
            !ok,
            "child that exits during health wait must not count as reconnect"
        );
        assert!(app.ingest_done);
        assert_ne!(app.status_msg.as_deref(), Some("RECONNECTED"));
        assert_eq!(ctl.last_reconnect_at, Some(t0));
    }

    #[test]
    fn try_reconnect_failure_stays_disconnected() {
        let mut app = App::new(100);
        app.ingest_done = true;
        let mut ctl = dummy_ctl();
        let t0 = Instant::now();
        let ok = ctl.try_reconnect(&mut app, t0, || Err("device gone".into()));
        assert!(!ok);
        assert!(app.ingest_done);
        assert_eq!(ctl.last_reconnect_at, Some(t0));
    }

    #[test]
    fn question_opens_help_and_esc_closes_without_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
        assert!(app.help_open);
        handle_help_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.help_open);
        assert!(!app.following, "closing help must not resume following");
    }

    #[test]
    fn help_shift_jk_scrolls_by_fast_step() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
        assert!(app.help_open);
        let body_len = crate::help::help_body_lines(&app).len();
        assert!(
            body_len > FAST_SCROLL_STEP as usize,
            "help body needs more than {FAST_SCROLL_STEP} lines, got {body_len}"
        );
        assert_eq!(app.help_scroll, 0);
        handle_help_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE),
        );
        assert_eq!(app.help_scroll, FAST_SCROLL_STEP as usize);
        handle_help_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('K'), KeyModifiers::NONE),
        );
        assert_eq!(app.help_scroll, 0);
    }

    #[test]
    fn help_jk_scrolls_one_line_and_question_closes_without_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
        assert!(app.help_open);
        handle_help_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(app.help_scroll, 1);
        handle_help_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        );
        assert_eq!(app.help_scroll, 0);
        handle_help_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        );
        assert!(!app.help_open);
        assert!(
            !app.following,
            "closing help with ? must not resume following"
        );
    }

    #[test]
    fn question_ignored_while_operator_pending() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.pending_yank = true;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('?'));
        assert!(!app.help_open);
    }

    #[test]
    fn slash_still_opens_highlight_new() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('/'));
        let picker = app.picker.as_ref().expect("highlight picker");
        assert_eq!(picker.kind, crate::picker::PickerKind::Highlight);
        assert!(matches!(picker.mode, crate::picker::PickerMode::New));
    }

    #[test]
    fn space_f_no_longer_opens_filter_picker() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        assert!(app.pending_leader);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        assert!(!app.pending_leader);
        assert!(app.picker.is_none());
    }

    #[test]
    fn slash_opens_highlight_new() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('/'));
        assert!(!app.highlight_box.editing);
        let picker = app.picker.as_ref().expect("highlight picker");
        assert_eq!(picker.kind, PickerKind::Highlight);
        assert_eq!(picker.mode, PickerMode::New);
    }

    #[test]
    fn space_space_opens_unified_manage() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        assert!(app.pending_leader);
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        assert!(!app.pending_leader);
        let picker = app.picker.as_ref().expect("unified picker");
        assert_eq!(picker.kind, PickerKind::Unified);
        assert_eq!(picker.mode, PickerMode::Manage);
    }

    #[test]
    fn space_i_opens_summary_panel_and_esc_closes_without_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        assert!(app.pending_leader);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
        assert!(!app.pending_leader);
        assert!(app.summary_open());

        handle_summary_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.summary_open());
        assert!(!app.following, "Esc must not resume following");
    }

    #[test]
    fn space_i_while_open_toggles_close() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
        assert!(app.summary_open());

        handle_summary_key(
            &mut app,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE),
        );
        assert!(app.pending_leader);
        handle_summary_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        assert!(!app.pending_leader);
        assert!(!app.summary_open(), "Leader i while open re-toggles closed");
    }

    #[test]
    fn summary_jk_scrolls_ready_body() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..3 {
            tx.send(
                crate::model::EntryRow::from_line(&format!(
                    "04-02 10:00:00.000  1  1 I Tag{i}   : line{i}"
                ))
                .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
        app.flush_summary_job(std::time::Duration::from_secs(5));
        assert!(matches!(app.summary_view, app::SummaryView::Ready(_)));

        assert_eq!(app.summary_scroll, 0);
        handle_summary_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        );
        assert_eq!(app.summary_scroll, 1);
        handle_summary_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        );
        assert_eq!(app.summary_scroll, 0);
    }

    #[test]
    fn bare_keys_open_unified_or_new() {
        use crate::filter_model::Group;
        use crate::highlight_model::HighlightGroup;
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.groups.groups.push(Group {
            label: "g".into(),
            chips: Vec::new(),
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("err").unwrap());
        assert!(app
            .groups
            .push_exclude(crate::input::Chip {
                field: crate::input::ChipField::Tag,
                value: "noise".into(),
            })
            .unwrap());

        handle_normal_key(&mut app, &mut input, KeyCode::Char(';'));
        let filter_new = app.picker.as_ref().expect("; opens filter New");
        assert_eq!(filter_new.kind, PickerKind::Filter);
        assert_eq!(filter_new.mode, PickerMode::New);
        app.close_picker();

        handle_normal_key(&mut app, &mut input, KeyCode::Char('/'));
        let hl = app.picker.as_ref().unwrap();
        assert_eq!(hl.kind, PickerKind::Highlight);
        assert_eq!(hl.mode, PickerMode::New);
        app.close_picker();

        handle_normal_key(&mut app, &mut input, KeyCode::Char('`'));
        let exclude = app.picker.as_ref().expect("` opens exclude New");
        assert_eq!(exclude.kind, PickerKind::Exclude);
        assert_eq!(exclude.mode, PickerMode::New);
        app.close_picker();

        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert!(app.pending_m);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert!(!app.pending_m);
        let bookmark = app.picker.as_ref().expect("mm opens bookmark Manage");
        assert_eq!(bookmark.kind, PickerKind::Bookmark);
        assert_eq!(bookmark.mode, PickerMode::Manage);
    }

    #[test]
    fn manage_no_match_stays_in_manage() {
        use crate::highlight_model::HighlightGroup;
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("error").unwrap());
        app.open_unified_picker();
        assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
        assert_eq!(app.picker.as_ref().unwrap().kind, PickerKind::Unified);

        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        );
        let picker = app.picker.as_ref().unwrap();
        assert_eq!(picker.mode, PickerMode::Manage);
        assert_eq!(picker.query, "z");
        assert!(picker.draft.is_empty());
    }

    #[test]
    fn manual_new_clears_draft_stays_in_new() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char(';'));
        assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);

        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        );
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        let picker = app.picker.as_ref().unwrap();
        assert_eq!(picker.mode, PickerMode::New);
        assert_eq!(picker.kind, PickerKind::Filter);
        assert!(picker.input.as_ref().is_some_and(|box_| box_.is_empty()));
    }

    #[test]
    fn open_picker_new_forces_new_mode() {
        use crate::picker::{PickerKind, PickerMode};

        for kind in [
            PickerKind::Filter,
            PickerKind::Highlight,
            PickerKind::Exclude,
            PickerKind::Bookmark,
        ] {
            let mut app = App::new(100);
            app.following = false;
            app.open_picker_new(kind.clone());
            assert_eq!(
                app.picker.as_ref().unwrap().mode,
                PickerMode::New,
                "{kind:?} must open New"
            );
            handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert!(app.picker.is_none(), "{kind:?} Esc from New closes");
            assert!(!app.following);
        }
    }

    #[test]
    fn unified_picker_opens_in_manage_even_when_empty() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        app.open_unified_picker();
        assert_eq!(app.picker.as_ref().unwrap().kind, PickerKind::Unified);
        assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::Manage);
    }

    #[test]
    fn picker_esc_closes_without_resuming_following() {
        use crate::picker::PickerKind;

        let mut app = App::new(100);
        app.following = false;
        app.open_picker_new(PickerKind::Highlight);
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.picker.is_none());
        assert!(!app.following);
    }

    #[test]
    fn highlight_picker_new_submit_closes_picker() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        app.open_picker_new(PickerKind::Highlight);
        assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
        for c in "error".chars() {
            handle_picker_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.highlight_groups.groups.len(), 1);
        assert_eq!(app.highlight_groups.groups[0].pattern, "error");
        assert!(app.picker.is_none());
        assert_eq!(app.active_highlight, Some(0));
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn picker_esc_from_unified_closes() {
        let mut app = App::new(100);
        app.open_unified_picker();
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.picker.is_none());
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn unified_delete_requires_confirm_and_can_cancel() {
        use crate::highlight_model::HighlightGroup;

        let mut app = App::new(100);
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
        app.open_unified_picker();

        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE));
        assert!(app.picker.as_ref().unwrap().confirm.is_some());
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        );
        assert!(
            app.picker.as_ref().unwrap().query.is_empty(),
            "confirm state must swallow keys instead of editing the picker"
        );
        assert!(app.picker.as_ref().unwrap().confirm.is_some());
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.highlight_groups.groups.len(), 1);
        assert!(app.picker.as_ref().unwrap().confirm.is_none());

        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
        );
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.highlight_groups.groups.is_empty());
        assert!(app.picker.is_some());
    }

    #[test]
    fn unified_enter_toggles_enabled_and_stays_open() {
        use crate::filter_model::Group;

        let mut app = App::new(100);
        app.groups.groups.push(Group {
            label: "first".into(),
            chips: Vec::new(),
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        app.groups.groups.push(Group {
            label: "second".into(),
            chips: Vec::new(),
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        app.open_unified_picker();
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(app.picker.is_some());
        assert!(app.groups.groups[0].enabled);
        assert!(!app.groups.groups[1].enabled);
    }

    #[test]
    fn unified_tab_multiselect_enter_batch_toggles_enabled() {
        use crate::filter_model::Group;
        use crate::highlight_model::HighlightGroup;

        let mut app = App::new(100);
        app.groups.groups.push(Group {
            label: "f1".into(),
            chips: Vec::new(),
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("h1").unwrap());
        app.open_unified_picker();

        // Tab marks first item and moves to second
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.picker.as_ref().unwrap().checked.len(), 1);
        assert_eq!(app.picker.as_ref().unwrap().selected, 1);
        // Tab marks second
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.picker.as_ref().unwrap().checked.len(), 2);

        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.picker.as_ref().unwrap().confirm.is_none());
        assert!(!app.groups.groups[0].enabled);
        assert!(!app.highlight_groups.groups[0].enabled);
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.highlight_groups.groups.len(), 1);
    }

    #[test]
    fn exclude_picker_rejects_second_chip_draft() {
        use crate::input::{Chip, ChipField};
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        app.open_picker_new(PickerKind::Exclude);
        assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
        app.picker
            .as_mut()
            .unwrap()
            .input
            .as_mut()
            .unwrap()
            .chips
            .push(Chip {
                field: ChipField::Tag,
                value: "first".into(),
            });

        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        );

        let input = app.picker.as_ref().unwrap().input.as_ref().unwrap();
        assert_eq!(input.chips.len(), 1);
        assert!(input.draft.is_empty(), "second chip draft must be rejected");
    }

    #[test]
    fn picker_query_mid_cursor_backspace_and_ctrl_u() {
        let mut app = App::new(100);
        app.open_unified_picker();
        for c in "abcdef".chars() {
            handle_picker_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        );
        assert_eq!(app.picker.as_ref().unwrap().query.as_str(), "abcef");
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        assert_eq!(app.picker.as_ref().unwrap().query.as_str(), "ef");
        assert_eq!(app.picker.as_ref().unwrap().query.cursor(), 0);
    }

    #[test]
    fn picker_new_ctrl_backspace_kills_word() {
        use crate::picker::PickerMode;

        let mut app = App::new(100);
        app.open_picker_new(crate::picker::PickerKind::Highlight);
        assert!(matches!(app.picker.as_ref().unwrap().mode, PickerMode::New));
        for c in "foo bar".chars() {
            handle_picker_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
        );
        assert_eq!(app.picker.as_ref().unwrap().draft.as_str(), "foo ");
    }

    #[test]
    fn picker_ctrl_a_homes_without_mutating_text() {
        let mut app = App::new(100);
        app.open_unified_picker();
        for c in "abc".chars() {
            handle_picker_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
        );
        let q = &app.picker.as_ref().unwrap().query;
        assert_eq!(q.as_str(), "abc", "Ctrl-A must not insert characters");
        assert_eq!(q.cursor(), 0);
    }

    #[test]
    fn picker_arrows_move_caret_without_mutating_text() {
        let mut app = App::new(100);
        app.open_unified_picker();
        for c in "abc".chars() {
            handle_picker_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let q = &app.picker.as_ref().unwrap().query;
        assert_eq!(q.as_str(), "abc");
        assert_eq!(q.cursor(), 1);
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(app.picker.as_ref().unwrap().query.as_str(), "abc");
        assert_eq!(app.picker.as_ref().unwrap().query.cursor(), 2);
    }

    #[test]
    fn manage_ctrl_e_no_longer_edits() {
        use crate::highlight_model::HighlightGroup;
        use crate::picker::PickerMode;

        let mut app = App::new(100);
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("error").unwrap());
        app.open_unified_picker();
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
        );
        // Ctrl-E is end-of-line on the (empty) query, not edit.
        assert!(matches!(
            app.picker.as_ref().unwrap().mode,
            PickerMode::Manage
        ));
        assert_eq!(app.picker.as_ref().unwrap().query.cursor(), 0);
    }

    #[test]
    fn filtered_highlight_edit_submit_closes_and_updates_pattern() {
        use crate::highlight_model::HighlightGroup;
        use crate::picker::PickerMode;

        let mut app = App::new(100);
        for pattern in ["alpha", "error", "warn"] {
            app.push_or_find_highlight_group(HighlightGroup::from_pattern(pattern).unwrap());
        }
        app.open_unified_picker();
        for c in "warn".chars() {
            handle_picker_key(
                &mut app,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            app.picker.as_ref().unwrap().mode,
            PickerMode::Edit { index: 2 }
        );
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(app.picker.is_none());
        assert_eq!(app.highlight_groups.groups[2].pattern, "warni");
        assert_eq!(app.active_highlight, Some(2));
    }

    #[test]
    fn test_popup_rect_anchors_below_modal_and_clamps_to_space_below() {
        let modal = Rect {
            x: 12,
            y: 10,
            width: 40,
            height: 3,
        };
        let frame_area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let rect = popup_rect(modal, frame_area, 20); // way more matches than fit
                                                      // desired = min(8,20)+2 = 10; +1 gap → y = 14; space = 10; height = 10
        assert_eq!(rect.height, 10);
        assert_eq!(rect.y, 14, "popup should sit one row below the modal");
        assert_eq!(rect.x, modal.x);
        assert_eq!(rect.width, modal.width);
        assert!(
            rect.y > modal.y + modal.height,
            "popup must leave a gap below the modal"
        );
    }

    #[test]
    fn test_popup_rect_clamps_when_little_space_below() {
        let modal = Rect {
            x: 12,
            y: 16,
            width: 40,
            height: 3,
        };
        let frame_area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 20,
        };
        // space below = 1 (≤ gap) → pack flush; desired=8 → height=1
        let rect = popup_rect(modal, frame_area, 6);
        assert_eq!(rect.height, 1);
        assert_eq!(rect.y, 19);
    }

    #[test]
    fn test_top_modal_rect_is_near_top() {
        let frame = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let rect = ui::top_modal_rect(frame, 40, 3);
        assert_eq!(rect.y, 1);
        assert_eq!(rect.width, 40);
        let below = ui::stack_below_rect(rect, frame, 5);
        assert_eq!(below.y, rect.y + rect.height);
        assert_eq!(below.x, rect.x);
    }

    #[test]
    fn test_centered_modal_rect_centers_and_clamps() {
        let frame = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let rect = ui::centered_modal_rect(frame, 40, 5);
        assert_eq!(rect.width, 40);
        assert_eq!(rect.height, 5);
        assert_eq!(rect.x, 20);
        assert_eq!(rect.y, 9);
        let tiny = ui::centered_modal_rect(
            Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 4,
            },
            40,
            10,
        );
        assert_eq!(tiny.width, 10);
        assert_eq!(tiny.height, 4);
    }

    #[test]
    fn test_highlight_box_tab_confirms_selected_candidate() {
        let mut app = App::new(100);
        app.highlight_groups
            .groups
            .push(highlight_model::HighlightGroup::from_pattern("error").unwrap());
        app.highlight_groups
            .groups
            .push(highlight_model::HighlightGroup::from_pattern("errno").unwrap());
        app.highlight_box.begin_editing();
        for c in "er".chars() {
            app.highlight_box.push_char(c);
        }
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert!(!app.highlight_box.editing);
        assert_eq!(
            app.highlight_groups.groups.len(),
            2,
            "must reuse existing, not add"
        );
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_highlight_box_enter_creates_when_no_candidate_match() {
        let mut app = App::new(100);
        app.highlight_groups
            .groups
            .push(highlight_model::HighlightGroup::from_pattern("error").unwrap());
        app.highlight_box.begin_editing();
        for c in "unique".chars() {
            app.highlight_box.push_char(c);
        }
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.highlight_groups.groups.len(), 2);
        assert_eq!(app.highlight_groups.groups[1].pattern, "unique");
    }

    #[test]
    fn test_semicolon_opens_filter_picker_in_new() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        super::handle_normal_key(&mut app, &mut input, KeyCode::Char(';'));
        let picker = app.picker.as_ref().expect("; must open filter New");
        assert_eq!(picker.kind, PickerKind::Filter);
        assert_eq!(picker.mode, PickerMode::New);
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_backtick_opens_exclude_picker() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        super::handle_normal_key(&mut app, &mut input, KeyCode::Char('`'));
        let picker = app.picker.as_ref().expect("` must open exclude picker");
        assert_eq!(picker.kind, PickerKind::Exclude);
        // Empty excludes → New (same rule as Filter/Highlight/Bookmark).
        assert_eq!(picker.mode, PickerMode::New);
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn filter_picker_enter_confirms_field_candidate_without_pill() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        app.open_picker_new(PickerKind::Filter);
        for c in "tag".chars() {
            handle_picker_key(
                &mut app,
                event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        let input = app
            .picker
            .as_ref()
            .and_then(|session| session.input.as_ref())
            .expect("filter new keeps InputBox");
        assert_eq!(input.draft_field, Some(input::ChipField::Tag));
        assert!(input.draft.is_empty());
        assert!(
            input.chips.is_empty(),
            "field confirm must not commit a pill"
        );
        assert!(matches!(
            app.picker.as_ref().map(|session| &session.mode),
            Some(PickerMode::New)
        ));
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert_eq!(data.draft_field, Some(input::ChipField::Tag));
        assert!(data.text.is_empty());
    }

    #[test]
    fn filter_picker_tab_confirms_field_candidate() {
        use crate::picker::PickerKind;

        let mut app = App::new(100);
        app.open_picker_new(PickerKind::Filter);
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char('t'), event::KeyModifiers::NONE),
        );
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Down, event::KeyModifiers::NONE),
        ); // tid
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Tab, event::KeyModifiers::NONE),
        );
        let input = app
            .picker
            .as_ref()
            .and_then(|session| session.input.as_ref())
            .unwrap();
        assert_eq!(input.draft_field, Some(input::ChipField::Tid));
        assert!(input.draft.is_empty());
    }

    #[test]
    fn test_aio_no_longer_enter_insert() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        for c in ['a', 'i', 'o'] {
            handle_normal_key(&mut app, &mut input, KeyCode::Char(c));
            assert_eq!(app.mode, app::Mode::Normal, "{c} must not open Input");
            assert_eq!(app.focus, app::Focus::LogList, "{c} must not open Input");
        }
    }

    #[test]
    fn test_number_keys_switch_focus_and_5_opens_unified_picker() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();

        handle_normal_key(&mut app, &mut input, KeyCode::Char('5'));
        assert!(matches!(
            app.picker.as_ref().map(|picker| &picker.kind),
            Some(crate::picker::PickerKind::Unified)
        ));
        app.close_picker();

        handle_normal_key(&mut app, &mut input, KeyCode::Char('1'));
        assert_eq!(app.focus, app::Focus::ChipStrip);

        handle_normal_key(&mut app, &mut input, KeyCode::Char('2'));
        assert_eq!(app.focus, app::Focus::ExcludeStrip);

        handle_normal_key(&mut app, &mut input, KeyCode::Char('3'));
        assert_eq!(app.focus, app::Focus::HighlightStrip);

        handle_normal_key(&mut app, &mut input, KeyCode::Char('4'));
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_esc_from_other_focus_preserves_loglist_position() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..20 {
            tx.send(
                model::EntryRow::from_line(&format!(
                    "04-02 10:00:00.000  1  1 I Tag     : line{i}"
                ))
                .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.cursor = 5;
        app.list_offset = 2;
        app.following = false;
        app.focus = app::Focus::ChipStrip;
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert_eq!(app.focus, app::Focus::LogList);
        assert!(
            !app.following,
            "Esc from ChipStrip must not resume following"
        );
        assert_eq!(app.cursor, 5);
        assert_eq!(app.list_offset, 2);
    }

    #[test]
    fn test_esc_on_loglist_resumes_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
            .unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : b").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-1);
        assert!(!app.following);
        assert_eq!(app.cursor, 0);
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert!(app.following);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn test_shift_j_and_shift_k_move_cursor_by_seven() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..20 {
            tx.send(
                crate::model::EntryRow::from_line(&format!(
                    "04-02 10:00:00.000  1  1 I Tag     : line{i}"
                ))
                .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.cursor = 10;
        app.following = false;

        handle_normal_key(&mut app, &mut input, KeyCode::Char('K'));
        assert_eq!(app.cursor, 3);

        handle_normal_key(&mut app, &mut input, KeyCode::Char('J'));
        assert_eq!(app.cursor, 10);
    }

    #[test]
    fn test_ctrl_c_in_normal_mode_quits() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_ctrl_c(&mut app, &mut input);
        assert!(app.should_quit);
        assert_eq!(app.mode, app::Mode::Normal);
    }

    #[test]
    fn ctrl_l_clears_buffers_in_hdc_loglist() {
        use crate::export::ExportSource;
        use std::sync::mpsc;

        let mut app = App::new(100);
        app.export_source = ExportSource::Hdc { device: None };
        app.focus = app::Focus::LogList;
        app.mode = app::Mode::Normal;
        let (tx, rx) = mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.rows().len(), 1);

        try_handle_ctrl_l(&mut app);
        assert!(app.rows().is_empty());
        assert!(app.following);
        assert_eq!(app.status_msg.as_deref(), Some("CLEARED"));
    }

    #[test]
    fn ctrl_l_clears_buffers_in_adb_loglist() {
        use crate::export::ExportSource;
        use std::sync::mpsc;

        let mut app = App::new(100);
        app.export_source = ExportSource::Adb { device: None };
        let (tx, rx) = mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);

        try_handle_ctrl_l(&mut app);
        assert!(app.rows().is_empty());
        assert_eq!(app.status_msg.as_deref(), Some("CLEARED"));
    }

    #[test]
    fn ctrl_l_noop_in_file_mode() {
        use crate::export::ExportSource;
        use std::sync::mpsc;

        let mut app = App::new(100);
        app.export_source = ExportSource::File("app.log".into());
        let (tx, rx) = mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        try_handle_ctrl_l(&mut app);
        assert_eq!(app.rows().len(), 1);
        assert_ne!(app.status_msg.as_deref(), Some("CLEARED"));
    }

    #[test]
    fn ctrl_l_noop_when_detail_open() {
        use crate::export::ExportSource;
        use std::sync::mpsc;

        let mut app = App::new(100);
        app.export_source = ExportSource::Hdc { device: None };
        let (tx, rx) = mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        app.toggle_detail_fields();
        assert!(app.detail_open());

        try_handle_ctrl_l(&mut app);
        assert_eq!(app.rows().len(), 1);
        assert_ne!(app.status_msg.as_deref(), Some("CLEARED"));
    }

    #[test]
    fn test_ctrl_c_in_insert_mode_cancels_input_instead_of_quitting() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.push_char('x');
        handle_ctrl_c(&mut app, &mut input);
        assert!(!app.should_quit);
        assert_eq!(app.mode, app::Mode::Normal);
        assert!(input.is_empty());
    }

    #[test]
    fn test_ctrl_c_in_insert_mode_also_returns_focus_to_loglist() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.push_char('x');
        handle_ctrl_c(&mut app, &mut input);
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(
            app.focus,
            app::Focus::LogList,
            "Ctrl+C should behave like Esc: also return focus to the log list"
        );
    }

    #[test]
    fn test_ctrl_c_with_field_popup_visible_resets_input() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.push_char('t'); // auto field popup visible
        assert!(input.field_popup_visible());
        handle_ctrl_c(&mut app, &mut input);
        assert!(input.is_empty());
        assert!(!input.field_popup_visible());
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_slash_is_literal_in_input_draft() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        handle_insert_key(&mut app, &mut input, KeyCode::Char('/')).unwrap();
        assert_eq!(input.draft, "/");
        assert!(input.field_popup_visible());
        assert!(
            input.field_candidates().is_empty(),
            "/ matches no field keyword"
        );
        assert!(input.draft_field.is_none());
    }

    #[test]
    fn test_enter_with_field_candidates_confirms_field() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        for c in "tag".chars() {
            handle_insert_key(&mut app, &mut input, KeyCode::Char(c)).unwrap();
        }
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        assert_eq!(input.draft_field, Some(input::ChipField::Tag));
        assert!(input.draft.is_empty());
        assert!(
            input.chips.is_empty(),
            "confirming a field must not commit a pill"
        );
        assert!(!input.field_popup_visible());
    }

    #[test]
    fn test_tab_with_field_candidates_confirms_field() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        handle_insert_key(&mut app, &mut input, KeyCode::Char('t')).unwrap();
        handle_insert_key(&mut app, &mut input, KeyCode::Down).unwrap(); // tid
        handle_insert_key(&mut app, &mut input, KeyCode::Tab).unwrap();
        assert_eq!(input.draft_field, Some(input::ChipField::Tid));
        assert_eq!(
            app.focus,
            app::Focus::Input,
            "Tab must not cycle focus when confirming a field"
        );
        assert_eq!(app.mode, app::Mode::Insert);
    }

    #[test]
    fn test_enter_without_field_candidates_commits_pill() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        for c in "error".chars() {
            handle_insert_key(&mut app, &mut input, KeyCode::Char(c)).unwrap();
        }
        assert!(input.field_popup_visible());
        assert!(input.field_candidates().is_empty());
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        assert_eq!(input.chips.len(), 1);
        assert_eq!(input.chips[0].field, input::ChipField::Msg);
        assert_eq!(input.chips[0].value, "error");
        assert!(!input.field_popup_visible());
    }

    #[test]
    fn test_esc_in_insert_mode_resets_input_and_returns_to_normal() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(
            app.focus,
            app::Focus::LogList,
            "Esc should also return focus to the log list"
        );
        assert!(input.is_empty());
    }

    #[test]
    fn test_enter_two_step_builds_group_and_returns_focus_to_loglist() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // commit pill
        assert_eq!(input.chips.len(), 1);
        assert_eq!(app.groups.groups.len(), 0);
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // submit group
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(
            app.mode,
            app::Mode::Normal,
            "adding a group should behave like Esc: back to Normal"
        );
        assert_eq!(
            app.focus,
            app::Focus::LogList,
            "adding a group should jump focus back to the log list"
        );
        assert!(app.following);
        assert!(input.chips.is_empty());
    }

    #[test]
    fn test_enter_with_empty_input_focuses_loglist() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        assert!(
            app.groups.groups.is_empty(),
            "empty draft must not build a group"
        );
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(
            app.focus,
            app::Focus::LogList,
            "empty Enter switches focus to LogList"
        );
    }

    #[test]
    fn test_tab_in_insert_cycles_focus_and_normal_tab_cycles_visible_only() {
        use crate::filter_model::Group;
        use crate::highlight_model::HighlightGroup;
        use crate::input::{Chip, ChipField};

        // Insert Tab still cycles through every region (Input is a valid
        // editing target there); draft is preserved.
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Tab).unwrap();
        assert_eq!(app.focus, app::Focus::ChipStrip);
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(input.draft, "x");

        // Normal Tab with all strips empty: only LogList is visible, so Tab
        // stays on LogList and never opens the unified picker.
        handle_normal_key(&mut app, &mut input, KeyCode::Tab);
        assert_eq!(app.focus, app::Focus::LogList);
        assert!(app.picker.is_none());

        // Populate all three strips so they become visible.
        app.groups.groups.push(Group {
            label: "g".into(),
            chips: Vec::new(),
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        assert!(app
            .groups
            .push_exclude(Chip {
                field: ChipField::Tag,
                value: "noise".into(),
            })
            .unwrap());
        app.highlight_groups
            .groups
            .push(HighlightGroup::from_pattern("err").unwrap());

        // From LogList, BackTab lands on HighlightStrip (last visible region).
        handle_normal_key(&mut app, &mut input, KeyCode::BackTab);
        assert_eq!(app.focus, app::Focus::HighlightStrip);
        // Forward cycle wraps to the head then advances:
        // Highlight → LogList → ChipStrip → ExcludeStrip → Highlight.
        handle_normal_key(&mut app, &mut input, KeyCode::Tab);
        assert_eq!(app.focus, app::Focus::LogList);
        handle_normal_key(&mut app, &mut input, KeyCode::Tab);
        assert_eq!(app.focus, app::Focus::ChipStrip);
        handle_normal_key(&mut app, &mut input, KeyCode::Tab);
        assert_eq!(app.focus, app::Focus::ExcludeStrip);
        handle_normal_key(&mut app, &mut input, KeyCode::Tab);
        assert_eq!(app.focus, app::Focus::HighlightStrip);
        // Never opens the unified picker via Tab.
        assert!(app.picker.is_none());
    }

    #[test]
    fn test_digit_in_insert_is_literal_not_focus_switch() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        handle_insert_key(&mut app, &mut input, KeyCode::Char('1')).unwrap();
        assert_eq!(app.focus, app::Focus::Input);
        assert_eq!(input.draft, "1");
    }

    #[test]
    fn test_space_is_literal_in_input_draft() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.push_char('a');
        handle_insert_key(&mut app, &mut input, KeyCode::Char(' ')).unwrap();
        input.push_char('b');
        assert_eq!(input.draft, "a b");
        assert!(input.chips.is_empty());
    }

    #[test]
    fn test_filter_build_group_ignore_case_by_default() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.set_field(input::ChipField::Tag);
        for c in "mytag".chars() {
            input.push_char(c);
        }
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // pill
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // submit
        let row =
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : m").unwrap();
        assert!(
            app.groups.matches(&row),
            "filter chips must ignore case by default"
        );
    }

    #[test]
    fn test_filter_group_dedup_skips_duplicate() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        focus_input_insert(&mut app);
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        assert_eq!(app.groups.groups.len(), 1);
        focus_input_insert(&mut app);
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        assert_eq!(
            app.groups.groups.len(),
            1,
            "duplicate filter group must not be added"
        );
    }

    #[test]
    fn test_dd_chord_requires_two_consecutive_d_presses() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.groups.groups.push(Group {
            label: "g0".into(),
            chips: Vec::new(),
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        app.focus = app::Focus::ChipStrip;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('d'));
        assert!(app.pending_d);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('j')); // unrelated key in between
        assert!(!app.pending_d, "pending_d should clear on a non-d key");
        assert_eq!(app.groups.groups.len(), 1, "group should NOT be deleted");
    }

    #[test]
    fn test_di_toggles_filter_group_disabled() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.groups.groups.push(Group {
            label: "g0".into(),
            chips: Vec::new(),
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
        app.focus = app::Focus::ChipStrip;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('d'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
        assert!(!app.groups.groups[0].enabled);
        assert_eq!(app.mode, app::Mode::Normal, "di must not enter Insert");
        assert_eq!(app.focus, app::Focus::ChipStrip);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('d'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
        assert!(app.groups.groups[0].enabled);
    }

    #[test]
    fn test_highlight_box_enter_adds_group_ignore_case() {
        let mut app = App::new(100);
        app.highlight_box.editing = true;
        for c in "ERROR".chars() {
            app.highlight_box.push_char(c);
        }
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.highlight_box.editing);
        assert_eq!(app.highlight_groups.groups.len(), 1);
        assert!(app.highlight_groups.any_match("", "an error occurred"));
    }

    #[test]
    fn test_highlight_box_enter_jumps_to_first_match() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap())
            .unwrap();
        tx.send(
            model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : findme here").unwrap(),
        )
        .unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:02.000  1  1 I T   : findme two").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = true;
        app.cursor = 2;

        app.highlight_box.editing = true;
        for c in "findme".chars() {
            app.highlight_box.push_char(c);
        }
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.cursor, 1);
        assert!(!app.following);
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_highlight_box_space_is_literal_single_enter_submits() {
        let mut app = App::new(100);
        app.highlight_box.editing = true;
        for c in "foo bar".chars() {
            app.highlight_box.push_char(c);
        }
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.highlight_groups.groups.len(), 1);
        assert_eq!(app.highlight_groups.groups[0].pattern, "foo bar");
        assert_eq!(app.highlight_groups.active_patterns().len(), 1);
    }

    #[test]
    fn test_highlight_box_duplicate_jumps_without_adding() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap())
            .unwrap();
        tx.send(
            model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : findme here").unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;

        app.highlight_box.editing = true;
        for c in "findme".chars() {
            app.highlight_box.push_char(c);
        }
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.highlight_groups.groups.len(), 1);
        assert_eq!(app.cursor, 1);

        app.cursor = 0;
        app.highlight_box.editing = true;
        for c in "FINDME".chars() {
            app.highlight_box.push_char(c);
        }
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.highlight_groups.groups.len(),
            1,
            "duplicate must not add another group"
        );
        assert_eq!(app.cursor, 1, "duplicate still jumps to first match");
    }

    #[test]
    fn test_g_jump_bottom_resumes_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
            .unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : b").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-1);
        assert!(!app.following);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('G'));
        assert_eq!(app.cursor, 1);
        assert!(app.following, "G jumps to bottom and resumes following");
    }

    #[test]
    fn test_j_to_bottom_resumes_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
            .unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : b").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-1);
        assert!(!app.following);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('j'));
        assert_eq!(app.cursor, 1);
        assert!(app.following, "j to bottom resumes following");
    }

    #[test]
    fn test_shift_j_to_bottom_resumes_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..10 {
            tx.send(
                model::EntryRow::from_line(&format!(
                    "04-02 10:00:00.000  1  1 I Tag     : line{i}"
                ))
                .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.cursor = 5;
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('J'));
        assert_eq!(app.cursor, 9);
        assert!(app.following, "J landing on bottom resumes following");
    }

    #[test]
    fn test_highlight_box_ctrl_c_cancels_without_adding() {
        let mut app = App::new(100);
        app.highlight_box.editing = true;
        app.highlight_box.push_char('x');
        handle_highlight_box_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert!(!app.highlight_box.editing);
        assert!(app.highlight_groups.groups.is_empty());
    }

    #[test]
    fn test_highlight_box_literal_metacharacters_do_not_drop_prior_groups() {
        // Patterns are literal (regex-escaped); metacharacters are valid input.
        let mut app = App::new(100);
        app.highlight_groups
            .groups
            .push(highlight_model::HighlightGroup::from_pattern("existing").unwrap());
        app.highlight_box.editing = true;
        for c in "(unclosed".chars() {
            app.highlight_box.push_char(c);
        }
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.highlight_box.editing);
        assert_eq!(app.highlight_groups.groups.len(), 2);
        assert!(app.highlight_groups.any_match("", "existing"));
        assert!(app.highlight_groups.any_match("", "see (unclosed here"));
    }

    #[test]
    fn test_highlight_box_esc_and_enter_return_focus_to_loglist() {
        let mut app = App::new(100);
        app.focus = app::Focus::Input;
        app.highlight_box.editing = true;
        app.highlight_box.push_char('x');
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.focus, app::Focus::LogList);

        app.focus = app::Focus::ChipStrip;
        app.highlight_box.editing = true;
        app.highlight_box.push_char('y');
        handle_highlight_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_mouse_scroll_down_and_up_move_cursor_by_three() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..20 {
            tx.send(
                crate::model::EntryRow::from_line(&format!(
                    "04-02 10:00:00.000  1  1 I Tag     : line{i}"
                ))
                .unwrap(),
            )
            .unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.cursor = 10;
        app.following = false;

        handle_mouse_event(
            &mut app,
            event::MouseEvent {
                kind: event::MouseEventKind::ScrollUp,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.cursor, 7);

        handle_mouse_event(
            &mut app,
            event::MouseEvent {
                kind: event::MouseEventKind::ScrollDown,
                column: 0,
                row: 0,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.cursor, 10);
    }

    #[test]
    fn test_mouse_click_is_ignored() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : a").unwrap(),
        )
        .unwrap();
        tx.send(
            crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I Tag     : b").unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        let cursor_before = app.cursor;

        handle_mouse_event(
            &mut app,
            event::MouseEvent {
                kind: event::MouseEventKind::Down(event::MouseButton::Left),
                column: 5,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
        );
        assert_eq!(app.cursor, cursor_before, "clicks are ignored in this pass");
    }

    fn drain_lines(app: &mut App, lines: &[&str]) {
        let (tx, rx) = std::sync::mpsc::channel();
        for line in lines {
            tx.send(crate::model::EntryRow::from_line(line).unwrap())
                .unwrap();
        }
        drop(tx);
        app.drain(&rx);
    }

    #[test]
    fn test_yy_yanks_raw_and_clears_pending() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let line = "04-02 10:00:00.000  1  1 I Tag     : hello";
        drain_lines(&mut app, &[line]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        assert!(app.pending_yank);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        assert!(!app.pending_yank);
        assert_eq!(app.last_yanked.as_deref(), Some(line));
    }

    #[test]
    fn test_yt_and_ym_yank_fields() {
        use crate::picker::{MsgChipPurpose, PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &["04-02 10:00:00.000  1  1 I MyTag   : hello boom world"],
        );
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert_eq!(app.last_yanked.as_deref(), Some("MyTag"));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert!(matches!(
            app.picker.as_ref().map(|p| (&p.kind, &p.mode)),
            Some((
                PickerKind::MsgChip {
                    purpose: MsgChipPurpose::Yank
                },
                PickerMode::New
            ))
        ));
        for c in "boom".chars() {
            handle_picker_key(
                &mut app,
                event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none());
        assert_eq!(app.last_yanked.as_deref(), Some("boom"));
    }

    #[test]
    fn test_capital_y_yanks_msg() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : onlymsg"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('Y'));
        assert_eq!(app.last_yanked.as_deref(), Some("onlymsg"));
        assert!(!app.pending_yank);
    }

    #[test]
    fn test_yank_pending_esc_cancels() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert!(!app.pending_yank);
        assert!(app.last_yanked.is_none());
    }

    #[test]
    fn test_visual_line_shift_v_jk_y_yanks_range() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let a = "04-02 10:00:00.000  1  1 I Tag     : a";
        let b = "04-02 10:00:01.000  1  1 I Tag     : b";
        let c = "04-02 10:00:02.000  1  1 I Tag     : c";
        drain_lines(&mut app, &[a, b, c]);
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('V'));
        assert_eq!(app.visual_anchor, Some(0));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('j'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('j'));
        assert_eq!(app.cursor, 2);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        assert!(app.visual_anchor.is_none());
        let expected = format!("{a}\n{b}\n{c}");
        assert_eq!(app.last_yanked.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn test_n_and_shift_n_jump_highlight_matches() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : aaa",
                "04-02 10:00:01.000  1  1 I Tag     : hit one",
                "04-02 10:00:02.000  1  1 I Tag     : bbb",
                "04-02 10:00:03.000  1  1 I Tag     : hit two",
            ],
        );
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(
            highlight_model::HighlightGroup::from_pattern("hit").unwrap(),
        );
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 3);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 3);
        assert_eq!(app.status_msg.as_deref(), Some("NO MORE"));
        app.status_msg = None;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('N'));
        assert_eq!(app.cursor, 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('N'));
        assert_eq!(app.cursor, 1);
        assert_eq!(app.status_msg.as_deref(), Some("NO MORE"));
    }

    #[test]
    fn test_e_and_shift_e_jump_severe_rows() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : aaa",
                "04-02 10:00:01.000  1  1 E Tag     : err one",
                "04-02 10:00:02.000  1  1 I Tag     : bbb",
                "04-02 10:00:03.000  1  1 F Tag     : fatal",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('e'));
        assert_eq!(app.cursor, 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('e'));
        assert_eq!(app.cursor, 3);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('e'));
        assert_eq!(app.cursor, 3);
        assert_eq!(app.status_msg.as_deref(), Some("NO MORE"));
        app.status_msg = None;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('E'));
        assert_eq!(app.cursor, 1);
        assert_ne!(app.status_msg.as_deref(), Some("NO ERROR"));
    }

    #[test]
    fn test_e_sets_no_error_when_no_severe() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : aaa"]);
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('e'));
        assert_eq!(app.cursor, 0);
        assert_eq!(app.status_msg.as_deref(), Some("NO ERROR"));
    }

    #[test]
    fn test_e_does_not_interfere_with_search_n() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : hit info",
                "04-02 10:00:01.000  1  1 E Tag     : other err",
                "04-02 10:00:02.000  1  1 I Tag     : hit two",
            ],
        );
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(
            highlight_model::HighlightGroup::from_pattern("hit").unwrap(),
        );
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 2, "n follows search, not severe");
        // Severe is behind the cursor; forward `e` no longer wraps.
        handle_normal_key(&mut app, &mut input, KeyCode::Char('E'));
        assert_eq!(app.cursor, 1, "E follows severe, not search");
    }

    #[test]
    fn test_ct_pushes_tag_chip_and_narrows_visible() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Keep    : a",
                "04-02 10:00:01.000  1  1 I Drop    : b",
                "04-02 10:00:02.000  1  1 I Keep    : c",
            ],
        );
        app.following = true;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        assert!(app.pending_chip);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert!(!app.pending_chip);
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.groups.groups[0].chips[0].value, "Keep");
        assert_eq!(app.visible.len(), 2);
        assert!(!app.following, "chip-from-cursor leaves following off");
        assert_eq!(app.status_msg.as_deref(), Some("FILTER"));
    }

    #[test]
    fn test_cl_uses_level_gte_semantics() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : info",
                "04-02 10:00:01.000  1  1 W Tag     : warn",
                "04-02 10:00:02.000  1  1 E Tag     : err",
                "04-02 10:00:03.000  1  1 F Tag     : fatal",
            ],
        );
        app.following = false;
        app.cursor = 1; // W
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('l'));
        assert_eq!(app.visible.len(), 3, "level>=W keeps W/E/F");
        let levels: Vec<_> = app
            .visible_rows()
            .iter()
            .map(|r| r.level.as_char())
            .collect();
        assert_eq!(levels, vec!['W', 'E', 'F']);
    }

    #[test]
    fn test_ct_duplicate_does_not_push() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I MyTag   : x"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert_eq!(app.groups.groups.len(), 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.status_msg.as_deref(), Some("EXISTS"));
    }

    #[test]
    fn test_cm_opens_picker_enter_pushes_token() {
        use crate::picker::{MsgChipPurpose, PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : hello timeout world",
                "04-02 10:00:01.000  1  1 I Tag     : other",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert!(matches!(
            app.picker
                .as_ref()
                .map(|picker| (&picker.kind, &picker.mode)),
            Some((
                PickerKind::MsgChip {
                    purpose: MsgChipPurpose::Chip { exclude: false }
                },
                PickerMode::New
            ))
        ));
        // filter to "timeout"
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char('t'), event::KeyModifiers::NONE),
        );
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(matches!(
            app.picker.as_ref().map(|p| &p.kind),
            Some(PickerKind::ActionList { value }) if value == "timeout"
        ));
        assert_eq!(app.picker.as_ref().unwrap().selected, 0);
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none());
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.groups.groups[0].chips[0].value, "timeout");
        assert_eq!(app.visible.len(), 1);
    }

    #[test]
    fn test_cm_draft_fallback_when_no_candidate() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : hello world",
                "04-02 10:00:01.000  1  1 I Tag     : customxyz",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        for c in "customxyz".chars() {
            handle_picker_key(
                &mut app,
                event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
            );
        }
        assert!(picker_render_data(&app, 10, 80).unwrap().labels.is_empty());
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(matches!(
            app.picker.as_ref().map(|p| &p.kind),
            Some(crate::picker::PickerKind::ActionList { value }) if value == "customxyz"
        ));
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert_eq!(app.groups.groups[0].chips[0].value, "customxyz");
        assert_eq!(app.visible.len(), 1);
    }

    #[test]
    fn test_cm_picker_esc_closes_instead_of_opening_manage_mode() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &["04-02 10:00:00.000  1  1 I Tag     : hello world"],
        );
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none());
    }

    #[test]
    fn test_capital_cm_picker_pushes_exclude_token() {
        use crate::picker::PickerKind;

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : noisy timeout",
                "04-02 10:00:01.000  1  1 I Tag     : useful",
            ],
        );
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('C'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert!(matches!(
            app.picker.as_ref().map(|picker| &picker.kind),
            Some(PickerKind::MsgChip {
                purpose: crate::picker::MsgChipPurpose::Chip { exclude: true }
            })
        ));
        for c in "timeout".chars() {
            handle_picker_key(
                &mut app,
                event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none(), "Cm skips ActionList");
        assert_eq!(app.groups.excludes.len(), 1);
        assert_eq!(app.groups.excludes[0].chip.value, "timeout");
        assert_eq!(app.visible.len(), 1);
    }

    #[test]
    fn test_ym_draft_fallback_yanks_draft() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &["04-02 10:00:00.000  1  1 I Tag     : hello world"],
        );
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        for c in "customxyz".chars() {
            handle_picker_key(
                &mut app,
                event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none());
        assert_eq!(app.last_yanked.as_deref(), Some("customxyz"));
    }

    #[test]
    fn test_cm_action_list_highlight() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : hello timeout world",
                "04-02 10:00:01.000  1  1 I Tag     : timeout again",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        for c in "timeout".chars() {
            handle_picker_key(
                &mut app,
                event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        // Input starts empty (not prefilled with the msg token).
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert_eq!(data.title, "Create");
        assert!(data.text.is_empty(), "query must not carry msg token");
        assert_eq!(data.labels, vec!["Filter", "Highlight"]);
        // Substring search (not fuzzy): type "h" → only Highlight.
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char('h'), event::KeyModifiers::NONE),
        );
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert_eq!(data.text, "h");
        assert_eq!(data.labels, vec!["Highlight"]);
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none());
        assert!(app.groups.groups.is_empty());
        assert_eq!(app.highlight_groups.groups.len(), 1);
        assert_eq!(app.highlight_groups.groups[0].pattern, "timeout");
        assert_eq!(app.active_highlight, Some(0));
    }

    #[test]
    fn test_cm_action_list_esc_cancels() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &["04-02 10:00:00.000  1  1 I Tag     : hello timeout"],
        );
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        for c in "timeout".chars() {
            handle_picker_key(
                &mut app,
                event::KeyEvent::new(KeyCode::Char(c), event::KeyModifiers::NONE),
            );
        }
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(matches!(
            app.picker.as_ref().map(|p| &p.kind),
            Some(crate::picker::PickerKind::ActionList { .. })
        ));
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none());
        assert!(app.groups.groups.is_empty());
        assert!(app.highlight_groups.groups.is_empty());
        assert!(!app.following);
    }

    #[test]
    fn test_msg_token_candidates_uncapped() {
        let msg = "aa bb cc dd ee ff gg hh ii jj kk";
        let tokens = input::msg_token_candidates(msg);
        assert!(tokens.len() > 8, "got {}", tokens.len());
        assert!(tokens.contains(&"kk".to_string()));
    }

    #[test]
    fn test_chip_pending_esc_does_not_resume_following() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        assert!(app.pending_chip);
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert!(!app.pending_chip);
        assert!(
            !app.following,
            "Esc while pending_c must not resume following"
        );
        assert!(app.groups.groups.is_empty());
    }

    #[test]
    fn test_c_unsupported_and_unknown_field() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('s'));
        assert_eq!(app.status_msg.as_deref(), Some("NO RAW/TIMESTAMP"));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('x'));
        assert_eq!(app.status_msg.as_deref(), Some("UNKNOWN FIELD"));
        assert!(app.groups.groups.is_empty());
    }

    #[test]
    fn test_fh_fe_view_focus_keys() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : hit one",
                "04-02 10:00:01.000  1  1 E Tag     : hit err",
                "04-02 10:00:02.000  1  1 E Tag     : plain err",
                "04-02 10:00:03.000  1  1 I Tag     : other",
            ],
        );
        app.push_or_find_highlight_group(
            highlight_model::HighlightGroup::from_pattern("hit").unwrap(),
        );
        app.following = true;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('h'));
        assert!(app.view_focus.highlight);
        assert!(!app.view_focus.severe);
        assert_eq!(app.visible.len(), 2);
        assert!(!app.following);

        // fe stacks: intersection of highlight ∩ severe.
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('e'));
        assert!(app.view_focus.highlight);
        assert!(app.view_focus.severe);
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.current_row().unwrap().msg, "hit err");

        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert!(app.following);
        assert!(app.view_focus.highlight && app.view_focus.severe);

        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('e'));
        assert!(app.view_focus.highlight);
        assert!(!app.view_focus.severe);
        assert_eq!(app.visible.len(), 2);

        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('h'));
        assert!(!app.view_focus.is_active());
        assert_eq!(app.visible.len(), 4);
    }

    #[test]
    fn test_fp_locks_pid_and_ft_clears_pid_lock() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  111  1 I Tag     : a",
                "04-02 10:00:01.000  222  2 I Tag     : b",
                "04-02 10:00:02.000  111  3 I Tag     : c",
            ],
        );
        app.following = true;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        assert!(app.pending_lock);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        assert_eq!(app.lock_pid.as_deref(), Some("111"));
        assert!(app.lock_tid.is_none());
        assert_eq!(app.visible.len(), 2);
        assert!(app.following, "lock coexists with following");
        assert_eq!(app.lock_badge_label().as_deref(), Some("pid=111"));

        app.cursor = 0; // still on a pid=111 row after rebuild+follow
                        // switch to tid lock on current row (tid=1 on first visible)
        let tid = app.current_row().unwrap().tid.clone();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert!(app.lock_pid.is_none(), "tid lock clears pid lock");
        assert_eq!(app.lock_tid.as_deref(), Some(tid.as_str()));
    }

    #[test]
    fn test_fp_toggle_same_value_and_fu_clear() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  111  1 I Tag     : a",
                "04-02 10:00:01.000  222  2 I Tag     : b",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        assert_eq!(app.visible.len(), 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        assert!(app.lock_pid.is_none());
        assert_eq!(app.visible.len(), 2);
        assert_eq!(app.status_msg.as_deref(), Some("UNLOCK"));

        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('u'));
        assert!(app.lock_pid.is_none());
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn test_esc_resume_following_keeps_lock() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  111  1 I Tag     : a",
                "04-02 10:00:01.000  222  2 I Tag     : b",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        assert_eq!(app.lock_pid.as_deref(), Some("111"));
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert!(app.following);
        assert_eq!(app.lock_pid.as_deref(), Some("111"));
        assert_eq!(app.visible.len(), 1);
    }

    #[test]
    fn test_lock_pending_esc_does_not_resume_or_clear_lock() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  111  1 I Tag     : a"]);
        app.following = false;
        app.lock_pid = Some("111".into());
        handle_normal_key(&mut app, &mut input, KeyCode::Char('f'));
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert!(!app.pending_lock);
        assert!(!app.following);
        assert_eq!(app.lock_pid.as_deref(), Some("111"));
    }

    #[test]
    fn test_ct_exclude_hides_tag_and_dd_clears() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Keep    : a",
                "04-02 10:00:01.000  1  1 I Spam    : b",
                "04-02 10:00:02.000  1  1 I Keep    : c",
            ],
        );
        app.following = false;
        app.cursor = 1; // Spam
        handle_normal_key(&mut app, &mut input, KeyCode::Char('C'));
        assert!(app.pending_exclude);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert_eq!(app.groups.excludes.len(), 1);
        assert!(app.groups.excludes[0]
            .chip
            .value
            .eq_ignore_ascii_case("Spam"));
        assert_eq!(app.visible.len(), 2);
        app.focus = app::Focus::ExcludeStrip;
        app.exclude_cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('d'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('d'));
        assert!(app.groups.excludes.is_empty());
        assert_eq!(app.focus, app::Focus::LogList);
        assert_eq!(app.visible.len(), 3);
    }

    #[test]
    fn test_exclude_di_restores_visibility() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Spam    : a",
                "04-02 10:00:01.000  1  1 I Keep    : b",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('C'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert_eq!(app.visible.len(), 1);
        app.focus = app::Focus::ExcludeStrip;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('d'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('i'));
        assert!(!app.groups.excludes[0].enabled);
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn test_input_bang_exclude_mode_submit() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Noise   : a",
                "04-02 10:00:01.000  1  1 I Keep    : b",
            ],
        );
        focus_input_insert(&mut app);
        handle_insert_key(&mut app, &mut input, KeyCode::Char('!')).unwrap();
        assert!(input.exclude_mode);
        // pick tag field via draft prefix
        input.push_char('t');
        input.push_char('a');
        assert!(input.confirm_field_candidate());
        for c in "Noise".chars() {
            input.push_char(c);
        }
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // commit pill
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // submit excludes
        assert_eq!(app.groups.excludes.len(), 1);
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.view_source()[app.source_idx_for_visible(0).unwrap()].tag,
            "Keep"
        );
    }

    #[test]
    fn test_exclude_strip_height_zero_when_empty() {
        let app = App::new(100);
        assert_eq!(ui::exclude_strip_height(&app, 80), 0);
    }

    #[test]
    fn test_p_toggles_detail_fields() {
        use crate::app::DetailView;
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I TagA    : one",
                "04-02 10:00:01.000  2  2 E TagB    : two",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        assert_eq!(app.detail, DetailView::Fields);
        let lines = ui::detail_field_lines(app.current_row().as_deref(), 40);
        let joined: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("TagA"), "detail shows selected tag");
        handle_normal_key(&mut app, &mut input, KeyCode::Char('j'));
        let lines2 = ui::detail_field_lines(app.current_row().as_deref(), 40);
        let joined2: String = lines2
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined2.contains("TagB"), "detail follows cursor");
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        assert_eq!(app.detail, DetailView::Closed);
    }

    #[test]
    fn test_w_toggles_collapsed_view() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : hello"]);
        assert!(
            !app.collapsed_view,
            "collapsed_view must default to false (multi-line)"
        );
        let cursor_before = app.cursor;
        let following_before = app.following;
        let list_offset_before = app.list_offset;

        handle_normal_key(&mut app, &mut input, KeyCode::Char('w'));
        assert!(app.collapsed_view, "w must toggle into collapsed view");
        assert_eq!(app.cursor, cursor_before, "w must not move cursor");
        assert_eq!(
            app.following, following_before,
            "w must not change following"
        );
        assert_eq!(
            app.list_offset, list_offset_before,
            "w must not change list_offset"
        );

        handle_normal_key(&mut app, &mut input, KeyCode::Char('w'));
        assert!(!app.collapsed_view, "w must toggle back to multi-line");
    }

    #[test]
    fn test_detail_esc_closes_without_resume_following() {
        use crate::app::DetailView;
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        assert_eq!(app.detail, DetailView::Fields);
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert_eq!(app.detail, DetailView::Closed);
        assert!(!app.following, "Esc must not resume_following");
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_detail_c_field_still_works() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Keep    : a",
                "04-02 10:00:01.000  1  1 I Drop    : b",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.visible.len(), 1);
        assert!(app.detail_open(), "c+t keeps detail open");
    }

    #[test]
    fn test_P_opens_pretty_and_switches_with_fields() {
        use crate::app::DetailView;
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &["04-02 10:00:00.000  1  1 I Tag     : {\"a\":1,\"b\":[2,3]}"],
        );
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('P'));
        assert_eq!(app.detail, DetailView::Pretty);
        let (text, ok) = ui::pretty_json_for_row(&app.current_row().unwrap());
        assert!(ok);
        assert!(text.contains('\n'), "pretty should indent");
        handle_normal_key(&mut app, &mut input, KeyCode::Char('P'));
        assert_eq!(app.detail, DetailView::Fields);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('P'));
        assert_eq!(app.detail, DetailView::Pretty);
        handle_normal_key(&mut app, &mut input, KeyCode::Esc);
        assert_eq!(app.detail, DetailView::Closed);
        assert!(!app.following);
    }

    #[test]
    fn test_pretty_non_json_shows_note() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &["04-02 10:00:00.000  1  1 I Tag     : not-json-at-all"],
        );
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('P'));
        let lines = ui::detail_pretty_lines(app.current_row().as_deref(), 40);
        let joined: String = lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("非 JSON"));
        assert!(joined.contains("not-json-at-all"));
    }

    #[test]
    fn test_p_closes_pretty_mode() {
        use crate::app::DetailView;
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &["04-02 10:00:00.000  1  1 I Tag     : {\"x\":1}"],
        );
        handle_normal_key(&mut app, &mut input, KeyCode::Char('P'));
        assert_eq!(app.detail, DetailView::Pretty);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('p'));
        assert_eq!(app.detail, DetailView::Closed);
    }

    #[test]
    fn test_yc_yanks_cli_command() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.export_source = export::ExportSource::File("demo.log".into());
        drain_lines(&mut app, &["04-02 10:00:00.000  99  1 I MyTag   : hello"]);
        app.following = false;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        assert!(app.pending_yank);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        assert!(!app.pending_yank);
        let cmd = app.last_yanked.as_deref().unwrap();
        assert!(cmd.starts_with("alnav grep -f 'demo.log' -i -e "));
        assert!(cmd.contains(r#"tag ~ "MyTag""#), "{cmd}");
        // Clipboard may fail in headless/CI; flash still carries approx note on Ok.
        if let Some(msg) = app.status_msg.as_deref() {
            if !msg.starts_with("YANK FAILED") {
                assert!(
                    msg.contains("approx"),
                    "yc success flash must note approx export: {msg}"
                );
            }
        }
    }

    #[test]
    fn test_tt_opens_panel_in_file_mode_and_tu_clears() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.export_source = export::ExportSource::File("demo.log".into());
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Tag     : a",
                "04-02 12:00:00.000  1  1 I Tag     : b",
            ],
        );
        app.following = true;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert!(app.pending_time);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert!(app.time_panel.is_some());
        assert!(!app.following, "tt opens with following=false");
        assert!(!app.pending_time);

        // Submit a one-sided since window via panel API (key path covered above).
        app.apply_time_bound(crate::filter_model::TimeBound {
            since: Some("04-02 11:00:00".into()),
            until: None,
        });
        assert!(app.filter_active());
        assert_eq!(app.visible.len(), 1);
        assert!(app.export_cli_command().contains("--since "));

        app.following = true;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('u'));
        assert!(app.time_bound.is_none());
        assert!(!app.following);
        assert_eq!(app.visible.len(), 2);
        assert_eq!(app.status_msg.as_deref(), Some("TIME CLEARED"));
    }

    #[test]
    fn test_tt_empty_candidates_flashes_and_live_modes_hide_t() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.export_source = export::ExportSource::File("demo.log".into());
        // No rows → no date candidates.
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert!(app.time_panel.is_none());
        assert_eq!(app.status_msg.as_deref(), Some("NO DATES"));

        // Abandoned `ts` → UNKNOWN, panel stays closed.
        app.status_msg = None;
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : a"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('s'));
        assert!(app.time_panel.is_none());
        assert_eq!(app.status_msg.as_deref(), Some("UNKNOWN"));

        // Live modes: bare `t` must not arm time operator.
        app.export_source = export::ExportSource::Hdc { device: None };
        app.status_msg = None;
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : a"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert!(!app.pending_time);
        assert!(app.time_panel.is_none());

        app.export_source = export::ExportSource::Adb { device: None };
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert!(!app.pending_time);
        assert!(app.time_panel.is_none());
    }

    #[test]
    fn test_time_panel_ctrl_c_cancels_without_apply() {
        let mut app = App::new(100);
        app.export_source = export::ExportSource::File("demo.log".into());
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : a"]);
        app.time_bound = Some(crate::filter_model::TimeBound {
            since: Some("04-02 10:00:00".into()),
            until: None,
        });
        assert!(app.open_time_panel());
        handle_time_panel_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        );
        assert!(app.time_panel.is_none());
        assert_eq!(
            app.time_bound.as_ref().and_then(|t| t.since.as_deref()),
            Some("04-02 10:00:00"),
            "Ctrl+C must not clear the already-applied bound"
        );
    }

    #[test]
    fn test_ma_md_bookmark_and_leader_opens_picker() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I TagA    : first",
                "04-02 10:00:01.000  1  1 I TagB    : second",
            ],
        );
        app.following = false;
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert!(app.pending_m);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        assert_eq!(app.bookmarks.len(), 1);
        assert_eq!(app.status_msg.as_deref(), Some("BOOKMARKED"));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        assert_eq!(app.bookmarks.len(), 1);
        assert_eq!(app.status_msg.as_deref(), Some("EXISTS"));
        app.cursor = 1;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert!(matches!(
            app.picker.as_ref().map(|picker| &picker.kind),
            Some(crate::picker::PickerKind::Bookmark)
        ));
        assert_eq!(
            app.picker.as_ref().map(|picker| &picker.mode),
            Some(&crate::picker::PickerMode::Manage)
        );
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Esc, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none());
        assert!(!app.following);
        app.cursor = 0;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('d'));
        assert!(app.bookmarks.is_empty());
        assert_eq!(app.status_msg.as_deref(), Some("REMOVED"));
    }

    #[test]
    fn bookmark_panel_enter_jumps_to_row_and_closes() {
        // mm opens the bookmark-only Manage panel (F2); Enter jumps to the
        // selected bookmark's row, closes the panel, and focuses LogList.
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I TagA    : first",
                "04-02 10:00:01.000  1  1 I TagB    : second",
            ],
        );
        app.following = false;
        app.cursor = 0;
        app.bookmark_add_current();
        // Move cursor off the bookmark, then open the panel and jump back.
        app.cursor = 1;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert_eq!(
            app.picker.as_ref().unwrap().mode,
            crate::picker::PickerMode::Manage
        );
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_none(), "Enter closes the bookmark panel");
        assert_eq!(app.focus, app::Focus::LogList);
        // Jumped back to the bookmarked row (visible index 0).
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn bookmark_panel_tab_is_noop() {
        // The bookmark panel disables Tab multi-select entirely (F2).
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        app.bookmark_add_current();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        let before = app.picker.as_ref().unwrap().checked.len();
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Tab, event::KeyModifiers::NONE),
        );
        // Tab neither toggles a check nor errors.
        assert_eq!(app.picker.as_ref().unwrap().checked.len(), before);
    }

    #[test]
    fn bookmark_panel_delete_confirms_and_deletes() {
        // Delete / Ctrl-Backspace arms a DeleteBookmark confirm; confirming removes it.
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        app.bookmark_add_current();
        assert_eq!(app.bookmarks.len(), 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Delete, event::KeyModifiers::NONE),
        );
        assert!(app.picker.as_ref().unwrap().confirm.is_some());
        // Confirm with Enter.
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(app.bookmarks.is_empty(), "bookmark deleted on confirm");
        assert!(app.bookmark_row_ids.is_empty(), "row-id cache cleared");
    }

    #[test]
    fn bookmark_panel_ctrl_x_stays_manage() {
        // The bookmark panel does not support edit (F2); Ctrl-X flashes and stays.
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        app.bookmark_add_current();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        let mode_before = app.picker.as_ref().unwrap().mode.clone();
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char('x'), event::KeyModifiers::CONTROL),
        );
        // Still Manage (no Edit transition).
        assert_eq!(app.picker.as_ref().unwrap().mode, mode_before);
        assert_eq!(app.status_msg.as_deref(), Some("BOOKMARKS NOT EDITABLE"));
    }

    #[test]
    fn bookmark_row_ids_cache_synced_on_add_and_remove() {
        // F1: the O(1) bookmark-row cache stays in sync with the bookmark list.
        let mut app = App::new(100);
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I TagA    : first",
                "04-02 10:00:01.000  1  1 I TagB    : second",
            ],
        );
        let rid0 = app.view_source()[app.source_idx_for_visible(0).unwrap()].row_id;
        app.cursor = 0;
        app.bookmark_add_current();
        assert!(app.is_bookmark_row(rid0));
        assert!(app.bookmark_row_ids.contains(&rid0));
        app.bookmark_remove_current();
        assert!(!app.is_bookmark_row(rid0));
        assert!(!app.bookmark_row_ids.contains(&rid0));
    }

    #[test]
    fn unified_picker_has_no_bookmark_items() {
        // AC2/F2: the aggregated panel no longer lists bookmarks.
        let mut app = App::new(100);
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        app.cursor = 0;
        app.bookmark_add_current();
        let items = unified_picker_items(&app);
        // No item label carries a bookmark-style prefix, and the bookmark
        // still exists in the session (it just isn't surfaced in the panel).
        assert!(items.iter().all(|i| !i.label.starts_with("[Bookmark]")));
        assert!(items
            .iter()
            .all(|i| i.id.kind != crate::picker::UnifiedKind::Filter
                || i.label.starts_with("[Filter]")));
        assert_eq!(
            app.bookmarks.len(),
            1,
            "bookmark still exists outside panel"
        );
    }

    #[test]
    fn bookmark_panel_render_data_has_jump_actions_and_detail_preview() {
        // Bookmark panel: Jump icons + Fields detail preview for the selected row.
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &["04-02 10:00:00.000  1  1 I TagA    : hello detail"],
        );
        app.bookmark_add_current();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert!(
            data.actions
                .iter()
                .all(|a| matches!(a, crate::ui::ActionKind::Jump)),
            "all bookmark rows get a Jump action"
        );
        assert!(data.show_preview, "bookmark panel shows preview by default");
        let row = data
            .detail_row
            .clone()
            .flatten()
            .expect("alive bookmark yields detail row");
        assert_eq!(row.tag, "TagA");
        assert!(row.msg.contains("hello detail"));
        assert_eq!(data.empty_msg, "无书签");
    }

    #[test]
    fn bookmark_panel_detail_preview_stale_row() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        app.bookmarks
            .try_add(crate::bookmark::Bookmark {
                row_id: 999_999,
                label: "stale label".into(),
            })
            .unwrap();
        app.bookmark_row_ids.insert(999_999);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert!(data.show_preview);
        assert!(
            matches!(data.detail_row, Some(None)),
            "missing bookmark row yields empty detail"
        );
    }

    #[test]
    fn test_ma_does_not_enter_insert() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(app.focus, app::Focus::LogList);
        assert_eq!(app.bookmarks.len(), 1);
    }

    #[test]
    fn test_leader_space_opens_unified_picker() {
        use crate::picker::PickerKind;

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I Tag     : x"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        assert_eq!(
            app.picker.as_ref().map(|picker| &picker.kind),
            Some(&PickerKind::Unified)
        );
    }

    #[test]
    fn filter_picker_new_submit_returns_focus_to_loglist() {
        use crate::input::{Chip, ChipField};
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        app.focus = app::Focus::ChipStrip;
        app.open_picker_new(PickerKind::Filter);
        assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
        let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
        input.chips.push(Chip {
            field: ChipField::Tag,
            value: "Foo".into(),
        });
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.groups.groups.len(), 1);
        assert!(app.picker.is_none());
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn exclude_picker_new_submit_returns_focus_to_loglist() {
        use crate::input::{Chip, ChipField};
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        app.focus = app::Focus::ExcludeStrip;
        app.open_picker_new(PickerKind::Exclude);
        assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
        let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
        input.chips.push(Chip {
            field: ChipField::Tag,
            value: "noise".into(),
        });
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(app.groups.excludes.len(), 1);
        assert!(app.picker.is_none());
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn picker_render_data_filter_shows_tag_vocab_after_field_pick() {
        use std::sync::mpsc;
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("01-01 00:00:00.000  1 1 I TargetTag: msg").unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);

        app.open_picker_new(crate::picker::PickerKind::Filter);
        {
            let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
            input.set_field(crate::input::ChipField::Tag);
        }
        app.flush_vocab_match();

        let data = picker_render_data(&app, 10, 80).unwrap();
        assert!(
            data.labels.iter().any(|l| l == "TargetTag"),
            "Tag vocab should appear in labels after field pick, got: {:?}",
            data.labels
        );
    }

    #[test]
    fn picker_render_data_filter_level_static_candidates() {
        let mut app = App::new(100);
        app.open_picker_new(crate::picker::PickerKind::Filter);
        {
            let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
            input.set_field(crate::input::ChipField::Level);
        }
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert!(data.labels.contains(&"W".to_string()));
        assert!(data.labels.contains(&"E".to_string()));
    }

    #[test]
    fn picker_render_data_unified_never_shows_preview() {
        use crate::picker::PickerKind;
        let mut app = App::new(100);
        app.open_picker(PickerKind::Unified);
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert!(
            !data.show_preview,
            "Unified picker must never render preview"
        );
        assert!(data.preview.is_empty());
    }

    #[test]
    fn picker_render_data_filter_new_empty_draft_has_empty_preview() {
        use crate::picker::PickerKind;
        let mut app = App::new(100);
        app.open_picker_new(PickerKind::Filter);
        let data = picker_render_data(&app, 10, 80).unwrap();
        // draft_field == None, draft empty: field keyword candidates shown, preview empty
        assert!(data.draft_field.is_none());
        assert!(data.preview.is_empty());
        assert!(data.show_preview, "Filter picker still shows preview pane");
        // field keywords should appear immediately without typing
        assert!(data.labels.iter().any(|l| l == "tag"));
    }

    #[test]
    fn picker_render_data_filter_new_field_set_empty_draft_has_empty_preview() {
        use crate::input::ChipField;
        use crate::picker::PickerKind;
        use std::sync::mpsc;
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(
            crate::model::EntryRow::from_line("01-01 00:00:00.000  1 1 I TargetTag: msg").unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);
        app.open_picker_new(PickerKind::Filter);
        {
            let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
            input.set_field(ChipField::Tag);
        }
        // field set but draft still empty: no preview content yet
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert!(data.draft_field == Some(ChipField::Tag));
        assert!(
            data.preview.is_empty(),
            "empty draft value must not preview"
        );
    }

    #[test]
    fn replace_last_token_middle() {
        assert_eq!(replace_last_token("MyApp tim", "timeout"), "MyApp timeout");
    }

    #[test]
    fn replace_last_token_no_space() {
        assert_eq!(replace_last_token("timeout", "error"), "error");
    }

    #[test]
    fn picker_tab_fills_tag_vocab_candidate() {
        use std::sync::mpsc;
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(100);
        let row =
            crate::model::EntryRow::from_line("01-01 00:00:00.000  1 1 I TabTestTag: msg").unwrap();
        tx.send(row).unwrap();
        drop(tx);
        app.drain(&rx);

        app.open_picker_new(crate::picker::PickerKind::Filter);
        {
            let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
            input.set_field(crate::input::ChipField::Tag);
            input.push_char('T');
        }
        app.flush_vocab_match();

        handle_picker_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Tab,
                crossterm::event::KeyModifiers::NONE,
            ),
        );

        let draft = app
            .picker
            .as_ref()
            .unwrap()
            .input
            .as_ref()
            .unwrap()
            .draft
            .clone();
        assert_eq!(
            draft, "TabTestTag",
            "Tab should fill vocab candidate into draft"
        );
    }

    #[test]
    fn space_w_opens_save_dialog_and_space_o_applies_preset() {
        use crate::input::{build_group_from_chips, Chip, ChipField};
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let mut app = App::new(100);
        app.config_dir = dir.path().to_path_buf();
        let mut input = input::InputBox::default();
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 E MyApp   : error one",
                "04-02 10:00:01.000  1  1 I Other   : ok",
            ],
        );
        app.groups.groups.push(
            build_group_from_chips(
                vec![Chip {
                    field: ChipField::Tag,
                    value: "MyApp".into(),
                }],
                true,
            )
            .unwrap()
            .unwrap(),
        );
        app.rebuild_visible();
        app.following = true;

        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('w'));
        assert!(app.preset_name.is_some());
        {
            let d = app.preset_name.as_mut().unwrap();
            for c in "crash-login".chars() {
                d.field.insert(c);
            }
        }
        handle_preset_name_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.preset_name.is_none());
        assert!(crate::preset::exists(dir.path(), "crash-login"));
        assert_eq!(app.status_msg.as_deref(), Some("PRESET SAVED"));

        app.groups.groups.clear();
        app.rebuild_visible();
        assert_eq!(app.groups.groups.len(), 0);

        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('o'));
        assert!(matches!(
            app.picker.as_ref().map(|p| &p.kind),
            Some(crate::picker::PickerKind::Preset)
        ));
        let data = picker_render_data(&app, 10, 80).unwrap();
        assert!(data.show_preview);
        assert!(data.preset_preview.is_some());
        handle_picker_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.picker.is_none());
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.groups.groups[0].chips[0].value, "MyApp");
        assert!(!app.following);
        assert_eq!(app.status_msg.as_deref(), Some("PRESET APPLIED"));
    }

    #[test]
    fn space_o_empty_library_flashes_without_opening() {
        use tempfile::TempDir;

        let dir = TempDir::new().unwrap();
        let mut app = App::new(100);
        app.config_dir = dir.path().to_path_buf();
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char(' '));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('o'));
        assert!(app.picker.is_none());
        assert_eq!(app.status_msg.as_deref(), Some("NO PRESETS"));
    }

    #[test]
    fn apply_preset_keeps_time_lock_and_bookmarks() {
        use crate::filter_model::{GroupList, TimeBound};
        use crate::highlight_model::HighlightGroupList;
        use crate::input::{build_group_from_chips, Chip, ChipField};

        let mut app = App::new(100);
        drain_lines(&mut app, &["04-02 10:00:00.000  9  8 E MyApp   : boom"]);
        app.lock_pid = Some("9".into());
        app.time_bound = Some(TimeBound {
            since: Some("04-02 10:00:00.000".into()),
            until: None,
        });
        app.bookmark_add_current();
        let bm_len = app.bookmarks.len();

        let preset = crate::preset::capture(
            &GroupList {
                groups: vec![build_group_from_chips(
                    vec![Chip {
                        field: ChipField::Tag,
                        value: "MyApp".into(),
                    }],
                    true,
                )
                .unwrap()
                .unwrap()],
                excludes: vec![],
            },
            &HighlightGroupList::default(),
            "keep-extra",
        )
        .unwrap()
        .unwrap();
        app.apply_preset(&preset).unwrap();
        assert_eq!(app.lock_pid.as_deref(), Some("9"));
        assert!(app.time_bound.is_some());
        assert_eq!(app.bookmarks.len(), bm_len);
        assert_eq!(app.groups.groups.len(), 1);
    }
}
