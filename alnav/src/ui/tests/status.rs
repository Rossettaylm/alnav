//! Status bar rendering tests.

use super::*;
use crate::app::Pending;
use crate::app::{App, Focus};
use crate::highlight_model::HighlightGroup;
use crate::theme;
use ratatui::backend::TestBackend;
use ratatui::style::Color;
use ratatui::Terminal;

fn row0_bg_at_needle(buf: &ratatui::buffer::Buffer, needle: &str) -> Option<Color> {
    let mut text = String::new();
    let mut starts = Vec::new();
    for x in 0..buf.area.width {
        starts.push(text.len());
        text.push_str(buf[(x, 0)].symbol());
    }
    let i = text.find(needle)?;
    let x = starts.iter().rposition(|&s| s <= i)? as u16;
    Some(buf[(x, 0)].bg)
}

#[test]
fn test_render_status_bar_shows_highlight_match_stats() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : aaa").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I T   : hit one").unwrap())
        .unwrap();
    tx.send(crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I T   : hit two").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());

    let backend = TestBackend::new(40, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("-/2"),
        "cursor not on hit: got {content:?}"
    );
    assert!(
        !content.contains("[-/2]"),
        "match stats must not use brackets: got {content:?}"
    );

    app.cursor = 1;
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("1/2"),
        "first hit ordinal: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_shows_disconnect_icon_in_stream_mode() {
    let mut app = App::new(100);
    app.following = false;
    app.ingest_done = true;
    app.export_source = crate::export::ExportSource::Hdc { device: None };

    let backend = TestBackend::new(40, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains(theme::GLYPH_DISCONNECT),
        "stream mode + ingest_done should show disconnect icon: got {content:?}"
    );
    assert!(
        !content.contains(theme::GLYPH_SOURCE_HDC),
        "disconnect must hide source glyph: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_shows_source_icon_when_live_connected() {
    let mut app = App::new(100);
    app.following = false;
    app.ingest_done = false;
    app.export_source = crate::export::ExportSource::Adb { device: None };

    let backend = TestBackend::new(40, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains(theme::GLYPH_SOURCE_ADB),
        "connected live should show source glyph: got {content:?}"
    );
    assert!(
        !content.contains(theme::GLYPH_DISCONNECT),
        "connected live must not show disconnect: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_hides_disconnect_icon_in_file_mode() {
    use crate::store::FileStore;
    use std::io::Write;

    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(b"04-02 10:00:00.000  1  1 I Tag     : ok\n")
        .unwrap();
    f.flush().unwrap();
    let mut app = App::new(100);
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.following = false;
    app.ingest_done = true;

    let backend = TestBackend::new(40, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        !content.contains(theme::GLYPH_DISCONNECT),
        "file mode must never show disconnect icon: got {content:?}"
    );
    assert!(
        content.contains(theme::GLYPH_SOURCE_FILE),
        "file mode should show source file glyph: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_shows_context_help_when_wide() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::LogList;

    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("help") && content.contains("filter"),
        "wide idle bar should show curated hints: got {content:?}"
    );
    assert!(
        !content.contains("j/k"),
        "idle LogList must not dump full L1: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_hides_context_help_when_narrow() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::LogList;

    // Wide enough for cursor + follow + source, too tight for help (avail < 8).
    let backend = TestBackend::new(12, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains(theme::GLYPH_FOLLOWING),
        "follow icon must win over help: got {content:?}"
    );
    assert!(
        !content.contains("help") && !content.contains("j/k"),
        "narrow bar should hide help entirely: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_follow_visible_when_paused() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::LogList;

    let backend = TestBackend::new(40, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains(theme::GLYPH_FOLLOWING),
        "paused follow must still occupy a slot: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_pending_chip_has_fields_not_prefix() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::LogList;
    app.pending = Pending::Chip;

    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        !content.contains("c…"),
        "left cluster must not show pending prefix: got {content:?}"
    );
    assert!(
        content.contains("tag") && content.contains("msg"),
        "pending L2 must list chip fields: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_flash_pill_with_pending_l2() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::LogList;
    app.pending = Pending::Chip;
    app.set_flash("EXISTS");

    let backend = TestBackend::new(100, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer();
    let content = cell_text(buf);
    assert!(
        content.contains("EXISTS"),
        "flash pill must stay visible with pending L2: got {content:?}"
    );
    assert!(
        content.contains("tag"),
        "pending L2 must not cover the flash slot: got {content:?}"
    );
    assert_eq!(
        row0_bg_at_needle(buf, "EXISTS"),
        Some(theme::success()),
        "EXISTS flash must be a filled success pill: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_narrow_hides_hints_keeps_flash_floor() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::LogList;
    app.set_flash("EXISTS");

    // Wide enough for left icons + natural flash, too tight for FLASH_MIN
    // reservation + MIN_HELP_WIDTH — hints must hide first.
    let backend = TestBackend::new(24, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("EXISTS"),
        "flash floor must survive a tight row: got {content:?}"
    );
    assert!(
        !content.contains("help") && !content.contains("filter"),
        "hints must hide before the flash floor is eaten: got {content:?}"
    );
    assert!(
        content.contains(theme::GLYPH_FOLLOWING),
        "left follow slot must never yield: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_failed_flash_uses_warning_fill() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::LogList;
    app.set_flash("YANK FAILED");

    let backend = TestBackend::new(60, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer();
    let content = cell_text(buf);
    assert!(
        content.contains("YANK FAILED"),
        "failed flash copy must render: got {content:?}"
    );
    assert_eq!(
        row0_bg_at_needle(buf, "YANK FAILED"),
        Some(theme::warning()),
        "FAILED flash must use warning fill: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_strip_idle_is_help_and_del() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::ChipStrip;

    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("help") && content.contains("del"),
        "strip idle should show help + del: got {content:?}"
    );
    assert!(
        !content.contains("group"),
        "strip idle must not dump full L1: got {content:?}"
    );
}

#[test]
fn test_render_status_bar_picker_expands_full_hints() {
    let mut app = App::new(100);
    app.following = false;
    app.focus = Focus::LogList;
    app.open_picker(crate::picker::PickerKind::Filter);

    let backend = TestBackend::new(100, 1);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_status_bar(&mut app, frame, frame.area()))
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("select") || content.contains("close"),
        "picker must expand full context hints: got {content:?}"
    );
    assert!(
        !content.contains("? help") && !content.contains("?help"),
        "picker must not keep idle LogList 1–2: got {content:?}"
    );
}
