//! Compare tray rendering tests.

use super::*;
use crate::app::App;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn compare_panel_paints_pid_and_ignores_collapsed_view() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:00.000  42  99 I Tag     : hello").unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 0;
    app.bookmark_add_current();
    app.collapsed_view = true;
    app.open_compare_panel();
    let backend = TestBackend::new(80, 16);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_compare_panel(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(content.contains("Bookmark Compare"), "{content}");
    assert!(content.contains("42"), "pid must render: {content}");
    assert!(content.contains("99"), "tid must render: {content}");
    assert!(content.contains("hello"), "{content}");
}
