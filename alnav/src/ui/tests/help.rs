//! Help panel rendering tests.

use super::*;
use crate::app::App;
use crate::theme;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;

fn render_help_text(app: &mut App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let _ = render_help_panel(app, frame, frame.area());
        })
        .unwrap();
    cell_text(terminal.backend().buffer())
}

#[test]
fn help_home_shows_numbered_filter_toc() {
    let mut app = App::new(100);
    app.open_help();
    let text = render_help_text(&mut app, 80, 24);
    assert!(text.contains("1"), "{text}");
    assert!(text.contains("Filter"), "{text}");
    assert!(text.contains("Active"), "{text}");
}

#[test]
fn help_short_frame_keeps_active_chrome_and_toc() {
    let mut app = App::new(100);
    app.open_help();
    let text = render_help_text(&mut app, 80, 8);
    assert!(text.contains("Active"), "{text}");
    assert!(
        text.contains("close") || text.contains("Esc"),
        "chrome should stay pinned: {text}"
    );
    assert!(
        text.contains("Log") || text.contains("Filter"),
        "TOC should keep a visible row: {text}"
    );
}

#[test]
fn help_exclude_page_shows_blurb() {
    let mut app = App::new(100);
    app.open_help();
    app.help_open_page(crate::help::HelpPage::Exclude);
    let text = render_help_text(&mut app, 80, 24);
    assert!(
        text.contains("AND NOT") || text.contains("Exclude"),
        "{text}"
    );
}

#[test]
fn help_modal_rect_is_vertically_centered() {
    let frame = Rect::new(0, 0, 80, 40);
    let rect = help_modal_rect(frame, 56, 10);
    let top = top_modal_rect(frame, 56, rect.height);
    assert!(rect.y > top.y, "help should sit below top-aligned modals");
    assert_eq!(rect.y, (frame.height - rect.height) / 2);
    assert_eq!(rect.x, (frame.width - rect.width) / 2);
}

#[test]
fn help_page_render_clamps_scroll_to_viewport() {
    let mut app = App::new(100);
    app.open_help();
    app.help_open_page(crate::help::HelpPage::Log);
    app.help_view = crate::help::HelpView::Page {
        id: crate::help::HelpPage::Log,
        scroll: 10_000,
    };
    let _ = render_help_text(&mut app, 80, 24);
    let n = crate::help::page_doc_lines(&app, crate::help::HelpPage::Log).len();
    let crate::help::HelpView::Page { scroll, .. } = app.help_view else {
        panic!("expected page");
    };
    assert!(
        scroll <= crate::help::page_max_scroll(n, app.help_body_view_h),
        "scroll={scroll} n={n} view={}",
        app.help_body_view_h
    );
    assert!(app.help_body_view_h > 1);
}

#[test]
fn help_search_hit_uses_theme_style() {
    let mut app = App::new(100);
    app.open_help();
    app.help_begin_search();
    if let Some(s) = app.help_search.as_mut() {
        s.query.set_text("chip");
    }
    app.help_rebuild_search();
    if let Some(s) = app.help_search.as_mut() {
        s.prompt = false;
    }
    app.help_jump_current_hit();
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let _ = render_help_panel(&mut app, frame, frame.area());
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    let want = theme::help_search_hit_style()
        .bg
        .or(theme::help_search_current_style().bg);
    let mut found = false;
    for y in buf.area.y..buf.area.y + buf.area.height {
        for x in buf.area.x..buf.area.x + buf.area.width {
            let cell = &buf[(x, y)];
            if cell.symbol().eq_ignore_ascii_case("c") && Some(cell.bg) == want {
                found = true;
            }
        }
    }
    assert!(found, "expected a help search hit cell with highlight bg");
}
