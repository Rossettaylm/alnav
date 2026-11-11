//! Log list, minimap and bookmark strip rendering tests.

use super::*;
use crate::app::{App, Focus};
use crate::highlight_model::HighlightGroup;
use crate::model::EntryRow;
use crate::theme;
use crate::ui::entry::render_entry_lines_ex;
use crate::ui::log_list::bookmark_summary_line;
use ratatui::backend::TestBackend;
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;

fn fg_of_needle(buf: &ratatui::buffer::Buffer, needle: &str) -> Option<Color> {
    for y in 0..buf.area.height {
        let mut text = String::new();
        let mut starts = Vec::new();
        for x in 0..buf.area.width {
            starts.push(text.len());
            text.push_str(buf[(x, y)].symbol());
        }
        if let Some(i) = text.find(needle) {
            let x = starts.iter().rposition(|&s| s <= i)? as u16;
            return Some(buf[(x, y)].fg);
        }
    }
    None
}

fn render_lines(app: &mut App, lines: &[&str]) -> ratatui::buffer::Buffer {
    let (tx, rx) = std::sync::mpsc::channel();
    for line in lines {
        tx.send(crate::model::EntryRow::from_line(line).unwrap())
            .unwrap();
    }
    drop(tx);
    app.drain(&rx);
    app.focus = Focus::Input;
    let backend = TestBackend::new(100, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_log_list(app, frame, frame.area()))
        .unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn test_render_log_list_shows_tag_and_msg() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello world")
            .unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);

    let backend = TestBackend::new(80, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();

    let content = cell_text(terminal.backend().buffer());
    assert!(content.contains("MyTag"));
    assert!(content.contains("hello world"));
}

#[test]
fn error_and_crash_rows_paint_tag_and_msg_in_theme_red() {
    crate::theme::install(crate::theme::UiTokens::builtin());
    let mut app = App::new(100);
    let buf = render_lines(
        &mut app,
        &[
            "04-02 10:00:00.000  1  1 I InfoTag : UNIQUEINFOMSG",
            "04-02 10:00:01.000  1  1 E ErrTag  : UNIQUEERRMSG",
            "04-02 10:00:02.000  1  1 F FatTag  : UNIQUEFATALMSG",
            "04-02 10:00:03.000  1  1 I AndroidRuntime: FATAL EXCEPTION: main UNIQUECRASH",
        ],
    );
    assert_ne!(
        fg_of_needle(&buf, "UNIQUEINFOMSG"),
        Some(Color::Red),
        "info msg must not use error red"
    );
    assert_eq!(fg_of_needle(&buf, "UNIQUEERRMSG"), Some(Color::Red));
    assert_eq!(fg_of_needle(&buf, "ErrTag"), Some(Color::Red));
    assert_eq!(fg_of_needle(&buf, "UNIQUEFATALMSG"), Some(Color::Red));
    assert_eq!(fg_of_needle(&buf, "UNIQUECRASH"), Some(Color::Red));
}

#[test]
fn test_render_log_list_highlights_selected_row_with_soft_gray_when_focused() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first").unwrap(),
    )
    .unwrap();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second").unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 1; // select the second row

    let backend = TestBackend::new(80, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();

    // Row 0 is the block's top border; content starts at y=1 (one row
    // per entry, no wrapping for these short messages) and x=1 (past
    // the left border column).
    let buf = terminal.backend().buffer();
    let selected_style = buf[(1, 2)].style();
    let unselected_style = buf[(1, 1)].style();
    assert_eq!(
        selected_style.bg,
        Some(Color::DarkGray),
        "focused selection must use the soft gray background"
    );
    assert_ne!(
        unselected_style.bg,
        Some(Color::DarkGray),
        "unselected rows must not get the selection background"
    );
    assert!(
        !selected_style.add_modifier.contains(Modifier::REVERSED),
        "must not use the old reverse-video style"
    );
}

#[test]
fn test_render_log_list_no_highlight_when_log_list_unfocused() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first").unwrap(),
    )
    .unwrap();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second").unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 1;
    app.focus = Focus::Input; // LogList no longer has keyboard focus

    let backend = TestBackend::new(80, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();

    let buf = terminal.backend().buffer();
    let selected_style = buf[(1, 2)].style();
    let unselected_style = buf[(1, 1)].style();
    assert_eq!(
        selected_style, unselected_style,
        "with LogList unfocused, the previously-selected row must look identical to any other row"
    );
}

#[test]
fn test_selection_preserves_keyword_highlight_bg() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : an error here")
            .unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.highlight_groups
        .groups
        .push(HighlightGroup::from_pattern("error").unwrap());
    app.focus = Focus::LogList;
    app.cursor = 0;

    let backend = TestBackend::new(80, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();

    let expected_hl = theme::highlight_style(0).bg;
    let buf = terminal.backend().buffer();
    // Scan the content row for a cell whose bg is the highlight color.
    let mut found = false;
    for x in 0..buf.area.width {
        if buf[(x, 1)].style().bg == expected_hl {
            found = true;
            break;
        }
    }
    assert!(found, "keyword highlight bg must survive selection overlay");
}

#[test]
fn test_render_log_list_bookmark_row_bg_priority() {
    // Bookmarked non-cursor rows get a faint lock wash; cursor and visual
    // override that wash. Priority: visual > cursor-selection > bookmark-wash.
    // The whole buffer is scanned (bookmark strip shifts list content, so
    // fixed row coords are unreliable).
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA   : first").unwrap(),
    )
    .unwrap();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I TagB   : second").unwrap(),
    )
    .unwrap();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:02.000  1  1 I TagC   : third").unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;

    // Bookmark rows 0 and 1; row 2 stays plain.
    app.cursor = 0;
    app.bookmark_add_current();
    app.cursor = 1;
    let bm_bg = theme::bookmark_row_style().bg;
    let vis_bg = theme::log_visual_style().bg;
    let sel_bg = theme::log_selection_style().bg;

    let scan_bg_in_rows =
        |terminal: &Terminal<TestBackend>, target: Option<Color>, y0: u16, y1: u16| -> bool {
            let buf = terminal.backend().buffer();
            for y in y0..y1 {
                for x in 0..buf.area.width {
                    if buf[(x, y)].style().bg == target {
                        return true;
                    }
                }
            }
            false
        };

    // Case 1: cursor on row 2 (focused, no visual selection). Rows 0 and 1
    // are bookmarked but not the cursor row → they get the bookmark bg;
    // the cursor row gets the selection gray.
    app.cursor = 2;
    app.focus = Focus::LogList;
    let backend = TestBackend::new(80, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();
    assert!(
        scan_bg_in_rows(&terminal, bm_bg, 0, terminal.backend().buffer().area.height),
        "bookmarked non-cursor row must get the bookmark bg"
    );
    assert!(
        scan_bg_in_rows(
            &terminal,
            sel_bg,
            0,
            terminal.backend().buffer().area.height
        ),
        "focused cursor row must get the selection bg"
    );

    // Case 1b: cursor on a bookmarked row uses selection wash (glyph stays).
    app.cursor = 0;
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();
    assert!(
        scan_bg_in_rows(
            &terminal,
            sel_bg,
            0,
            terminal.backend().buffer().area.height
        ),
        "cursor on a bookmark must use selection bg"
    );

    // Case 2: enter visual-line on row 0, extend cursor to row 1. Both
    // bookmarked rows are inside the visual range → visual overrides bookmark.
    // The bookmark strip occupies the top rows (1 border + 2 strip rows),
    // so list content starts at y=3; rows 0,1 land at y=3,4 and must carry
    // the visual bg, NOT the bookmark bg.
    app.enter_visual_line(); // anchor at row 0
    app.cursor = 1; // range [0,1]
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();
    assert!(
        scan_bg_in_rows(
            &terminal,
            vis_bg,
            3,
            terminal.backend().buffer().area.height
        ),
        "visual selection must override bookmark bg on list rows"
    );
    assert!(
        !scan_bg_in_rows(&terminal, bm_bg, 3, terminal.backend().buffer().area.height),
        "bookmark bg must yield to visual selection on list rows"
    );
}

#[test]
fn pinned_lineno_uses_bookmark_glyph() {
    let row =
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : msg").unwrap();
    let lines = render_entry_lines_ex(&row, &[], 200, Some((1, 1)), false, true);
    let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains(theme::GLYPH_BOOKMARK_PIN),
        "pinned lineno must include pin glyph: {text:?}"
    );
    let mark = lines[0]
        .spans
        .iter()
        .find(|s| s.content.as_ref() == theme::GLYPH_BOOKMARK_PIN)
        .expect("pin glyph span");
    assert_eq!(mark.style.fg, theme::bookmark_mark_style().fg);
}

#[test]
fn test_render_log_list_persists_scroll_offset_when_cursor_moves_within_viewport() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    for i in 0..30 {
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
    app.following = false;

    // Backend height 7 minus 2 border rows leaves a 5-row inner viewport.
    let backend = TestBackend::new(80, 7);
    let mut terminal = Terminal::new(backend).unwrap();

    app.cursor = 20;
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();
    let offset_at_edge = app.list_offset;
    assert!(
        offset_at_edge > 0,
        "cursor near the bottom of a 30-row list in a 5-row viewport must have scrolled"
    );

    app.cursor -= 2; // moves, but stays inside the already-scrolled viewport
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();

    assert_eq!(
        app.list_offset, offset_at_edge,
        "moving within the visible window must not re-scroll the viewport"
    );
}

#[test]
fn test_render_log_list_collapsed_view_produces_single_line_items() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    let long = "word ".repeat(40);
    tx.send(
        crate::model::EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : {long}"))
            .unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.following = false;
    app.collapsed_view = true;

    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer();
    let mut found_ellipsis = false;
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            if buf[(x, y)].symbol() == "…" {
                found_ellipsis = true;
            }
        }
    }
    assert!(
        found_ellipsis,
        "collapsed view must render truncated msg with ellipsis"
    );
}

#[test]
fn test_minimap_row_maps_ends() {
    assert_eq!(minimap_row_for_index(0, 100, 10), 0);
    assert_eq!(minimap_row_for_index(99, 100, 10), 9);
    assert_eq!(minimap_row_for_index(50, 100, 10), 4);
}

#[test]
fn test_build_minimap_marks_severe_and_search() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    for (i, level, msg) in [
        (0, "I", "ok"),
        (1, "E", "boom"),
        (2, "I", "findme here"),
        (3, "I", "ok"),
    ] {
        let _ = i;
        tx.send(
            EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 {level} Tag     : {msg}"))
                .unwrap(),
        )
        .unwrap();
    }
    drop(tx);
    app.drain(&rx);
    app.highlight_groups
        .groups
        .push(HighlightGroup::from_pattern("findme").unwrap());
    app.list_offset = 0;

    let marks = build_minimap_marks(&app, 4);
    assert_eq!(marks.len(), 4);
    assert!(marks.iter().any(|m| *m == MinimapMark::Severe));
    assert!(marks.iter().any(|m| *m == MinimapMark::Highlight));
    assert!(marks.iter().any(|m| *m == MinimapMark::Viewport));
    // Index 1 (E) maps near row 1 of 4.
    assert_eq!(marks[minimap_row_for_index(1, 4, 4)], MinimapMark::Severe);
}

#[test]
fn test_build_minimap_marks_file_uses_cache_and_hit_index() {
    use crate::store::FileStore;
    use std::io::Write;

    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(
        b"04-02 10:00:00.000  1  1 I Tag     : ok\n\
          04-02 10:00:01.000  1  1 E Tag     : boom\n\
          04-02 10:00:02.000  1  1 I Tag     : findme here\n\
          04-02 10:00:03.000  1  1 I Tag     : ok\n",
    )
    .unwrap();
    f.flush().unwrap();
    let mut app = App::new(100);
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    // Wait for severe prefetch so cache is warm (no UI row_at needed).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let done = app
            .index
            .store
            .as_file()
            .map(|fs| fs.progress().severe_done)
            .unwrap_or(true);
        if done {
            break;
        }
        if std::time::Instant::now() > deadline {
            panic!("severe prefetch timed out");
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    app.highlight_scan.hits = vec![2];
    app.highlight_scan.done = true;
    app.list_offset = 0;

    let marks = build_minimap_marks(&app, 4);
    assert!(marks.iter().any(|m| *m == MinimapMark::Severe));
    assert!(marks.iter().any(|m| *m == MinimapMark::Highlight));
    assert_eq!(marks[minimap_row_for_index(1, 4, 4)], MinimapMark::Severe);
    assert_eq!(
        marks[minimap_row_for_index(2, 4, 4)],
        MinimapMark::Highlight
    );
}

#[test]
fn test_build_minimap_marks_bookmark_over_highlight() {
    // F5: a bookmarked alive row produces a Bookmark mark; on overlap with
    // a Highlight mark, Bookmark wins (Severe > Bookmark > Highlight).
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I Tag     : findme").unwrap())
        .unwrap();
    tx.send(EntryRow::from_line("04-02 10:00:01.000  1  1 I Tag     : other").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.highlight_groups
        .groups
        .push(HighlightGroup::from_pattern("findme").unwrap());
    app.cursor = 0;
    app.bookmark_add_current();
    app.list_offset = 0;

    let marks = build_minimap_marks(&app, 4);
    assert!(
        marks.iter().any(|m| *m == MinimapMark::Bookmark),
        "bookmark row yields a Bookmark mark: {marks:?}"
    );
    // Index 0 is both highlighted and bookmarked; Bookmark must win there.
    assert_eq!(marks[minimap_row_for_index(0, 2, 4)], MinimapMark::Bookmark);
}

#[test]
fn test_build_minimap_empty_when_no_visible() {
    let app = App::new(100);
    assert!(build_minimap_marks(&app, 10).is_empty());
}

#[test]
fn test_render_log_list_draws_minimap_rail() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 E Tag     : err").unwrap())
        .unwrap();
    for i in 0..8 {
        tx.send(
            EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I Tag     : line{i}")).unwrap(),
        )
        .unwrap();
    }
    drop(tx);
    app.drain(&rx);

    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_log_list(&mut app, frame, frame.area()))
        .unwrap();
    let buf = terminal.backend().buffer();
    // Inner rightmost content col (width 40 → border at 0/39, inner 1..38, rail at 38).
    let rail_x = buf.area.width - 2;
    let mut found_mark = false;
    for y in 1..buf.area.height.saturating_sub(1) {
        let ch = buf[(rail_x, y)].symbol();
        if ch == "•" || ch == "│" {
            found_mark = true;
            break;
        }
    }
    assert!(
        found_mark,
        "minimap rail should paint │/• inside the log border"
    );
}

#[test]
fn bookmark_summary_is_one_line_not_wrapped_bodies() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    let long_msg = "word ".repeat(20);
    tx.send(
        crate::model::EntryRow::from_line(&format!(
            "04-02 10:00:00.000  1  1 I Tag     : {long_msg}"
        ))
        .unwrap(),
    )
    .unwrap();
    tx.send(
        crate::model::EntryRow::from_line("04-02 10:00:08.000  1  1 I Tag     : later").unwrap(),
    )
    .unwrap();
    drop(tx);
    app.drain(&rx);
    app.cursor = 0;
    app.bookmark_add_current();
    app.cursor = 1;
    app.bookmark_add_current();
    let line = app.bookmarks.summary_line();
    assert_eq!(line, "★ 2  10:00:00→10:00:08");
    let painted = bookmark_summary_line(&line);
    assert_eq!(painted.spans.len(), 1);
    assert!(!line.contains("word"), "summary must not embed pin bodies");
}
