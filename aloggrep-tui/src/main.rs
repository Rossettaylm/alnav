mod app;
mod filter_model;
mod ingest;
mod input;
mod model;
mod search_model;
mod theme;
mod ui;

use std::io::{self, IsTerminal};
use std::time::Duration;

use clap::Parser;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::Terminal;

use aloggrep::expr::{Expr, SameFieldOp};

use app::App;
use filter_model::{Group, GroupList, TimeBound};

/// Page size for Ctrl-d/Ctrl-u paging in the log list. `App` doesn't track
/// the rendered viewport height (that's `ui.rs`'s job at render time), so a
/// fixed page size is the simplest correct approach.
const PAGE_SIZE: isize = 10;

#[derive(Parser)]
#[command(name = "aloggrep-tui", about = "Interactive vim-style viewer for aloggrep")]
struct Cli {
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

    /// Max in-memory lines before the oldest are evicted (streaming modes)
    #[arg(long, default_value_t = 100_000)]
    max_lines: usize,

    /// Capture logs directly from hdc hilog (HarmonyOS device)
    #[arg(long)]
    hdc: bool,

    /// Device serial number (for --hdc with multiple devices)
    #[arg(long, value_name = "SERIAL")]
    device: Option<String>,
}

fn initial_group(cli: &Cli) -> Result<GroupList, String> {
    // TUI always compiles filters case-insensitively (CLI `-i` is a no-op).
    // Startup CLI multi-values keep OR (`msg:a|b` label); interactive chips use And.
    let expr = Expr::from_filters(
        &cli.tag, &cli.msg, &cli.pkg, &cli.pid, &cli.tid, cli.level.as_deref(), true, SameFieldOp::Or,
    )?;
    let time = if cli.since.is_some() || cli.until.is_some() {
        Some(TimeBound { since: cli.since.clone(), until: cli.until.clone() })
    } else {
        None
    };
    if expr.is_none() && time.is_none() {
        return Ok(GroupList::default());
    }
    let mut chips = Vec::new();
    for v in &cli.tag {
        chips.push(input::Chip { field: input::ChipField::Tag, value: v.clone() });
    }
    for v in &cli.msg {
        chips.push(input::Chip { field: input::ChipField::Msg, value: v.clone() });
    }
    for v in &cli.pkg {
        chips.push(input::Chip { field: input::ChipField::Pkg, value: v.clone() });
    }
    for v in &cli.pid {
        chips.push(input::Chip { field: input::ChipField::Pid, value: v.clone() });
    }
    for v in &cli.tid {
        chips.push(input::Chip { field: input::ChipField::Tid, value: v.clone() });
    }
    if let Some(l) = &cli.level {
        chips.push(input::Chip { field: input::ChipField::Level, value: l.clone() });
    }
    let mut label_parts = Vec::new();
    if !cli.tag.is_empty() { label_parts.push(format!("tag:{}", cli.tag.join("|"))); }
    if !cli.msg.is_empty() { label_parts.push(format!("msg:{}", cli.msg.join("|"))); }
    if !cli.pkg.is_empty() { label_parts.push(format!("pkg:{}", cli.pkg.join("|"))); }
    if !cli.pid.is_empty() { label_parts.push(format!("pid:{}", cli.pid.join("|"))); }
    if !cli.tid.is_empty() { label_parts.push(format!("tid:{}", cli.tid.join("|"))); }
    if let Some(l) = &cli.level { label_parts.push(format!("level:{l}")); }
    if let Some(s) = &cli.since { label_parts.push(format!("since:{s}")); }
    if let Some(u) = &cli.until { label_parts.push(format!("until:{u}")); }
    let label = if label_parts.is_empty() { "(startup filter)".to_string() } else { label_parts.join(" AND ") };
    Ok(GroupList {
        groups: vec![Group {
            label,
            chips,
            expr,
            time,
            enabled: true,
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        let mut full = vec!["aloggrep-tui"];
        full.extend_from_slice(args);
        Cli::parse_from(full)
    }

    #[test]
    fn test_initial_group_empty_cli_yields_empty_group_list() {
        let c = cli(&["-f", "app.log"]);
        let groups = initial_group(&c).unwrap();
        assert!(groups.groups.is_empty());
    }

    #[test]
    fn test_initial_group_pid_only_label_contains_pid() {
        let c = cli(&["-f", "app.log", "--pid", "1234"]);
        let groups = initial_group(&c).unwrap();
        assert_eq!(groups.groups.len(), 1);
        assert!(groups.groups[0].label.contains("pid:1234"), "label was: {}", groups.groups[0].label);
    }

    #[test]
    fn test_initial_group_multiple_fields_label_contains_all() {
        let c = cli(&[
            "-f", "app.log",
            "-t", "TagA",
            "-m", "boom",
            "--pkg", "com.example",
            "--pid", "111",
            "--tid", "222",
            "-l", "E",
            "--since", "10:00:00",
            "--until", "10:01:00",
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
        assert!(label.contains("since:10:00:00"), "label was: {label}");
        assert!(label.contains("until:10:01:00"), "label was: {label}");
    }
}

/// Ensures the `--hdc` child process is killed no matter which exit path
/// `main()` takes, including an early `?` bail-out between binding this
/// guard and the end of `main()`. Manual kill/wait calls threaded into every
/// fallible line are easy to miss when new fallible calls are added later;
/// `Drop` closes that gap structurally instead.
struct HdcChildGuard(Option<std::process::Child>);

impl Drop for HdcChildGuard {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn main() -> Result<(), String> {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, event::DisableMouseCapture);
        default_panic(info);
    }));

    let cli = Cli::parse();

    if cli.hdc && cli.file.is_some() {
        eprintln!("aloggrep-tui: --hdc cannot be combined with -f");
        std::process::exit(2);
    }
    if !cli.hdc && cli.file.is_none() {
        eprintln!("aloggrep-tui: either -f FILE or --hdc is required");
        std::process::exit(2);
    }

    if !io::stdout().is_terminal() {
        eprintln!("aloggrep-tui: stdout is not a terminal, refusing to start");
        std::process::exit(2);
    }

    let groups = initial_group(&cli)?;
    let mut app = App::new(cli.max_lines);
    app.groups = groups;

    let (rx, hdc_child) = if cli.hdc {
        let session = aloggrep::hdc::spawn_hilog(cli.device.as_deref())?;
        let (rx, child) = ingest::spawn_hdc_ingest(session);
        (rx, Some(child))
    } else {
        let rx = ingest::spawn_file_ingest(cli.file.clone().unwrap()).map_err(|e| e.to_string())?;
        (rx, None)
    };
    let _hdc_child_guard = HdcChildGuard(hdc_child);

    enable_raw_mode().map_err(|e| e.to_string())?;
    let setup: Result<Terminal<CrosstermBackend<io::Stdout>>, String> = (|| {
        execute!(io::stdout(), EnterAlternateScreen, event::EnableMouseCapture).map_err(|e| e.to_string())?;
        let backend = CrosstermBackend::new(io::stdout());
        Terminal::new(backend).map_err(|e| e.to_string())
    })();
    let mut terminal = match setup {
        Ok(t) => t,
        Err(e) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, event::DisableMouseCapture);
            return Err(e); // _hdc_child_guard drops here, killing the child if any
        }
    };

    let mut input = input::InputBox::default();
    let _ = cli.ignore_case; // retained for CLI compat; TUI always ignore-case
    let result = run(&mut terminal, &mut app, &mut input, &rx);

    let disable_result = disable_raw_mode().map_err(|e| e.to_string());
    let leave_result = execute!(io::stdout(), LeaveAlternateScreen, event::DisableMouseCapture).map_err(|e| e.to_string());
    disable_result.and(leave_result)?;

    result
    // _hdc_child_guard drops here at end of main(), killing the child if not already killed
}

/// Anchors the field-candidate dropdown just above the input box's top
/// border (covering the bottom of the log area), clamped to the space
/// available above the input. The region below Input is only Search+status
/// (~4 rows), which is too short for a usable candidate list.
fn popup_rect(input_area: Rect, _frame_area: Rect, match_count: usize) -> Rect {
    let desired = match_count.clamp(1, 6) as u16 + 2;
    let height = desired.min(input_area.y).max(1);
    let width = input_area.width.clamp(12, 28);
    let y = input_area.y.saturating_sub(height);
    Rect { x: input_area.x, y, width, height }
}

fn copy_to_clipboard(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut cb| cb.set_text(text.to_string()))
        .map_err(|e| e.to_string())
}

fn apply_yank(app: &mut App, text: String) {
    app.record_yank(text.clone());
    app.status_msg = match copy_to_clipboard(&text) {
        Ok(()) => Some("YANKED".into()),
        Err(e) => Some(format!("YANK FAILED: {e}")),
    };
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    input: &mut input::InputBox,
    rx: &std::sync::mpsc::Receiver<model::EntryRow>,
) -> Result<(), String> {
    use app::Mode;

    while !app.should_quit {
        app.drain(rx);

        terminal
            .draw(|frame| {
                let outer_w = frame.area().width;
                let filter_h = ui::filter_strip_height(app, outer_w);
                let search_h = ui::search_strip_height(app, outer_w);
                let [filter_area, search_strip_area, log_area, input_area, search_area, status_area] =
                    Layout::vertical([
                        Constraint::Length(filter_h),
                        Constraint::Length(search_h),
                        Constraint::Fill(1),
                        Constraint::Length(3),
                        Constraint::Length(3),
                        Constraint::Length(1),
                    ])
                    .areas(frame.area());
                ui::render_chip_strip(app, frame, filter_area);
                ui::render_search_chip_strip(app, frame, search_strip_area);
                ui::render_log_list(app, frame, log_area);
                ui::render_input_box(input, app.mode, app.focus == app::Focus::Input, frame, input_area);
                ui::render_search_box(&app.search_box, frame, search_area);
                ui::render_status_bar(app, frame, status_area);
                if let Some(popup) = &input.popup {
                    let rect = popup_rect(input_area, frame.area(), popup.matches().len());
                    ui::render_popup(input, frame, rect);
                }
            })
            .map_err(|e| e.to_string())?;

        if !event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            continue;
        }
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

        // Search-box editing is checked before Ctrl+C / Normal/Insert so that
        // Ctrl+C cancels the draft like Esc, instead of quitting in Normal.
        if app.search_box.editing {
            handle_search_box_key(app, key);
            continue;
        }

        // Ctrl+C: quit from Normal (like `q`), but only cancel in-progress
        // input from Insert (like Esc) — mirrors the shell/readline
        // "abort current line" convention instead of nuking the session.
        if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
            handle_ctrl_c(app, input);
            continue;
        }

        // Ctrl-d/Ctrl-u page the log list when it has focus in Normal mode.
        // Intercepted here (like Ctrl+C above) rather than threading the full
        // KeyEvent into `handle_normal_key`, which deliberately only takes a
        // bare `KeyCode`.
        if key.modifiers.contains(event::KeyModifiers::CONTROL)
            && app.mode == Mode::Normal
            && app.focus == app::Focus::LogList
        {
            match key.code {
                KeyCode::Char('d') => {
                    app.move_cursor_manual(PAGE_SIZE);
                    continue;
                }
                KeyCode::Char('u') => {
                    app.move_cursor_manual(-PAGE_SIZE);
                    continue;
                }
                _ => {}
            }
        }

        match app.mode {
            Mode::Normal => handle_normal_key(app, input, key.code),
            Mode::Insert => handle_insert_key(app, input, key.code)?,
        }
    }
    Ok(())
}

fn handle_ctrl_c(app: &mut App, input: &mut input::InputBox) {
    use app::Mode;

    match app.mode {
        Mode::Normal => app.should_quit = true,
        Mode::Insert => {
            if input.popup.is_some() {
                input.cancel_popup();
            } else {
                *input = input::InputBox::default();
                app.mode = Mode::Normal;
                focus_loglist(app);
            }
        }
    }
}

/// Return keyboard focus to the log list without changing cursor / offset /
/// following. Used when leaving ChipStrip/SearchStrip/Input/SearchBox.
fn focus_loglist(app: &mut App) {
    app.focus = app::Focus::LogList;
}

/// Return focus to the log list and resume live follow (pin to bottom).
/// Reserved for Esc on LogList, Visual Esc, and successful filter-group submit.
fn focus_loglist_and_follow(app: &mut App) {
    app.focus = app::Focus::LogList;
    app.resume_following();
}

fn focus_input_insert(app: &mut App) {
    app.focus = app::Focus::Input;
    app.mode = app::Mode::Insert;
}

/// Mouse wheel step size for the log list, independent of `PAGE_SIZE`
/// (Ctrl-d/Ctrl-u) and the `Shift+J`/`Shift+K` fast-move binding — a wheel
/// "notch" is a smaller, more frequent unit than either of those.
const MOUSE_SCROLL_STEP: isize = 3;

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
/// dispatched before any of them while `app.search_box.editing`. Ctrl+C
/// here cancels the draft (like Esc), not quit. Invalid regex on Enter is
/// silently ignored so a typo can't end the session or drop existing groups.
fn handle_search_box_key(app: &mut App, key: event::KeyEvent) {
    let is_ctrl_c = key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Enter => {
            match app.search_box.submit_draft() {
                Ok(Some(group)) => {
                    let idx = app.push_or_find_search_group(group);
                    app.search_box.clear();
                    let _ = app.jump_first_match_of(idx);
                    app.focus = app::Focus::LogList;
                }
                Ok(None) => {
                    // empty Enter: no-op (stay editing)
                }
                Err(()) => {
                    // bad regex: exit editing, keep prior search groups
                    app.search_box.clear();
                    focus_loglist(app);
                }
            }
        }
        KeyCode::Esc => {
            app.search_box.clear();
            focus_loglist(app);
        }
        _ if is_ctrl_c => {
            app.search_box.clear();
            focus_loglist(app);
        }
        KeyCode::Backspace => app.search_box.backspace(),
        KeyCode::Char(c) => app.search_box.push_char(c),
        _ => {}
    }
}

fn handle_strip_d_chord(app: &mut App, kind: app::StripKind, code: KeyCode) -> bool {
    use app::StripKind;
    if !matches!(
        (kind, app.focus),
        (StripKind::Filter, app::Focus::ChipStrip) | (StripKind::Search, app::Focus::SearchStrip)
    ) {
        return false;
    }
    if app.pending_d {
        match code {
            KeyCode::Char('d') => {
                app.delete_focused_strip_group(kind);
                app.pending_d = false;
                return true;
            }
            KeyCode::Char('i') => {
                app.toggle_disable_focused(kind);
                app.pending_d = false;
                return true;
            }
            _ => {
                app.pending_d = false;
                return false;
            }
        }
    }
    if code == KeyCode::Char('d') {
        app.pending_d = true;
        return true;
    }
    false
}

fn handle_normal_key(app: &mut App, _input: &mut input::InputBox, code: KeyCode) {
    use app::{Focus, StripKind, YankField};

    // Visual-line: handle selection motions / yank before anything else.
    if app.visual_anchor.is_some() && app.focus == Focus::LogList {
        match code {
            KeyCode::Char('j') => {
                app.move_cursor_manual(1);
                return;
            }
            KeyCode::Char('k') => {
                app.move_cursor_manual(-1);
                return;
            }
            KeyCode::Char('J') => {
                app.move_cursor_manual(7);
                return;
            }
            KeyCode::Char('K') => {
                app.move_cursor_manual(-7);
                return;
            }
            KeyCode::Char('y') => {
                if let Some((lo, hi)) = app.selection_range() {
                    if let Some(text) = app.yank_range(lo, hi, YankField::Raw) {
                        apply_yank(app, text);
                    }
                }
                app.clear_visual();
                return;
            }
            KeyCode::Char('Y') => {
                if let Some((lo, hi)) = app.selection_range() {
                    if let Some(text) = app.yank_range(lo, hi, YankField::Msg) {
                        apply_yank(app, text);
                    }
                }
                app.clear_visual();
                return;
            }
            KeyCode::Esc => {
                app.clear_visual();
                focus_loglist_and_follow(app);
                return;
            }
            _ => {
                app.clear_visual();
                // fall through so the key still does its Normal action
            }
        }
    }

    // Yank operator pending: consume the second key (or Esc) and return.
    if app.pending_yank {
        app.pending_yank = false;
        if app.focus == Focus::LogList {
            match code {
                KeyCode::Esc => {
                    app.status_msg = None;
                    return;
                }
                KeyCode::Char(c) => {
                    if let Some(field) = YankField::from_char(c) {
                        if let Some(text) = app.yank_field(field) {
                            apply_yank(app, text);
                        }
                    } else {
                        app.status_msg = None;
                    }
                    return;
                }
                _ => {
                    app.status_msg = None;
                    return;
                }
            }
        }
    }

    // Filter / Search strip: `dd` delete, `di` toggle disable.
    if handle_strip_d_chord(app, StripKind::Filter, code) {
        return;
    }
    if handle_strip_d_chord(app, StripKind::Search, code) {
        return;
    }
    if code != KeyCode::Char('d') {
        app.pending_d = false;
    }

    match (app.focus, code) {
        (_, KeyCode::Char('q')) => app.should_quit = true,
        (_, KeyCode::Tab) => {
            app.cycle_focus_forward();
            if app.focus == Focus::Input {
                app.mode = app::Mode::Insert;
            }
        }
        (_, KeyCode::BackTab) => {
            app.cycle_focus_backward();
            if app.focus == Focus::Input {
                app.mode = app::Mode::Insert;
            }
        }
        (_, KeyCode::Char('1')) => app.focus = Focus::ChipStrip,
        (_, KeyCode::Char('2')) => app.focus = Focus::SearchStrip,
        (_, KeyCode::Char('3')) => app.focus = Focus::LogList,
        (_, KeyCode::Char('4')) => focus_input_insert(app),
        (_, KeyCode::Esc) => {
            if app.focus == Focus::LogList {
                focus_loglist_and_follow(app);
            } else {
                focus_loglist(app);
            }
        }
        (Focus::LogList, KeyCode::Char('j')) => app.move_cursor_manual(1),
        (Focus::LogList, KeyCode::Char('k')) => app.move_cursor_manual(-1),
        (Focus::LogList, KeyCode::Char('J')) => app.move_cursor_manual(7),
        (Focus::LogList, KeyCode::Char('K')) => app.move_cursor_manual(-7),
        (Focus::LogList, KeyCode::Char('g')) => {
            app.following = false;
            app.jump_top();
        }
        (Focus::LogList, KeyCode::Char('G')) => {
            app.following = false;
            app.jump_bottom();
        }
        (Focus::LogList, KeyCode::Char('y')) => {
            app.pending_yank = true;
            app.status_msg = Some("y…".into());
        }
        (Focus::LogList, KeyCode::Char('Y')) => {
            if let Some(text) = app.yank_field(YankField::Msg) {
                apply_yank(app, text);
            }
        }
        (Focus::LogList, KeyCode::Char('V')) => app.enter_visual_line(),
        (Focus::LogList, KeyCode::Char('n')) => {
            let _ = app.find_match(1);
        }
        (Focus::LogList, KeyCode::Char('N')) => {
            let _ = app.find_match(-1);
        }
        (Focus::ChipStrip, KeyCode::Char('h')) => app.move_strip_cursor(StripKind::Filter, -1),
        (Focus::ChipStrip, KeyCode::Char('l')) => app.move_strip_cursor(StripKind::Filter, 1),
        (Focus::SearchStrip, KeyCode::Char('h')) => app.move_strip_cursor(StripKind::Search, -1),
        (Focus::SearchStrip, KeyCode::Char('l')) => app.move_strip_cursor(StripKind::Search, 1),
        (_, KeyCode::Char('a') | KeyCode::Char('i') | KeyCode::Char('o')) => {
            focus_input_insert(app);
        }
        (_, KeyCode::Char('/')) => {
            app.search_box.editing = true;
        }
        _ => {}
    }
}

fn handle_insert_key(app: &mut App, input: &mut input::InputBox, code: KeyCode) -> Result<(), String> {
    if input.popup.is_some() {
        match code {
            KeyCode::Up => input.popup.as_mut().unwrap().move_selection(-1),
            KeyCode::Down => input.popup.as_mut().unwrap().move_selection(1),
            KeyCode::Enter | KeyCode::Tab => input.confirm_popup(),
            KeyCode::Esc => input.cancel_popup(),
            KeyCode::Backspace => input.popup.as_mut().unwrap().backspace(),
            KeyCode::Char(c) => input.popup.as_mut().unwrap().push_char(c),
            _ => {}
        }
        return Ok(());
    }

    // Tab/BackTab cycle focus while editing (digits stay literal chars).
    // Enter two-step: pending draft → commit pill; chips ready → submit group;
    // empty input → jump focus to LogList.
    // Filter chips always compile case-insensitively. Space is literal text.
    match code {
        KeyCode::Esc => {
            *input = input::InputBox::default();
            app.mode = app::Mode::Normal;
            focus_loglist(app);
        }
        KeyCode::Tab => {
            app.cycle_focus_forward();
            app.mode = if app.focus == app::Focus::Input {
                app::Mode::Insert
            } else {
                app::Mode::Normal
            };
        }
        KeyCode::BackTab => {
            app.cycle_focus_backward();
            app.mode = if app.focus == app::Focus::Input {
                app::Mode::Insert
            } else {
                app::Mode::Normal
            };
        }
        KeyCode::Char('/') => input.open_popup(),
        KeyCode::Enter => {
            if input.has_pending_draft() {
                input.commit_draft_as_chip();
            } else if input.is_empty() {
                app.mode = app::Mode::Normal;
                app.focus = app::Focus::LogList;
            } else if let Some(group) = input.build_group(true)? {
                if app.push_filter_group(group) {
                    app.rebuild_visible();
                }
                app.mode = app::Mode::Normal;
                focus_loglist_and_follow(app);
            }
        }
        KeyCode::Backspace => input.backspace(),
        KeyCode::Char(c) => input.push_char(c),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    #[test]
    fn test_popup_rect_anchors_above_input_and_clamps_to_space_above() {
        let input_area = Rect { x: 0, y: 10, width: 40, height: 3 };
        let frame_area = Rect { x: 0, y: 0, width: 80, height: 14 };
        let rect = popup_rect(input_area, frame_area, 20); // way more matches than fit
        // desired = min(6,20)+2 = 8; space above = 10; height = 8; y = 10-8 = 2
        assert_eq!(rect.height, 8);
        assert_eq!(rect.y, 2, "popup should sit directly above the input box");
        assert!(rect.y + rect.height <= input_area.y, "popup must not overlap the input box");
    }

    #[test]
    fn test_popup_rect_clamps_when_little_space_above() {
        let input_area = Rect { x: 0, y: 3, width: 40, height: 3 };
        let frame_area = Rect { x: 0, y: 0, width: 80, height: 20 };
        let rect = popup_rect(input_area, frame_area, 6);
        assert_eq!(rect.height, 3);
        assert_eq!(rect.y, 0);
    }

    #[test]
    fn test_a_enters_insert_mode() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        assert_eq!(app.mode, app::Mode::Insert);
        assert_eq!(app.focus, app::Focus::Input);
    }

    #[test]
    fn test_number_keys_switch_focus_and_4_enters_insert() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();

        handle_normal_key(&mut app, &mut input, KeyCode::Char('4'));
        assert_eq!(app.focus, app::Focus::Input);
        assert_eq!(app.mode, app::Mode::Insert, "4 focuses Input in Insert (no idle Normal)");

        handle_normal_key(&mut app, &mut input, KeyCode::Char('1'));
        assert_eq!(app.focus, app::Focus::ChipStrip);

        handle_normal_key(&mut app, &mut input, KeyCode::Char('2'));
        assert_eq!(app.focus, app::Focus::SearchStrip);

        handle_normal_key(&mut app, &mut input, KeyCode::Char('3'));
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_esc_from_other_focus_preserves_loglist_position() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..20 {
            tx.send(model::EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}")).unwrap())
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
        assert!(!app.following, "Esc from ChipStrip must not resume following");
        assert_eq!(app.cursor, 5);
        assert_eq!(app.list_offset, 2);
    }

    #[test]
    fn test_esc_on_loglist_resumes_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap()).unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : b").unwrap()).unwrap();
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
            tx.send(crate::model::EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}")).unwrap())
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
    fn test_ctrl_c_in_insert_mode_cancels_input_instead_of_quitting() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
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
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        input.push_char('x');
        handle_ctrl_c(&mut app, &mut input);
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(app.focus, app::Focus::LogList, "Ctrl+C should behave like Esc: also return focus to the log list");
    }

    #[test]
    fn test_ctrl_c_with_popup_open_only_closes_popup() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        input.push_char('x');
        input.open_popup(); // commits "x" as a msg chip, opens popup
        assert_eq!(input.chips.len(), 1);
        handle_ctrl_c(&mut app, &mut input);
        assert!(input.popup.is_none());
        assert_eq!(input.chips.len(), 1, "chip built before opening the popup should survive Ctrl+C");
        assert_eq!(app.mode, app::Mode::Insert, "should stay in Insert, not quit or fully reset");
    }

    #[test]
    fn test_esc_in_insert_mode_resets_input_and_returns_to_normal() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Esc).unwrap();
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(app.focus, app::Focus::LogList, "Esc should also return focus to the log list");
        assert!(input.is_empty());
    }

    #[test]
    fn test_enter_two_step_builds_group_and_returns_focus_to_loglist() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // commit pill
        assert_eq!(input.chips.len(), 1);
        assert_eq!(app.groups.groups.len(), 0);
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // submit group
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.mode, app::Mode::Normal, "adding a group should behave like Esc: back to Normal");
        assert_eq!(app.focus, app::Focus::LogList, "adding a group should jump focus back to the log list");
        assert!(app.following);
        assert!(input.chips.is_empty());
    }

    #[test]
    fn test_enter_with_empty_input_focuses_loglist() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        assert!(app.groups.groups.is_empty(), "empty draft must not build a group");
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(app.focus, app::Focus::LogList, "empty Enter switches focus to LogList");
    }

    #[test]
    fn test_tab_in_insert_cycles_focus_away_from_input() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        input.push_char('x'); // draft preserved across Tab
        handle_insert_key(&mut app, &mut input, KeyCode::Tab).unwrap();
        assert_eq!(app.focus, app::Focus::ChipStrip);
        assert_eq!(app.mode, app::Mode::Normal);
        assert_eq!(input.draft, "x");
        handle_normal_key(&mut app, &mut input, KeyCode::Tab); // ChipStrip → SearchStrip
        handle_normal_key(&mut app, &mut input, KeyCode::Tab); // → LogList
        handle_normal_key(&mut app, &mut input, KeyCode::Tab); // → Input + Insert
        assert_eq!(app.focus, app::Focus::Input);
        assert_eq!(app.mode, app::Mode::Insert);
    }

    #[test]
    fn test_digit_in_insert_is_literal_not_focus_switch() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        handle_insert_key(&mut app, &mut input, KeyCode::Char('1')).unwrap();
        assert_eq!(app.focus, app::Focus::Input);
        assert_eq!(input.draft, "1");
    }

    #[test]
    fn test_space_is_literal_in_input_draft() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
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
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        input.set_field(input::ChipField::Tag);
        for c in "mytag".chars() {
            input.push_char(c);
        }
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // pill
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap(); // submit
        let row = crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : m").unwrap();
        assert!(app.groups.matches(&row), "filter chips must ignore case by default");
    }

    #[test]
    fn test_filter_group_dedup_skips_duplicate() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        assert_eq!(app.groups.groups.len(), 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        input.push_char('x');
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        handle_insert_key(&mut app, &mut input, KeyCode::Enter).unwrap();
        assert_eq!(app.groups.groups.len(), 1, "duplicate filter group must not be added");
    }

    #[test]
    fn test_dd_chord_requires_two_consecutive_d_presses() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.groups.groups.push(Group {
            label: "g0".into(),
            chips: Vec::new(),
            expr: None,
            time: None,
            enabled: true,
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
            expr: None,
            time: None,
            enabled: true,
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
    fn test_search_box_enter_adds_group_ignore_case() {
        let mut app = App::new(100);
        app.search_box.editing = true;
        for c in "ERROR".chars() {
            app.search_box.push_char(c);
        }
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.search_box.editing);
        assert_eq!(app.search_groups.groups.len(), 1);
        assert!(app.search_groups.any_match("an error occurred"));
    }

    #[test]
    fn test_search_box_enter_jumps_to_first_match() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap()).unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : findme here").unwrap()).unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:02.000  1  1 I T   : findme two").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = true;
        app.cursor = 2;

        app.search_box.editing = true;
        for c in "findme".chars() {
            app.search_box.push_char(c);
        }
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.cursor, 1);
        assert!(!app.following);
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_search_box_space_is_literal_single_enter_submits() {
        let mut app = App::new(100);
        app.search_box.editing = true;
        for c in "foo bar".chars() {
            app.search_box.push_char(c);
        }
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.search_groups.groups.len(), 1);
        assert_eq!(app.search_groups.groups[0].pattern, "foo bar");
        assert_eq!(app.search_groups.active_patterns().len(), 1);
    }

    #[test]
    fn test_search_box_duplicate_jumps_without_adding() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap()).unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : findme here").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;

        app.search_box.editing = true;
        for c in "findme".chars() {
            app.search_box.push_char(c);
        }
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.search_groups.groups.len(), 1);
        assert_eq!(app.cursor, 1);

        app.cursor = 0;
        app.search_box.editing = true;
        for c in "FINDME".chars() {
            app.search_box.push_char(c);
        }
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.search_groups.groups.len(), 1, "duplicate must not add another group");
        assert_eq!(app.cursor, 1, "duplicate still jumps to first match");
    }

    #[test]
    fn test_g_jump_bottom_does_not_resume_follow() {
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap()).unwrap();
        tx.send(model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : b").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-1);
        assert!(!app.following);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('G'));
        assert_eq!(app.cursor, 1);
        assert!(!app.following, "G must not resume following; only Esc on LogList does");
    }

    #[test]
    fn test_search_box_ctrl_c_cancels_without_adding() {
        let mut app = App::new(100);
        app.search_box.editing = true;
        app.search_box.push_char('x');
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.search_box.editing);
        assert!(app.search_groups.groups.is_empty());
    }

    #[test]
    fn test_search_box_invalid_regex_does_not_drop_prior_groups() {
        let mut app = App::new(100);
        app.search_groups
            .groups
            .push(search_model::SearchGroup::from_pattern("existing").unwrap());
        app.search_box.editing = true;
        for c in "(unclosed".chars() {
            app.search_box.push_char(c);
        }
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.search_box.editing);
        assert_eq!(app.search_groups.groups.len(), 1);
        assert!(app.search_groups.any_match("existing"));
    }

    #[test]
    fn test_search_box_esc_and_enter_return_focus_to_loglist() {
        let mut app = App::new(100);
        app.focus = app::Focus::Input;
        app.search_box.editing = true;
        app.search_box.push_char('x');
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.focus, app::Focus::LogList);

        app.focus = app::Focus::ChipStrip;
        app.search_box.editing = true;
        app.search_box.push_char('y');
        handle_search_box_key(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.focus, app::Focus::LogList);
    }

    #[test]
    fn test_mouse_scroll_down_and_up_move_cursor_by_three() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        for i in 0..20 {
            tx.send(crate::model::EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}")).unwrap())
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
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : a").unwrap()).unwrap();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I Tag     : b").unwrap()).unwrap();
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
            tx.send(crate::model::EntryRow::from_line(line).unwrap()).unwrap();
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
        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        drain_lines(&mut app, &["04-02 10:00:00.000  1  1 I MyTag   : boom"]);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('t'));
        assert_eq!(app.last_yanked.as_deref(), Some("MyTag"));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('y'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
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
        app.search_groups
            .groups
            .push(search_model::SearchGroup::from_pattern("hit").unwrap());
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 3);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('N'));
        assert_eq!(app.cursor, 1);
    }
}
