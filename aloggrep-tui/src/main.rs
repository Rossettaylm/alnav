mod app;
mod bookmark;
mod config;
mod export;
mod filter_model;
mod help;
mod ingest;
mod input;
mod model;
mod picker;
mod preview;
mod highlight_model;
mod theme;
mod ui;
mod vocab;

use std::io::{self, IsTerminal};
use std::time::Duration;

use std::path::PathBuf;

use clap::Parser;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
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
const LEVEL_CANDIDATES: &[&str] = &["V", "D", "I", "W", "E", "F"];

#[derive(Parser)]
#[command(
    name = "aloggrep-tui",
    about = "Interactive vim-style viewer for aloggrep"
)]
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
    #[arg(long, default_value_t = 500_000)]
    max_lines: usize,

    /// Capture logs directly from hdc hilog (HarmonyOS device)
    #[arg(long)]
    hdc: bool,

    /// Device serial number (for --hdc with multiple devices)
    #[arg(long, value_name = "SERIAL")]
    device: Option<String>,

    /// Config directory override (reads `theme.toml`; default: `$ALOGGREP_HOME` or `~/.config/aloggrep`)
    #[arg(long, value_name = "DIR")]
    config_path: Option<PathBuf>,
}

fn initial_group(cli: &Cli) -> Result<GroupList, String> {
    // TUI always compiles filters case-insensitively (CLI `-i` is a no-op).
    // Startup CLI multi-values keep OR (`msg:a|b` label); interactive chips use And.
    let expr = Expr::from_filters(
        &cli.tag,
        &cli.msg,
        &cli.pkg,
        &cli.pid,
        &cli.tid,
        cli.level.as_deref(),
        true,
        SameFieldOp::Or,
    )?;
    let time = if cli.since.is_some() || cli.until.is_some() {
        Some(TimeBound {
            since: cli.since.clone(),
            until: cli.until.clone(),
        })
    } else {
        None
    };
    if expr.is_none() && time.is_none() {
        return Ok(GroupList::default());
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
    if let Some(s) = &cli.since {
        label_parts.push(format!("since:{s}"));
    }
    if let Some(u) = &cli.until {
        label_parts.push(format!("until:{u}"));
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
            expr,
            time,
            enabled: true,
        }],
        excludes: Vec::new(),
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
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            event::DisableMouseCapture
        );
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

    let config_dir = config::resolve_config_dir(cli.config_path.as_deref());
    let theme_status = config::load_theme(&config_dir);
    let (app_config, config_status) = config::load_config(&config_dir);

    let groups = initial_group(&cli)?;
    let mut app = App::new(cli.max_lines);
    app.config = app_config;
    app.groups = groups;
    app.export_source = if cli.hdc {
        export::ExportSource::Hdc {
            device: cli.device.clone(),
        }
    } else {
        export::ExportSource::File(cli.file.clone().unwrap())
    };
    if let Some(hint) = theme_status.status_hint() {
        app.set_flash(hint);
    }
    if let Some(hint) = config_status.status_hint() {
        app.set_flash(hint);
    }

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
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            event::EnableMouseCapture
        )
        .map_err(|e| e.to_string())?;
        let backend = CrosstermBackend::new(io::stdout());
        Terminal::new(backend).map_err(|e| e.to_string())
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
            return Err(e); // _hdc_child_guard drops here, killing the child if any
        }
    };

    let mut input = input::InputBox::default();
    let _ = cli.ignore_case; // retained for CLI compat; TUI always ignore-case
    let result = run(&mut terminal, &mut app, &mut input, &rx);

    let disable_result = disable_raw_mode().map_err(|e| e.to_string());
    let leave_result = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )
    .map_err(|e| e.to_string());
    disable_result.and(leave_result)?;

    result
    // _hdc_child_guard drops here at end of main(), killing the child if not already killed
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
        Ok(()) => app.set_flash("YANKED"),
        Err(e) => app.set_flash(format!("YANK FAILED: {e}")),
    }
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    input: &mut input::InputBox,
    rx: &std::sync::mpsc::Receiver<model::EntryRow>,
) -> Result<(), String> {
    use app::Mode;
    use std::time::Instant;

    // P4: minimum draw interval during active ingest to avoid thrashing the
    // renderer while the background thread floods the channel.
    // After ingest completes (ingest_done=true) or after any user event,
    // we always draw immediately.
    const MIN_INGEST_DRAW_MS: u64 = 50;
    let mut last_draw = Instant::now();
    let mut force_draw = true; // first frame always draws

    while !app.should_quit {
        app.drain(rx);
        app.tick_flash();
        // P1: recompute highlight match stats once per frame (O(n) scan).
        // All mutation paths set match_stats_stale=true; here is the single
        // amortised recompute point so render_status_bar just reads a cached value.
        app.recompute_match_stats_if_stale();

        // P4: throttle draws during active file ingest to ~20 FPS.
        // User events (force_draw=true) and post-ingest mode always draw immediately.
        let elapsed_ms = last_draw.elapsed().as_millis() as u64;
        let should_draw =
            force_draw || app.ingest_done || elapsed_ms >= MIN_INGEST_DRAW_MS;

        if should_draw {
            force_draw = false;
            last_draw = Instant::now();

            terminal
            .draw(|frame| {
                let frame_area = frame.area();
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
                if let Some(data) = picker_render_data(app) {
                    ui::render_picker(
                        &data.title,
                        &data.mode,
                        &data.text,
                        &data.match_query,
                        &data.chips,
                        data.exclude_chips,
                        data.draft_field,
                        &data.labels,
                        &data.styles,
                        &data.checked,
                        data.selected,
                        &data.empty_msg,
                        &data.preview,
                        app.config.picker_left_ratio,
                        frame,
                        frame_area,
                    );
                    if let Some(confirm) = app
                        .picker
                        .as_ref()
                        .and_then(|session| session.confirm.as_ref())
                    {
                        ui::render_confirm_dialog(confirm, frame, frame_area);
                    }
                // Search / Input use top stack: modal → candidates → Preview (H1).
                } else if app.highlight_box.editing {
                    let area = ui::top_modal_rect(frame_area, modal_w, ui::search_modal_height());
                    ui::render_highlight_modal(&app.highlight_box, frame, area);
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
                    let preview_lines = preview::preview_search_lines(app).unwrap_or_default();
                    let content_rows = if preview_lines.is_empty() {
                        1
                    } else {
                        preview_lines.len()
                    };
                    let prev = ui::preview_popup_rect(stack_bottom, frame_area, content_rows);
                    if prev.height > 0 {
                        ui::render_preview("Preview", &preview_lines, "输入以预览", frame, prev);
                    }
                } else if app.focus == app::Focus::Input {
                    let input_area = ui::top_modal_rect(frame_area, modal_w, 3);
                    ui::render_input_modal(input, app.mode, frame, input_area);
                    let mut stack_bottom = input_area;
                    if input.field_popup_visible() {
                        // `.max(1)` so empty-match state still gets a row for「无匹配字段」.
                        let count = input.field_candidates().len().max(1);
                        let rect = popup_rect(input_area, frame_area, count);
                        ui::render_popup(input, frame, rect);
                        stack_bottom = rect;
                    }
                    let preview_lines = preview::preview_filter_lines(app, input);
                    let content_rows = if preview_lines.is_empty() {
                        1
                    } else {
                        preview_lines.len()
                    };
                    let prev = ui::preview_popup_rect(stack_bottom, frame_area, content_rows);
                    if prev.height > 0 {
                        ui::render_preview("Preview", &preview_lines, "无匹配行", frame, prev);
                    }
                } else if app.detail_open() {
                    let inner_w = modal_w.saturating_sub(2).max(1);
                    let content_rows = ui::detail_content_lines(app, inner_w).len().max(1);
                    let h = ui::detail_modal_height(frame_area, content_rows);
                    let area = ui::top_modal_rect(frame_area, modal_w, h);
                    ui::render_detail(app, frame, area);
                }
            })
            .map_err(|e| e.to_string())?;
        } // end if should_draw

        // Poll for user events. When we skipped the draw (ingest active, drew
        // recently) use a short timeout so we return quickly for the next draw.
        let poll_ms = if should_draw {
            100u64 // normal: wait up to 100ms for the next event
        } else {
            MIN_INGEST_DRAW_MS.saturating_sub(last_draw.elapsed().as_millis() as u64).max(1)
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

        // Picker / Search-box are checked before Ctrl+C / Normal/Insert
        // so Ctrl+C cancels the draft like Esc, instead of quitting in Normal.
        if app.picker.is_some() {
            handle_picker_key(app, key);
            continue;
        }
        if app.highlight_box.editing {
            handle_highlight_box_key(app, key);
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

struct PickerRenderData {
    title: String,
    mode: crate::picker::PickerMode,
    text: String,
    /// Query used to color substring matches in the candidate list.
    match_query: String,
    chips: Vec<input::Chip>,
    exclude_chips: bool,
    /// Confirmed field awaiting a value (`tag:` prefix); Filter/Exclude New·Edit only.
    draft_field: Option<input::ChipField>,
    labels: Vec<String>,
    styles: Vec<ratatui::style::Style>,
    checked: Vec<bool>,
    selected: usize,
    empty_msg: String,
    preview: Vec<preview::PreviewLine>,
}

fn picker_render_data(app: &App) -> Option<PickerRenderData> {
    use crate::picker::{PickerKind, PickerMode, PickerSession, UnifiedKind};

    let session = app.picker.as_ref()?;
    let base_title = match session.kind {
        PickerKind::Unified => "Manage",
        PickerKind::Filter => "Filter",
        PickerKind::Highlight => "Highlight",
        PickerKind::Bookmark => "Bookmark",
        PickerKind::Exclude => "Exclude",
        PickerKind::MsgChip { .. } => "Message",
    };
    let mode_name = match session.mode {
        PickerMode::Manage => "Search",
        PickerMode::New => "New",
        PickerMode::Edit { .. } => "Edit",
    };
    let title = format!("{base_title} · {mode_name}");
    let mode = session.mode.clone();
    let text = match session.mode {
        PickerMode::Manage => session.query.clone(),
        PickerMode::New | PickerMode::Edit { .. } => match session.kind {
            PickerKind::Filter | PickerKind::Exclude => session
                .input
                .as_ref()
                .map(|input| input.draft.clone())
                .unwrap_or_default(),
            _ => session.draft.clone(),
        },
    };
    let match_query = text.clone();
    let mut selected = session.selected;
    let mut chips = Vec::new();
    let mut exclude_chips = false;
    let mut draft_field = None;
    let mut labels = Vec::new();
    let mut styles = Vec::new();
    let mut checked = Vec::new();
    let mut preview_lines = Vec::new();
    let mut empty_msg = "无项目".to_string();

    match session.mode {
        PickerMode::Manage => {
            let all = unified_picker_items(app);
            let all_labels: Vec<String> = all.iter().map(|item| item.label.clone()).collect();
            let visible = PickerSession::filtered_indices(&all_labels, &session.query);
            labels = visible.iter().map(|&index| all[index].label.clone()).collect();
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

            if let Some(&src) = visible.get(session.selected) {
                let item = &all[src];
                match item.id.kind {
                    UnifiedKind::Highlight => {
                        preview_lines = preview::preview_highlight_pattern_lines(
                            app,
                            &app.highlight_groups.groups[item.id.source_index].pattern,
                        )
                        .unwrap_or_default();
                    }
                    UnifiedKind::Filter => {
                        let input = input::InputBox {
                            chips: app.groups.groups[item.id.source_index].chips.clone(),
                            ..input::InputBox::default()
                        };
                        preview_lines = preview::preview_filter_lines(app, &input);
                    }
                    UnifiedKind::Exclude => {
                        let input = input::InputBox {
                            chips: vec![app.groups.excludes[item.id.source_index].chip.clone()],
                            exclude_mode: true,
                            ..input::InputBox::default()
                        };
                        preview_lines = preview::preview_filter_lines(app, &input);
                    }
                    UnifiedKind::Bookmark => {}
                }
            }
        }
        PickerMode::New | PickerMode::Edit { .. } => match session.kind {
            PickerKind::Highlight => {
                if !session.draft.is_empty() {
                    labels = app.vocab.all_candidates(&session.draft);
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
                preview_lines =
                    preview::preview_highlight_pattern_lines(app, &session.draft).unwrap_or_default();
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
                                .map(|f| ratatui::style::Style::default().fg(theme::field_color(*f)))
                                .collect();
                            selected = input.field_selected;
                        }
                        Some(field) => {
                            use crate::input::ChipField;
                            let q = &input.draft;
                            labels = match field {
                                ChipField::Tag => app.vocab.tag_candidates(q),
                                ChipField::Pkg => app.vocab.pkg_candidates(q),
                                ChipField::Msg => app.vocab.msg_candidates(q),
                                ChipField::Level => {
                                    let ql = q.to_lowercase();
                                    LEVEL_CANDIDATES
                                        .iter()
                                        .filter(|&&s| ql.is_empty() || s.to_lowercase().contains(&ql))
                                        .map(|&s| s.to_string())
                                        .collect()
                                }
                                ChipField::Pid | ChipField::Tid => vec![],
                            };
                            styles = vec![
                                ratatui::style::Style::default().fg(theme::field_color(field));
                                labels.len()
                            ];
                        }
                    }
                    preview_lines = preview::preview_filter_lines(app, input);
                }
                empty_msg = "Enter 收 pill / 提交".to_string();
            }
            PickerKind::Bookmark => {
                empty_msg = "Enter 添加当前行".to_string();
            }
            PickerKind::MsgChip { .. } => {
                let visible =
                    PickerSession::filtered_indices(&session.choices, &session.draft);
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
            PickerKind::Unified => {}
        },
    }

    Some(PickerRenderData {
        title,
        mode,
        text,
        match_query,
        chips,
        exclude_chips,
        draft_field,
        labels,
        styles,
        checked,
        selected,
        empty_msg,
        preview: preview_lines,
    })
}

/// Aggregate Filter → Highlight → Exclude → Bookmark (newest-first) for Manage.
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
    for (displayed, bookmark) in app.bookmarks.items.iter().enumerate().rev() {
        items.push(UnifiedItem {
            id: UnifiedId {
                kind: UnifiedKind::Bookmark,
                source_index: displayed,
            },
            label: format!("[{}]: {}", UnifiedKind::Bookmark.tag(), bookmark.label),
            enabled: bookmark.enabled,
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
    let all = unified_picker_items(app);
    let labels: Vec<String> = all.iter().map(|item| item.label.clone()).collect();
    let visible = PickerSession::filtered_indices(&labels, &session.query);
    visible.iter().map(|&i| all[i].id).collect()
}

fn unified_selected_id(app: &App) -> Option<crate::picker::UnifiedId> {
    let session = app.picker.as_ref()?;
    let ids = unified_visible_ids(app);
    ids.get(session.selected).copied()
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
    let ConfirmKind::DeleteMany { mut items } = confirm;
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

fn replace_last_token(draft: &str, replacement: &str) -> String {
    match draft.rfind(' ') {
        Some(pos) => format!("{} {}", &draft[..pos], replacement),
        None => replacement.to_string(),
    }
}

fn submit_highlight_picker(app: &mut App) {
    use crate::picker::PickerMode;

    let Some((mode, draft, candidate_selected)) = app
        .picker
        .as_ref()
        .map(|session| {
            (
                session.mode.clone(),
                session.draft.clone(),
                session.selected,
            )
        })
    else {
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
                app.set_flash("已存在");
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
        .map(|session| (session.kind, session.mode.clone()))
    else {
        return;
    };

    {
        let Some(input) = app.picker.as_mut().and_then(|session| session.input.as_mut()) else {
            return;
        };
        if input.confirm_field_candidate() {
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
        app.set_flash("排除条件仅支持一个 chip");
        return;
    }

    let selected = match (kind, mode) {
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
                app.set_flash("已存在");
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
                app.set_flash("已存在");
                return;
            }
            index
        }
        _ => return,
    };
    match kind {
        PickerKind::Filter => app.group_cursor = selected,
        PickerKind::Exclude => app.exclude_cursor = selected,
        _ => {}
    }
    app.close_picker();
}

fn submit_bookmark_picker(app: &mut App) {
    use crate::picker::PickerMode;

    let Some((mode, draft)) = app
        .picker
        .as_ref()
        .map(|session| (session.mode.clone(), session.draft.clone()))
    else {
        return;
    };
    match mode {
        PickerMode::New => {
            let before = app.bookmarks.len();
            app.bookmark_add_current();
            if app.bookmarks.len() == before {
                return;
            }
        }
        PickerMode::Edit { index } => {
            if draft.is_empty() {
                return;
            }
            let Some(row_id) = app.bookmarks.items.get(index).map(|bookmark| bookmark.row_id)
            else {
                return;
            };
            if !app.update_bookmark_label(row_id, draft) {
                return;
            }
        }
        PickerMode::Manage => return,
    };
    app.close_picker();
}

fn handle_picker_key(app: &mut App, key: event::KeyEvent) {
    use crate::picker::{PickerKind, PickerMode, PickerSession};

    let ctrl = key.modifiers.contains(event::KeyModifiers::CONTROL);
    if app.picker.as_ref().is_some_and(|session| session.confirm.is_some()) {
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
        .map(|session| (session.kind, session.mode.clone()))
    else {
        return;
    };

    if matches!(mode, PickerMode::Manage) {
        let ids = unified_visible_ids(app);
        match key.code {
            KeyCode::Up => {
                app.picker.as_mut().unwrap().selected =
                    app.picker.as_ref().unwrap().selected.saturating_sub(1);
            }
            KeyCode::Down => {
                let session = app.picker.as_mut().unwrap();
                session.selected = (session.selected + 1).min(ids.len().saturating_sub(1));
            }
            KeyCode::Backspace => {
                let session = app.picker.as_mut().unwrap();
                session.query.pop();
                session.selected = 0;
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
            KeyCode::Char('e') if ctrl => {
                if app.picker.as_ref().is_some_and(|s| s.checked.len() > 1) {
                    app.set_flash("不支持批量编辑");
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
                        let pattern =
                            app.highlight_groups.groups[id.source_index].pattern.clone();
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
                    UnifiedKind::Bookmark => {
                        let label = app.bookmarks.items[id.source_index].label.clone();
                        let session = app.picker.as_mut().unwrap();
                        session.kind = id.kind.as_picker_kind();
                        session.enter_edit(id.source_index, label);
                    }
                }
            }
            KeyCode::Char('d') if ctrl => {
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
            KeyCode::Char(c) if !ctrl => {
                let session = app.picker.as_mut().unwrap();
                session.query.push(c);
                session.selected = 0;
            }
            _ => {}
        }
        return;
    }

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
                    (s.draft.clone(), s.selected)
                };
                if !draft.is_empty() {
                    let n = app.vocab.all_candidates(&draft).len();
                    app.picker.as_mut().unwrap().selected = (sel + 1).min(n.saturating_sub(1));
                } else {
                    let mut highlight_box = crate::highlight_model::HighlightBox {
                        draft,
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
                    (session.draft.clone(), session.selected)
                };
                if !draft.is_empty() {
                    let cands = app.vocab.all_candidates(&draft);
                    if let Some(replacement) = cands.into_iter().nth(selected) {
                        let new_draft = replace_last_token(&draft, &replacement);
                        let session = app.picker.as_mut().unwrap();
                        session.draft = new_draft;
                        session.selected = 0;
                    }
                }
            }
            KeyCode::Backspace => {
                let session = app.picker.as_mut().unwrap();
                session.draft.pop();
                session.selected = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                let session = app.picker.as_mut().unwrap();
                session.draft.push(c);
                session.selected = 0;
            }
            _ => {}
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
                            ChipField::Tag => app.vocab.tag_candidates(&input.draft),
                            ChipField::Pkg => app.vocab.pkg_candidates(&input.draft),
                            ChipField::Msg => app.vocab.msg_candidates(&input.draft),
                            ChipField::Level => {
                                let ql = input.draft.to_lowercase();
                                LEVEL_CANDIDATES
                                    .iter()
                                    .filter(|&&s| ql.is_empty() || s.to_lowercase().contains(&ql))
                                    .map(|&s| s.to_string())
                                    .collect()
                            }
                            ChipField::Pid | ChipField::Tid => vec![],
                        }
                    };
                    if let Some(value) = labels.into_iter().nth(selected) {
                        if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                            input.draft = value;
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
                            ChipField::Tag => app.vocab.tag_candidates(&input.draft).len(),
                            ChipField::Pkg => app.vocab.pkg_candidates(&input.draft).len(),
                            ChipField::Msg => app.vocab.msg_candidates(&input.draft).len(),
                            ChipField::Level => LEVEL_CANDIDATES.len(),
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
                    app.set_flash("排除条件仅支持一个 chip");
                } else if let Some(input) = app.picker.as_mut().unwrap().input.as_mut() {
                    input.push_char(c);
                }
            }
            _ => {}
        },
        PickerKind::Bookmark => match key.code {
            KeyCode::Enter => submit_bookmark_picker(app),
            KeyCode::Backspace => {
                app.picker.as_mut().unwrap().draft.pop();
            }
            KeyCode::Char(c) if !ctrl => app.picker.as_mut().unwrap().draft.push(c),
            _ => {}
        },
        PickerKind::MsgChip { .. } => match key.code {
            KeyCode::Enter | KeyCode::Tab => {
                let _ = app.confirm_msg_chip_picker();
            }
            KeyCode::Up => {
                let selected = app.picker.as_ref().unwrap().selected.saturating_sub(1);
                app.picker.as_mut().unwrap().selected = selected;
            }
            KeyCode::Down => {
                let session = app.picker.as_ref().unwrap();
                let count =
                    PickerSession::filtered_indices(&session.choices, &session.draft).len();
                app.picker.as_mut().unwrap().selected =
                    (session.selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Backspace => {
                let session = app.picker.as_mut().unwrap();
                session.draft.pop();
                session.selected = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                let session = app.picker.as_mut().unwrap();
                session.draft.push(c);
                session.selected = 0;
            }
            _ => {}
        },
        PickerKind::Unified => {}
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
    let is_ctrl_c =
        key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Up => app.highlight_box.move_selection(&app.highlight_groups.groups, -1),
        KeyCode::Down => app.highlight_box.move_selection(&app.highlight_groups.groups, 1),
        KeyCode::Enter | KeyCode::Tab => {
            match app.highlight_box.confirm_or_submit(&app.highlight_groups.groups) {
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
        KeyCode::Backspace => app.highlight_box.backspace(),
        KeyCode::Char(c) => app.highlight_box.push_char(c),
        _ => {}
    }
}

fn handle_strip_d_chord(app: &mut App, kind: app::StripKind, code: KeyCode) -> bool {
    use app::StripKind;
    if !matches!(
        (kind, app.focus),
        (StripKind::Filter, app::Focus::ChipStrip)
            | (StripKind::Exclude, app::Focus::ExcludeStrip)
            | (StripKind::Highlight, app::Focus::HighlightStrip)
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
        app.pending_leader = false;
        app.pending_d = true;
        return true;
    }
    false
}

fn handle_leader_key(app: &mut App, code: KeyCode) -> bool {
    if app.pending_leader {
        app.pending_leader = false;
        match code {
            // Leader+Leader → unified Manage search panel
            KeyCode::Char(' ') => app.open_unified_picker(),
            KeyCode::Esc => {}
            _ => app.set_flash("未知 Leader"),
        }
        return true;
    }

    if code == KeyCode::Char(' ') {
        app.clear_visual();
        app.pending_d = false;
        app.pending_yank = false;
        app.pending_chip = false;
        app.pending_exclude = false;
        app.pending_lock = false;
        app.pending_m = false;
        app.pending_leader = true;
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

    if handle_leader_key(app, code) {
        return;
    }

    // Yank operator pending: consume the second key (or Esc) and return.
    if app.pending_yank {
        app.pending_yank = false;
        if app.focus == Focus::LogList {
            match code {
                KeyCode::Esc => {
                    return;
                }
                KeyCode::Char(c) => {
                    // H10: `yc` exports CLI command (before YankField — `c` is not a field).
                    if c == 'c' {
                        let cmd = app.export_cli_command();
                        apply_yank(app, cmd);
                        return;
                    }
                    if let Some(field) = YankField::from_char(c) {
                        if let Some(text) = app.yank_field(field) {
                            apply_yank(app, text);
                        }
                    }
                    return;
                }
                _ => {
                    return;
                }
            }
        }
    }

    // Chip-from-cursor operator pending (`c` + field letter). Esc clears pending
    // without resume_following (same as Search cancel / yank Esc).
    if app.pending_chip {
        app.pending_chip = false;
        if app.focus == Focus::LogList {
            match code {
                KeyCode::Esc => {
                    app.cancel_chip_from_cursor();
                    return;
                }
                KeyCode::Char(c) => {
                    use app::ChipFieldKey;
                    use input::ChipField;
                    match ChipFieldKey::from_char(c) {
                        ChipFieldKey::Field(ChipField::Msg) => {
                            app.begin_msg_chip_picker(false);
                        }
                        ChipFieldKey::Field(field) => {
                            let _ = app.push_chip_from_field(field);
                        }
                        ChipFieldKey::Unsupported => {
                            app.set_flash("不支持 raw/timestamp");
                        }
                        ChipFieldKey::Unknown => {
                            app.set_flash("未知字段");
                        }
                    }
                    return;
                }
                _ => {
                    app.set_flash("未知字段");
                    return;
                }
            }
        }
    }

    // Exclude-from-cursor operator pending (`C` + field). Esc clears pending only.
    if app.pending_exclude {
        app.pending_exclude = false;
        if app.focus == Focus::LogList {
            match code {
                KeyCode::Esc => {
                    app.cancel_chip_from_cursor();
                    return;
                }
                KeyCode::Char(c) => {
                    use app::ChipFieldKey;
                    use input::ChipField;
                    match ChipFieldKey::from_char(c) {
                        ChipFieldKey::Field(ChipField::Msg) => {
                            app.begin_msg_chip_picker(true);
                        }
                        ChipFieldKey::Field(field) => {
                            let _ = app.push_exclude_from_field(field);
                        }
                        ChipFieldKey::Unsupported => {
                            app.set_flash("不支持 raw/timestamp");
                        }
                        ChipFieldKey::Unknown => {
                            app.set_flash("未知字段");
                        }
                    }
                    return;
                }
                _ => {
                    app.set_flash("未知字段");
                    return;
                }
            }
        }
    }

    // Session lock operator pending (`f` + p/t/u). Esc clears pending only;
    // does not clear lock or resume_following.
    if app.pending_lock {
        app.pending_lock = false;
        if app.focus == Focus::LogList {
            match code {
                KeyCode::Esc => {
                    app.cancel_lock_pending();
                    return;
                }
                KeyCode::Char('p') => {
                    app.apply_session_lock(app::LockKind::Pid);
                    return;
                }
                KeyCode::Char('t') => {
                    app.apply_session_lock(app::LockKind::Tid);
                    return;
                }
                KeyCode::Char('u') => {
                    app.clear_session_lock();
                    return;
                }
                KeyCode::Char(_) => {
                    app.set_flash("未知");
                    return;
                }
                _ => {
                    app.set_flash("未知");
                    return;
                }
            }
        }
    }

    // Bookmark operator pending (`m`/`M` + a/d/m/M). Esc clears pending only.
    // `mm` → Manage, `MM` → New; mixed case on the second key follows that key.
    if app.pending_m {
        app.pending_m = false;
        if app.focus == Focus::LogList {
            match code {
                KeyCode::Esc => app.cancel_bookmark_op(),
                KeyCode::Char('a') => app.bookmark_add_current(),
                KeyCode::Char('d') => app.bookmark_remove_current(),
                KeyCode::Char('m') => {
                    app.open_picker_new(crate::picker::PickerKind::Bookmark);
                }
                _ => app.set_flash("未知"),
            }
        }
        return;
    }

    // Filter / Exclude / Search strip: `dd` delete, `di` toggle disable.
    if handle_strip_d_chord(app, StripKind::Filter, code) {
        return;
    }
    if handle_strip_d_chord(app, StripKind::Exclude, code) {
        return;
    }
    if handle_strip_d_chord(app, StripKind::Highlight, code) {
        return;
    }
    if code != KeyCode::Char('d') {
        app.pending_d = false;
    }

    match (app.focus, code) {
        (_, KeyCode::Char('q')) => app.should_quit = true,
        (_, KeyCode::Tab) => app.cycle_visible_focus_forward(),
        (_, KeyCode::BackTab) => app.cycle_visible_focus_backward(),
        (_, KeyCode::Char('1')) => app.focus = Focus::ChipStrip,
        (_, KeyCode::Char('2')) => app.focus = Focus::ExcludeStrip,
        (_, KeyCode::Char('3')) => app.focus = Focus::HighlightStrip,
        (_, KeyCode::Char('4')) => app.focus = Focus::LogList,
        (_, KeyCode::Char('5')) => app.open_unified_picker(),
        (_, KeyCode::Esc) => {
            // H4: Esc closes detail only — does not resume_following.
            if app.detail_open() {
                app.close_detail();
                app.focus = Focus::LogList;
            } else if app.focus == Focus::LogList {
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
        (Focus::LogList, KeyCode::Char('p')) => {
            app.toggle_detail_fields();
        }
        (Focus::LogList, KeyCode::Char('P')) => {
            app.toggle_detail_pretty();
        }
        (Focus::LogList, KeyCode::Char('c')) => {
            app.begin_chip_from_cursor();
        }
        (Focus::LogList, KeyCode::Char('C')) => {
            app.begin_exclude_from_cursor();
        }
        (Focus::LogList, KeyCode::Char('f')) => {
            app.begin_lock_from_cursor();
        }
        (Focus::LogList, KeyCode::Char('m') | KeyCode::Char('M')) => {
            app.begin_bookmark_op();
        }
        (Focus::LogList, KeyCode::Char('y')) => {
            app.pending_chip = false;
            app.pending_exclude = false;
            app.pending_lock = false;
            app.pending_m = false;
            app.pending_leader = false;
            app.pending_yank = true;
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
        (Focus::LogList, KeyCode::Char('e')) => {
            if !app.find_severe(1) {
                app.set_flash("NO ERROR");
            }
        }
        (Focus::LogList, KeyCode::Char('E')) => {
            if !app.find_severe(-1) {
                app.set_flash("NO ERROR");
            }
        }
        (Focus::ChipStrip, KeyCode::Char('h')) => app.move_strip_cursor(StripKind::Filter, -1),
        (Focus::ChipStrip, KeyCode::Char('l')) => app.move_strip_cursor(StripKind::Filter, 1),
        (Focus::ExcludeStrip, KeyCode::Char('h')) => app.move_strip_cursor(StripKind::Exclude, -1),
        (Focus::ExcludeStrip, KeyCode::Char('l')) => app.move_strip_cursor(StripKind::Exclude, 1),
        (Focus::HighlightStrip, KeyCode::Char('h')) => app.move_strip_cursor(StripKind::Highlight, -1),
        (Focus::HighlightStrip, KeyCode::Char('l')) => app.move_strip_cursor(StripKind::Highlight, 1),
        // Unified Manage: Space Space · New: `;` Filter · `/` Highlight · `` ` `` Exclude · `mm` Bookmark
        (_, KeyCode::Char(';')) => {
            app.open_picker_new(crate::picker::PickerKind::Filter);
        }
        (_, KeyCode::Char('/')) => {
            app.open_picker_new(crate::picker::PickerKind::Highlight);
        }
        (_, KeyCode::Char('`')) => {
            app.open_picker_new(crate::picker::PickerKind::Exclude);
        }
        _ => {}
    }
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
            if input.confirm_field_candidate() {
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
                    app.set_flash("已存在");
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
    use crossterm::event::{KeyEvent, KeyModifiers};

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
    fn bare_keys_open_unified_or_new() {
        use crate::filter_model::Group;
        use crate::highlight_model::HighlightGroup;
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        let mut input = input::InputBox::default();
        app.groups.groups.push(Group {
            label: "g".into(),
            chips: Vec::new(),
            expr: None,
            time: None,
            enabled: true,
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
        let bookmark = app.picker.as_ref().expect("mm opens bookmark New");
        assert_eq!(bookmark.kind, PickerKind::Bookmark);
        assert_eq!(bookmark.mode, PickerMode::New);
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
        assert!(picker
            .input
            .as_ref()
            .is_some_and(|box_| box_.is_empty()));
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
            app.open_picker_new(kind);
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
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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

        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
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
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        );
        assert_eq!(app.highlight_groups.groups.len(), 1);
        assert!(app.picker.as_ref().unwrap().confirm.is_none());

        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
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
            expr: None,
            time: None,
            enabled: true,
        });
        app.groups.groups.push(Group {
            label: "second".into(),
            chips: Vec::new(),
            expr: None,
            time: None,
            enabled: true,
        });
        app.open_unified_picker();
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        );
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
            expr: None,
            time: None,
            enabled: true,
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

        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );
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
    fn filtered_highlight_edit_submit_closes_and_updates_pattern() {
        use crate::picker::PickerMode;
        use crate::highlight_model::HighlightGroup;

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
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            app.picker.as_ref().unwrap().mode,
            PickerMode::Edit { index: 2 }
        );
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        );
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
                                                      // desired = min(8,20)+2 = 10; space below = 24-(10+3)=11; height = 10; y = 13
        assert_eq!(rect.height, 10);
        assert_eq!(rect.y, 13, "popup should sit directly below the modal");
        assert_eq!(rect.x, modal.x);
        assert_eq!(rect.width, modal.width);
        assert!(
            rect.y >= modal.y + modal.height,
            "popup must not overlap the modal"
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
        // space below = 20-(16+3)=1; desired=8 → height=1
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
        assert!(input.chips.is_empty(), "field confirm must not commit a pill");
        assert!(matches!(
            app.picker.as_ref().map(|session| &session.mode),
            Some(PickerMode::New)
        ));
        let data = picker_render_data(&app).unwrap();
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
            app.picker.as_ref().map(|picker| picker.kind),
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
            expr: None,
            time: None,
            enabled: true,
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
    fn test_g_jump_bottom_does_not_resume_follow() {
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
        assert!(
            !app.following,
            "G must not resume following; only Esc on LogList does"
        );
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
        app.push_or_find_highlight_group(highlight_model::HighlightGroup::from_pattern("hit").unwrap());
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 1);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 3);
        handle_normal_key(&mut app, &mut input, KeyCode::Char('N'));
        assert_eq!(app.cursor, 1);
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
        app.push_or_find_highlight_group(highlight_model::HighlightGroup::from_pattern("hit").unwrap());
        handle_normal_key(&mut app, &mut input, KeyCode::Char('n'));
        assert_eq!(app.cursor, 2, "n follows search, not severe");
        handle_normal_key(&mut app, &mut input, KeyCode::Char('e'));
        assert_eq!(app.cursor, 1, "e follows severe, not search");
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
        assert_eq!(app.status_msg.as_deref(), Some("已存在"));
    }

    #[test]
    fn test_cm_opens_picker_enter_pushes_token() {
        use crate::picker::{PickerKind, PickerMode};

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
            app.picker.as_ref().map(|picker| (picker.kind, &picker.mode)),
            Some((PickerKind::MsgChip { exclude: false }, PickerMode::New))
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
        assert!(picker_render_data(&app).unwrap().labels.is_empty());
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
            app.picker.as_ref().map(|picker| picker.kind),
            Some(PickerKind::MsgChip { exclude: true })
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
        assert_eq!(app.groups.excludes.len(), 1);
        assert_eq!(app.groups.excludes[0].chip.value, "timeout");
        assert_eq!(app.visible.len(), 1);
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
        assert_eq!(app.status_msg.as_deref(), Some("不支持 raw/timestamp"));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('c'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('x'));
        assert_eq!(app.status_msg.as_deref(), Some("未知字段"));
        assert!(app.groups.groups.is_empty());
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
        assert_eq!(app.lock_badge_label().as_deref(), Some("LOCK pid=111"));

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
        assert_eq!(app.view_source()[app.visible[0]].tag, "Keep");
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
        let lines = ui::detail_field_lines(app.current_row(), 40);
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
        let lines2 = ui::detail_field_lines(app.current_row(), 40);
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
        let (text, ok) = ui::pretty_json_for_row(app.current_row().unwrap());
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
        let lines = ui::detail_pretty_lines(app.current_row(), 40);
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
        assert!(cmd.starts_with("aloggrep -f 'demo.log' -i -e "));
        assert!(cmd.contains(r#"tag ~ "MyTag""#), "{cmd}");
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
        assert_eq!(app.status_msg.as_deref(), Some("已收藏"));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('a'));
        assert_eq!(app.bookmarks.len(), 1);
        assert_eq!(app.status_msg.as_deref(), Some("已存在"));
        app.cursor = 1;
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        handle_normal_key(&mut app, &mut input, KeyCode::Char('m'));
        assert!(matches!(
            app.picker.as_ref().map(|picker| picker.kind),
            Some(crate::picker::PickerKind::Bookmark)
        ));
        assert_eq!(
            app.picker.as_ref().map(|picker| &picker.mode),
            Some(&crate::picker::PickerMode::New)
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
        assert_eq!(app.status_msg.as_deref(), Some("已删除"));
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
            app.picker.as_ref().map(|picker| picker.kind),
            Some(PickerKind::Unified)
        );
    }

    #[test]
    fn unified_enter_toggles_bookmark_enabled() {
        let mut app = App::new(100);
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Old     : first",
                "04-02 10:00:01.000  1  1 I New     : second",
            ],
        );
        app.cursor = 0;
        app.bookmark_add_current();
        app.cursor = 1;
        app.bookmark_add_current();
        app.open_unified_picker();
        // Newest bookmark is first among bookmark rows; with only bookmarks, selected=0.
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert!(app.picker.is_some());
        // Newest-first display: storage index 1 is first in unified list.
        assert!(!app.bookmarks.items[1].enabled);
        assert!(app.bookmarks.items[0].enabled);
    }

    #[test]
    fn bookmark_picker_edit_and_new_use_current_row() {
        use crate::picker::{PickerKind, PickerMode};

        let mut app = App::new(100);
        drain_lines(
            &mut app,
            &[
                "04-02 10:00:00.000  1  1 I Old     : first",
                "04-02 10:00:01.000  1  1 I New     : second",
            ],
        );
        app.cursor = 0;
        app.bookmark_add_current();
        app.open_unified_picker();
        // Query "Bookmark" so only bookmark rows remain (Filter/Highlight/Exclude absent).
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Char('e'), event::KeyModifiers::CONTROL),
        );
        assert!(matches!(
            app.picker.as_ref().unwrap().mode,
            PickerMode::Edit { .. }
        ));
        app.picker.as_mut().unwrap().draft = "renamed".into();
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert_eq!(app.bookmarks.items[0].label, "renamed");
        assert!(app.picker.is_none(), "Edit submit closes picker");
        assert_eq!(app.focus, app::Focus::LogList);

        app.cursor = 1;
        app.open_picker_new(PickerKind::Bookmark);
        assert_eq!(app.picker.as_ref().unwrap().mode, PickerMode::New);
        handle_picker_key(
            &mut app,
            event::KeyEvent::new(KeyCode::Enter, event::KeyModifiers::NONE),
        );
        assert_eq!(app.bookmarks.len(), 2);
        assert!(app.picker.is_none(), "New submit closes picker");
        assert_eq!(app.focus, app::Focus::LogList);
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
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
        handle_picker_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        );

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
            crate::model::EntryRow::from_line("01-01 00:00:00.000  1 1 I TargetTag: msg")
                .unwrap(),
        )
        .unwrap();
        drop(tx);
        app.drain(&rx);

        app.open_picker_new(crate::picker::PickerKind::Filter);
        {
            let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
            input.set_field(crate::input::ChipField::Tag);
        }

        let data = picker_render_data(&app).unwrap();
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
        let data = picker_render_data(&app).unwrap();
        assert!(data.labels.contains(&"W".to_string()));
        assert!(data.labels.contains(&"E".to_string()));
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
        let row = crate::model::EntryRow::from_line(
            "01-01 00:00:00.000  1 1 I TabTestTag: msg",
        )
        .unwrap();
        tx.send(row).unwrap();
        drop(tx);
        app.drain(&rx);

        app.open_picker_new(crate::picker::PickerKind::Filter);
        {
            let input = app.picker.as_mut().unwrap().input.as_mut().unwrap();
            input.set_field(crate::input::ChipField::Tag);
            input.push_char('T');
        }

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
        assert_eq!(draft, "TabTestTag", "Tab should fill vocab candidate into draft");
    }
}
