//! Command palette rendering tests.

use super::*;
use crate::app::App;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn command_palette_empty_query_hides_catalog() {
    let mut app = App::new(100);
    app.open_command_palette();
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let _ = render_command_palette(&app, f, f.area());
        })
        .unwrap();
    let text = cell_text(terminal.backend().buffer());
    assert!(
        text.contains("Command Palette"),
        "input shell title: {text:?}"
    );
    assert!(
        !text.contains("Add Filter"),
        "empty query must not list commands: {text:?}"
    );
}

#[test]
fn command_palette_filter_query_shows_add_filter_and_key() {
    let mut app = App::new(100);
    app.open_command_palette();
    app.command_palette
        .as_mut()
        .unwrap()
        .query
        .set_text("filter");
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let _ = render_command_palette(&app, f, f.area());
        })
        .unwrap();
    let text = cell_text(terminal.backend().buffer());
    assert!(text.contains("Add Filter"), "hits: {text:?}");
    assert!(
        text.contains(';'),
        "Filter New key hint should appear: {text:?}"
    );
}
