//! Open-file and stream source panels.

use crate::theme;
use crate::ui::{
    centered_modal_rect, modal_width, render_modal_shell, render_picker, ActionKind,
    PickerRightPane,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

/// Open-file left pane: at least half width so basename-first labels stay readable.
const OPEN_FILE_LEFT_RATIO_FLOOR: f32 = 0.55;

/// Open-file source panel (`C-f`): left candidates + draft, right full path.
pub fn render_open_file_panel(
    panel: &crate::source_panel::OpenFilePanel,
    left_ratio: f32,
    frame: &mut Frame,
    frame_area: Rect,
) -> Option<Position> {
    let labels: Vec<String> = panel
        .choices
        .iter()
        .map(|c| {
            let glyph = match c {
                crate::source_panel::OpenFileChoice::Recent(_) => theme::GLYPH_SOURCE_RECENT,
                crate::source_panel::OpenFileChoice::Corpus { .. } => theme::GLYPH_SOURCE_FILE,
            };
            format!("{glyph} {}", c.display_label())
        })
        .collect();
    let styles = vec![Style::default(); labels.len()];
    let checked = vec![false; labels.len()];
    let actions = vec![ActionKind::None; labels.len()];
    let mut right_lines: Vec<Line<'static>> = Vec::new();
    if let Some(status) = &panel.corpus_status {
        right_lines.push(Line::from(Span::styled(status.clone(), theme::muted())));
        right_lines.push(Line::from(""));
    }
    match panel.selected_full_path() {
        Some(path) => {
            right_lines.push(Line::from(Span::styled(
                "full path".to_string(),
                theme::muted().add_modifier(Modifier::DIM),
            )));
            right_lines.push(Line::from(Span::raw(path)));
            if let Some(label) = panel.selected_corpus_label() {
                right_lines.push(Line::from(""));
                right_lines.push(Line::from(Span::styled(
                    "corpus".to_string(),
                    theme::muted().add_modifier(Modifier::DIM),
                )));
                right_lines.push(Line::from(Span::styled(label, theme::muted())));
            }
        }
        None => {
            right_lines.push(Line::from(Span::styled(
                "no selection".to_string(),
                theme::preview_placeholder_style(),
            )));
        }
    }
    let title = format!("{} Open file", theme::GLYPH_SOURCE_OPEN_FILE);
    let mode = crate::picker::PickerMode::New;
    let open_left = left_ratio.max(OPEN_FILE_LEFT_RATIO_FLOOR);
    render_picker(
        &title,
        &mode,
        panel.draft.as_str(),
        panel.draft.cursor(),
        panel.draft.as_str(),
        &[],
        false,
        None,
        &labels,
        &styles,
        &checked,
        &actions,
        panel.selected,
        "scanning log_dirs… or configure log_dirs · Ctrl-r refresh",
        PickerRightPane::PlainLines(&right_lines),
        open_left,
        true,
        Some(theme::GLYPH_SOURCE_OPEN_FILE),
        frame,
        frame_area,
    )
}

/// Centered HDC / ADB chooser (`C-g`).
pub fn render_stream_source_panel(
    panel: &crate::source_panel::StreamSourcePanel,
    frame: &mut Frame,
    frame_area: Rect,
) {
    let width = modal_width(frame_area.width).min(40);
    let height = 8;
    let area = centered_modal_rect(frame_area, width, height);
    let title = format!("{} Stream source", theme::GLYPH_SOURCE_HDC);
    let inner = render_modal_shell(&title, frame, area);
    let rows = [
        (0usize, theme::GLYPH_SOURCE_HDC, "HDC  hilog", "h"),
        (1usize, theme::GLYPH_SOURCE_ADB, "ADB  logcat", "a"),
    ];
    let mut lines = vec![Line::from(Span::styled(
        "j/k move  ·  Enter / h / a select  ·  Esc cancel",
        theme::muted(),
    ))];
    lines.push(Line::from(""));
    for (idx, glyph, label, hot) in rows {
        let selected = panel.selected == idx;
        let style = if selected {
            theme::candidate_selected_style()
        } else {
            theme::candidate_unselected_style()
        };
        let marker = if selected {
            theme::candidate_prefix()
        } else {
            " ".repeat(theme::candidate_prefix().chars().count().max(1))
        };
        lines.push(Line::from(vec![
            Span::styled(marker, style),
            Span::styled(format!("[{hot}] "), theme::muted()),
            Span::styled(format!("{glyph} {label}"), style),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
