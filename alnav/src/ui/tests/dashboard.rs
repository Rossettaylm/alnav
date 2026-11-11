//! Dashboard rendering tests.

use super::*;
use crate::app::App;
use crate::ui::text::{fit_display_end, fit_display_middle};
use ratatui::backend::TestBackend;
use ratatui::Terminal;
use unicode_width::UnicodeWidthStr;

fn dashboard_app(paths: Vec<String>) -> App {
    let mut app = App::new(100);
    let recent = crate::recent::RecentFiles { paths };
    app.recent = recent.clone();
    app.dashboard = Some(crate::dashboard::DashboardState::new(recent));
    app
}

fn render_dashboard_text(app: &App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_dashboard(app, frame, frame.area()))
        .unwrap();
    cell_text(terminal.backend().buffer())
}

#[test]
fn dashboard_full_logo_uses_per_line_theme_colors() {
    let p = crate::theme_builtins::palette_by_name("dracula").unwrap();
    crate::theme::install(crate::theme::map_to_tokens_for(&p, "dracula"));
    let app = dashboard_app(Vec::new());
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_dashboard(&app, frame, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer();
    let fg_on_row_containing = |needle: &str| -> Option<ratatui::style::Color> {
        for y in buf.area.y..buf.area.y + buf.area.height {
            let mut line = String::new();
            let mut sample = None;
            for x in buf.area.x..buf.area.x + buf.area.width {
                let cell = &buf[(x, y)];
                line.push_str(cell.symbol());
                if sample.is_none() && cell.symbol() != " " {
                    sample = Some(cell.fg);
                }
            }
            if line.contains(needle) {
                return sample;
            }
        }
        None
    };
    let top = fg_on_row_containing("█████╗");
    let bottom = fg_on_row_containing("╚══════╝");
    crate::theme::install(crate::theme::UiTokens::builtin());
    assert_eq!(top, Some(p.cyan));
    assert_eq!(bottom, Some(p.blue));
    assert_ne!(top, bottom);
}

#[test]
fn dashboard_renders_hyper_sections_and_footer() {
    let app = dashboard_app(vec!["/tmp/app.log".into()]);
    let text = render_dashboard_text(&app, 100, 30);

    assert!(text.contains("█████╗"));
    assert!(text.contains("╚══════╝"));
    assert!(text.contains("App / Android Log Navigator"));
    assert!(text.contains("Quick Actions"));
    assert!(text.contains("HDC — HarmonyOS hilog"));
    assert!(text.contains("ADB — Android logcat"));
    assert!(text.contains("Open file — Recent + configured log_dirs (fuzzy)"));
    assert!(text.contains("Recent Files"));
    assert!(text.contains("j/k move"));
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn dashboard_standard_terminal_keeps_full_header_and_footer() {
    let app = dashboard_app(Vec::new());
    let text = render_dashboard_text(&app, 80, 24);

    assert!(text.contains("█████╗"));
    assert!(text.contains("╚══════╝"));
    assert!(text.contains("App / Android Log Navigator"));
    assert!(text.contains("Quick Actions"));
    assert!(text.contains("Recent Files"));
    assert!(text.contains("j/k move"));
}

#[test]
fn dashboard_empty_recent_keeps_placeholder() {
    let app = dashboard_app(Vec::new());
    let text = render_dashboard_text(&app, 80, 24);

    assert!(text.contains("Recent Files"));
    assert!(text.contains("No recent files yet"));
}

#[test]
fn dashboard_short_frame_keeps_selected_recent_visible() {
    let paths = (1..=20).map(|i| format!("/tmp/file-{i:02}.log")).collect();
    let mut app = dashboard_app(paths);
    app.dashboard.as_mut().unwrap().cursor = 22;

    let text = render_dashboard_text(&app, 48, 12);
    assert!(text.contains("file-20.log"));
}

#[test]
fn dashboard_formats_home_path_and_ellipsizes_parent() {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/test".into());
    let path =
        format!("{home}/Work/a-very-long-project-name/another-long-directory/device/app.log");
    let app = dashboard_app(vec![path]);

    let text = render_dashboard_text(&app, 48, 24);
    assert!(text.contains("app.log"));
    assert!(text.contains("~/"));
    assert!(text.contains('…'));
}

#[test]
fn dashboard_renders_flash_in_reserved_row() {
    let mut app = dashboard_app(Vec::new());
    app.set_flash("HDC CONNECT FAILED");

    let text = render_dashboard_text(&app, 80, 24);
    assert!(text.contains("HDC CONNECT FAILED"));
}

#[test]
fn dashboard_recent_window_reports_range_and_caps_at_nine() {
    let paths = (1..=20).map(|i| format!("/tmp/file-{i:02}.log")).collect();
    let app = dashboard_app(paths);
    let text = render_dashboard_text(&app, 100, 30);

    assert!(text.contains("Recent Files  1-9 / 20"));
    assert!(text.contains("file-09.log"));
    assert!(!text.contains("file-10.log"));
}

#[test]
fn dashboard_hotkeys_are_right_aligned_in_content_column() {
    let app = dashboard_app(Vec::new());
    let text = render_dashboard_text(&app, 100, 30);
    let hdc_line = text.lines().find(|line| line.contains("HDC —")).unwrap();
    let hotkey_byte = hdc_line.find("[h]").unwrap();
    let hotkey_column = UnicodeWidthStr::width(&hdc_line[..hotkey_byte]);

    assert_eq!(hotkey_column, 83); // centered x=14 + (72 - "[h]".width)
}

#[test]
fn dashboard_minimal_keeps_wordmark_actions_and_selected_recent() {
    let mut app = dashboard_app(vec!["/tmp/selected.log".into()]);
    let text = render_dashboard_text(&app, 30, 8);
    assert!(text.contains("alnav"));
    assert!(text.contains("HDC"));
    assert!(text.contains("ADB"));
    assert!(text.contains("Open file"));

    app.dashboard.as_mut().unwrap().cursor = 3;
    let text = render_dashboard_text(&app, 30, 5);
    assert!(text.contains("HDC"));
    assert!(text.contains("ADB"));
    assert!(text.contains("Open file"));
    assert!(text.contains("selected.log"));
    assert!(!text.contains("alnav"));
}

#[test]
fn dashboard_flash_row_does_not_shift_footer() {
    let app = dashboard_app(Vec::new());
    let without_flash = render_dashboard_text(&app, 80, 24);
    let footer_before = without_flash
        .lines()
        .position(|line| line.contains("j/k move"))
        .unwrap();

    let mut app = app;
    app.set_flash("ADB CONNECT FAILED");
    let with_flash = render_dashboard_text(&app, 80, 24);
    let footer_after = with_flash
        .lines()
        .position(|line| line.contains("j/k move"))
        .unwrap();

    assert_eq!(footer_before, footer_after);
    assert!(with_flash.contains("ADB CONNECT FAILED"));
}

#[test]
fn dashboard_unicode_truncation_preserves_emoji_graphemes() {
    let end = fit_display_end("👩‍💻.log", 3);
    assert_eq!(end, "👩‍💻…");
    assert_eq!(UnicodeWidthStr::width(end.as_str()), 3);

    let middle = fit_display_middle("~/项目/👩‍💻/nearest/", 12);
    assert!(UnicodeWidthStr::width(middle.as_str()) <= 12);
    assert!(!middle.contains("👩‍") || middle.contains("👩‍💻"));
}
