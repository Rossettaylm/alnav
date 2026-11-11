//! Startup dashboard: logo, actions and recent files.

use super::clear_to_canvas;
use super::text::{fit_display_end, fit_display_middle};
use crate::app::App;
use crate::theme;
use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

const DASHBOARD_MAX_WIDTH: u16 = 72;

/// Full-frame startup Dashboard (unbound source), composed like a borderless
/// dashboard-nvim Hyper page rather than a popup.
pub fn render_dashboard(app: &App, frame: &mut Frame, area: Rect) {
    let Some(dash) = app.dashboard.as_ref() else {
        return;
    };
    if area.width == 0 || area.height == 0 {
        return;
    }

    clear_to_canvas(frame, area);
    let available_width = area.width.saturating_sub(4).max(1);
    let content_width = available_width.min(DASHBOARD_MAX_WIDTH).min(area.width);
    let density = crate::dashboard::DashboardDensity::for_size(content_width, area.height);
    let selected_recent =
        dash.cursor >= crate::dashboard::QUICK_ACTION_COUNT && dash.cursor < dash.len();
    let show_minimal_header = density == crate::dashboard::DashboardDensity::Minimal
        && area.height >= 5
        && (!selected_recent || area.height >= 6);
    let fixed_rows = density.fixed_rows(show_minimal_header).min(area.height);
    let recent_capacity = usize::from(area.height.saturating_sub(fixed_rows))
        .min(crate::dashboard::MAX_VISIBLE_RECENTS);
    let frame_height = fixed_rows
        .saturating_add(recent_capacity as u16)
        .min(area.height);
    let x = area.x + area.width.saturating_sub(content_width) / 2;
    let y = if density == crate::dashboard::DashboardDensity::Full {
        area.y + area.height.saturating_sub(frame_height) / 2
    } else {
        area.y + u16::from(area.height > frame_height)
    };
    let content = Rect::new(x, y, content_width, frame_height);

    let items = dash.items();
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(frame_height as usize);

    match density {
        crate::dashboard::DashboardDensity::Full => {
            lines.extend(
                theme::DASHBOARD_LOGO
                    .into_iter()
                    .enumerate()
                    .map(|(i, row)| {
                        Line::from(Span::styled(row, theme::dashboard_logo_line_style(i)))
                            .alignment(Alignment::Center)
                    }),
            );
            lines.push(
                Line::from(Span::styled(
                    "App / Android Log Navigator",
                    theme::dashboard_muted_style(),
                ))
                .alignment(Alignment::Center),
            );
            lines.push(Line::from(""));
        }
        crate::dashboard::DashboardDensity::Compact => {
            lines.push(
                Line::from(Span::styled("alnav", theme::dashboard_header_style()))
                    .alignment(Alignment::Center),
            );
        }
        crate::dashboard::DashboardDensity::Minimal if show_minimal_header => {
            lines.push(
                Line::from(Span::styled("alnav", theme::dashboard_header_style()))
                    .alignment(Alignment::Center),
            );
        }
        crate::dashboard::DashboardDensity::Minimal => {}
    }

    if density != crate::dashboard::DashboardDensity::Minimal {
        lines.push(dashboard_section_line(
            theme::GLYPH_TITLE_DASHBOARD,
            "Quick Actions",
        ));
    }
    for (idx, item) in items
        .iter()
        .take(crate::dashboard::QUICK_ACTION_COUNT)
        .enumerate()
    {
        lines.push(dashboard_item_line(
            item,
            idx == dash.cursor,
            content_width,
            density.show_descriptions(),
        ));
    }

    let range = dash.visible_recent_range(recent_capacity);
    if density != crate::dashboard::DashboardDensity::Minimal {
        let recent_title = if dash.recent.paths.len() > recent_capacity && !range.is_empty() {
            format!(
                "Recent Files  {}-{} / {}",
                range.start + 1,
                range.end,
                dash.recent.paths.len()
            )
        } else {
            "Recent Files".to_string()
        };
        lines.push(dashboard_section_line(
            theme::GLYPH_SOURCE_RECENT,
            &recent_title,
        ));
    }

    if dash.recent.paths.is_empty() {
        if recent_capacity > 0 {
            lines.push(Line::from(Span::styled(
                "No recent files yet",
                theme::dashboard_muted_style(),
            )));
            lines.extend(
                std::iter::repeat_with(|| Line::from("")).take(recent_capacity.saturating_sub(1)),
            );
        }
    } else {
        for recent_idx in range.clone() {
            let item_idx = crate::dashboard::QUICK_ACTION_COUNT + recent_idx;
            if let Some(item) = items.get(item_idx) {
                lines.push(dashboard_item_line(
                    item,
                    item_idx == dash.cursor,
                    content_width,
                    false,
                ));
            }
        }
        lines.extend(
            std::iter::repeat_with(|| Line::from(""))
                .take(recent_capacity.saturating_sub(range.len())),
        );
    }

    lines.push(
        Line::from(
            app.status_msg
                .as_deref()
                .map(|msg| Span::styled(msg.to_string(), theme::dashboard_flash_style()))
                .unwrap_or_else(|| Span::raw("")),
        )
        .alignment(Alignment::Center),
    );
    if density != crate::dashboard::DashboardDensity::Minimal {
        lines.push(
            Line::from(Span::styled(
                format!(
                    "j/k move  ·  Enter open  ·  q quit  ·  alnav v{}",
                    env!("CARGO_PKG_VERSION")
                ),
                theme::dashboard_muted_style(),
            ))
            .alignment(Alignment::Center),
        );
    }

    lines.truncate(content.height as usize);
    frame.render_widget(Paragraph::new(lines), content);
}

fn dashboard_section_line(glyph: &'static str, title: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!("{glyph}  {title}"),
        theme::dashboard_section_style(),
    ))
}

fn dashboard_item_line(
    item: &crate::dashboard::DashboardItem,
    selected: bool,
    width: u16,
    show_description: bool,
) -> Line<'static> {
    let row_style = theme::dashboard_item_style(selected);
    let marker = if selected {
        theme::candidate_prefix()
    } else {
        " ".repeat(UnicodeWidthStr::width(theme::candidate_prefix().as_str()).max(1))
    };
    let prefix = format!("{marker}{}  ", item.glyph());
    let hotkey = item
        .hotkey()
        .map(|key| format!("[{key}]"))
        .unwrap_or_default();
    let prefix_width = UnicodeWidthStr::width(prefix.as_str());
    let hotkey_width = UnicodeWidthStr::width(hotkey.as_str());
    let hotkey_gap = usize::from(!hotkey.is_empty());
    let label_budget = usize::from(width)
        .saturating_sub(prefix_width)
        .saturating_sub(hotkey_width)
        .saturating_sub(hotkey_gap);

    let mut spans = vec![Span::styled(prefix, row_style)];
    let label_width = match item {
        crate::dashboard::DashboardItem::Recent { path, .. } => {
            let (basename, parent) = dashboard_recent_parts(path, label_budget);
            let basename_width = UnicodeWidthStr::width(basename.as_str());
            spans.push(Span::styled(basename, row_style));
            if parent.is_empty() {
                basename_width
            } else {
                let parent_text = format!("  {parent}");
                let width = basename_width + UnicodeWidthStr::width(parent_text.as_str());
                spans.push(Span::styled(
                    parent_text,
                    theme::dashboard_description_style(selected),
                ));
                width
            }
        }
        _ => {
            let title = fit_display_end(&item.label(), label_budget);
            let mut used = UnicodeWidthStr::width(title.as_str());
            spans.push(Span::styled(title, row_style));
            if show_description {
                if let Some(description) = item.description() {
                    let remaining = label_budget.saturating_sub(used);
                    let separator = " — ";
                    let separator_width = UnicodeWidthStr::width(separator);
                    if remaining > separator_width {
                        let description = fit_display_end(description, remaining - separator_width);
                        let text = format!("{separator}{description}");
                        used += UnicodeWidthStr::width(text.as_str());
                        spans.push(Span::styled(
                            text,
                            theme::dashboard_description_style(selected),
                        ));
                    }
                }
            }
            used
        }
    };

    let used = prefix_width + label_width + hotkey_width + hotkey_gap;
    spans.push(Span::styled(
        " ".repeat(usize::from(width).saturating_sub(used)),
        row_style,
    ));
    if !hotkey.is_empty() {
        spans.push(Span::styled(" ", row_style));
        spans.push(Span::styled(
            hotkey,
            theme::dashboard_hotkey_style(selected),
        ));
    }
    Line::from(spans)
}

fn dashboard_recent_parts(path: &str, max_width: usize) -> (String, String) {
    let path_ref = std::path::Path::new(path);
    let basename = path_ref
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    let basename = fit_display_end(basename, max_width);
    let basename_width = UnicodeWidthStr::width(basename.as_str());
    if basename_width >= max_width {
        return (basename, String::new());
    }

    let parent = dashboard_parent_label(path_ref);
    let parent_budget = max_width.saturating_sub(basename_width + 2);
    if parent_budget < 3 {
        return (basename, String::new());
    }
    (basename, fit_display_middle(&parent, parent_budget))
}

fn dashboard_parent_label(path: &std::path::Path) -> String {
    let Some(parent) = path.parent() else {
        return String::new();
    };
    let mut label = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .and_then(|home| {
            parent.strip_prefix(&home).ok().map(|relative| {
                if relative.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    format!("~/{}", relative.display())
                }
            })
        })
        .unwrap_or_else(|| parent.display().to_string());
    if !label.ends_with(std::path::MAIN_SEPARATOR) {
        label.push(std::path::MAIN_SEPARATOR);
    }
    label
}
