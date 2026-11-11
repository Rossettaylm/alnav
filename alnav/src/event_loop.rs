//! Main event loop: poll ingest, draw frames, route key/mouse events.

use crate::app::{App, Dialog, Surface};
use crate::handlers::{
    handle_compare_key, handle_confirm_key, handle_ctrl_c, handle_dashboard_key, handle_help_key,
    handle_highlight_box_key, handle_hist_panel_key, handle_insert_key, handle_mouse_event,
    handle_normal_event, handle_open_file_panel_key, handle_palette_key, handle_picker_key,
    handle_preset_name_key, handle_stream_source_panel_key, handle_summary_key,
    handle_time_panel_key, picker_render_data, popup_rect,
};
use crate::live_ctl::LiveIngestCtl;
use crate::{app, input, preview, theme, ui};
use crossterm::event::{self, Event, KeyCode};
use ratatui::layout::Position;
use ratatui::layout::{Constraint, Layout};
use ratatui::Terminal;
use std::time::{Duration, Instant};

pub(crate) fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    input: &mut input::InputBox,
    live: &mut Option<LiveIngestCtl>,
) -> Result<(), String> {
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
        app.log_corpus.poll();
        if app.log_corpus.take_dirty() {
            app.refresh_open_file_choices();
        }
        app.ensure_vocab_candidates();
        app.poll_vocab_match();
        app.poll_summary_job();
        app.poll_hist_job();
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
                .draw(|frame| draw_frame(frame, app, input))
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
                if app.compare.is_some() {
                    continue;
                }
                handle_mouse_event(app, mouse);
                continue;
            }
            _ => continue,
        };

        route_key(app, input, live, key)?;
    }
    Ok(())
}

/// One frame: canvas, Dashboard or main layout, overlays, hardware caret.
fn draw_frame(frame: &mut ratatui::Frame, app: &mut App, input: &input::InputBox) {
    let frame_area = frame.area();
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme::canvas_style()),
        frame_area,
    );
    let mut hw_cursor: Option<Position> = None;

    let surface = app.surface();
    if app.dashboard.is_some() && surface != Surface::OpenFile {
        ui::render_dashboard(app, frame, frame_area);
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
    let preview_limit = ui::picker_preview_capacity(frame_area, ui::PICKER_PREVIEW_LEFT_RATIO);
    let preview_inner_w = ui::picker_preview_inner_width(frame_area, ui::PICKER_PREVIEW_LEFT_RATIO);
    match surface {
        Surface::OpenFile => {
            if let Some(panel) = app.open_file_panel.as_ref() {
                hw_cursor = ui::render_open_file_panel(
                    panel,
                    app.config.picker_left_ratio,
                    frame,
                    frame_area,
                );
            }
        }
        Surface::StreamSource => {
            if let Some(panel) = app.stream_source_panel.as_ref() {
                ui::render_stream_source_panel(panel, frame, frame_area);
            }
        }
        Surface::Picker => {
            if let Some(data) = picker_render_data(app, preview_limit, preview_inner_w) {
                let right_pane = match &data.preset_preview {
                    Some(lines) => ui::PickerRightPane::ChipRules(lines),
                    None => ui::PickerRightPane::Hits(&data.preview),
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
            }
        }
        // Search / Input use top stack: modal → candidates → Preview (H1).
        Surface::HighlightBox => {
            let area = ui::top_modal_rect(frame_area, modal_w, ui::search_modal_height());
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
                let preview_lines = preview::preview_search_lines(app, limit).unwrap_or_default();
                ui::render_preview("Preview", &preview_lines, "type to preview", frame, prev);
            }
        }
        Surface::Input => {
            let input_area = ui::top_modal_rect(frame_area, modal_w, 3);
            hw_cursor = ui::render_input_modal(input, app.mode, frame, input_area);
            let mut stack_bottom = input_area;
            if input.field_popup_visible() {
                // `.max(1)` so empty-match state still gets a row for "no match".
                let count = input.field_candidates().len().max(1);
                let rect = popup_rect(input_area, frame_area, count);
                ui::render_popup(input, frame, rect);
                stack_bottom = rect;
            }
            let prev = ui::preview_popup_rect(stack_bottom, frame_area);
            if prev.height > 0 {
                let limit = ui::preview_content_capacity(prev);
                let preview_lines = app.preview_filter_throttled(input, limit);
                ui::render_preview("Preview", &preview_lines, "no matches", frame, prev);
            }
        }
        Surface::TimePanel => {
            let h = ui::time_panel_height(frame_area);
            let area = ui::top_modal_rect(frame_area, modal_w.max(56), h);
            hw_cursor = ui::render_time_panel(app, frame, area);
        }
        Surface::CommandPalette => {
            hw_cursor = ui::render_command_palette(app, frame, frame_area);
        }
        Surface::Compare => {
            let area = ui::compare_modal_rect(frame_area);
            ui::render_compare_panel(app, frame, area);
        }
        Surface::Help => {
            let content_rows = crate::help::help_body_lines(app).len().max(1);
            let area = ui::help_modal_rect(frame_area, modal_w.max(56), content_rows);
            hw_cursor = ui::render_help_panel(app, frame, area);
        }
        Surface::Detail => {
            let inner_w = modal_w.saturating_sub(2).max(1);
            let content_rows = ui::detail_content_lines(app, inner_w).len().max(1);
            let h = ui::detail_modal_height(frame_area, content_rows);
            let area = ui::top_modal_rect(frame_area, modal_w, h);
            ui::render_detail(app, frame, area);
        }
        Surface::Summary => {
            let content_rows = ui::summary_content_row_count(app);
            let h = ui::summary_modal_height(frame_area, content_rows);
            let area = ui::top_modal_rect(frame_area, modal_w.max(56), h);
            ui::render_summary_panel(app, frame, area);
        }
        Surface::Hist => {
            let content_rows = ui::hist_content_row_count(app);
            let h = ui::hist_modal_height(frame_area, content_rows);
            let area = ui::top_modal_rect(frame_area, modal_w.max(56), h);
            ui::render_hist_panel(app, frame, area);
        }
        Surface::Dashboard | Surface::Main => {}
    }
    // Preset name dialog paints above picker (rename) or alone (save).
    if let Some(dialog) = app.preset_name.as_ref() {
        hw_cursor = ui::render_preset_name_dialog(dialog, frame, frame_area);
    }
    if let Some(confirm) = app.confirm.as_ref() {
        ui::render_confirm_dialog(confirm, frame, frame_area);
        hw_cursor = None;
    }
    if let Some(pos) = hw_cursor {
        frame.set_cursor_position(pos);
    }
}

/// Route one key event to the topmost owning surface (modal → panel → mode).
fn route_key(
    app: &mut App,
    input: &mut input::InputBox,
    live: &mut Option<LiveIngestCtl>,
    key: event::KeyEvent,
) -> Result<(), String> {
    // Dialogs and surfaces before Ctrl+C / Normal/Insert so Ctrl+C cancels
    // the draft like Esc, instead of quitting in Normal.
    match app.dialog() {
        Some(Dialog::Confirm) => {
            handle_confirm_key(app, key);
            return Ok(());
        }
        Some(Dialog::PresetName) => {
            handle_preset_name_key(app, key);
            return Ok(());
        }
        None => {}
    }
    match app.surface() {
        Surface::OpenFile => handle_open_file_panel_key(app, live, key),
        Surface::StreamSource => handle_stream_source_panel_key(app, live, key),
        Surface::Dashboard => handle_dashboard_key(app, live, key),
        Surface::Picker => handle_picker_key(app, key),
        Surface::TimePanel => handle_time_panel_key(app, key),
        Surface::HighlightBox => handle_highlight_box_key(app, key),
        Surface::CommandPalette => handle_palette_key(app, key),
        Surface::Compare => handle_compare_key(app, key),
        Surface::Help => handle_help_key(app, key),
        Surface::Summary => handle_summary_key(app, key),
        Surface::Hist => handle_hist_panel_key(app, key),
        Surface::Detail | Surface::Input | Surface::Main => return route_mode_key(app, input, key),
    }
    Ok(())
}

/// Keys with no owning overlay: Ctrl+C, then Normal / Insert mode handlers.
fn route_mode_key(
    app: &mut App,
    input: &mut input::InputBox,
    key: event::KeyEvent,
) -> Result<(), String> {
    use app::Mode;

    // Ctrl+C: quit from Normal (like `q`), but only cancel in-progress
    // input from Insert (like Esc) — mirrors the shell/readline
    // "abort current line" convention instead of nuking the session.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
        handle_ctrl_c(app, input);
        return Ok(());
    }

    match app.mode {
        Mode::Normal => handle_normal_event(app, input, key),
        Mode::Insert => handle_insert_key(app, input, key.code)?,
    }
    Ok(())
}
