//! Summary panel tests.

use super::*;
use crate::app::App;
use crate::ui::panels::summary::{bar_line, summary_report_lines};
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::Terminal;

#[test]
fn bar_line_proportional_fill_and_count_label() {
    let line = bar_line("E", 5, 10, 20, Style::default());
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        text.contains('█'),
        "partial bar should have some filled blocks"
    );
    assert!(
        text.contains('░'),
        "partial bar should have some empty blocks"
    );
    assert!(
        text.trim_end().ends_with("5"),
        "trailing count label: {text:?}"
    );

    let full = bar_line("E", 10, 10, 20, Style::default());
    let full_text: String = full.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        !full_text.contains('░'),
        "max count should fill the whole bar"
    );

    let empty = bar_line("E", 0, 10, 20, Style::default());
    let empty_text: String = empty.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        !empty_text.contains('█'),
        "zero count should have no filled blocks"
    );
}

fn fake_summary_report() -> alnav::summary::SummaryOutput {
    use std::collections::HashMap;
    let mut levels = HashMap::new();
    levels.insert('I', 3usize);
    levels.insert('E', 2usize);
    let mut tag_levels = HashMap::new();
    tag_levels.insert('I', 2usize);
    tag_levels.insert('E', 1usize);
    alnav::summary::SummaryOutput {
        total: 5,
        matched: 5,
        levels,
        top_tags: vec![alnav::summary::TagEntry {
            tag: "MyTag".into(),
            count: 3,
            levels: tag_levels,
        }],
        time_range: alnav::summary::TimeRange {
            first: "04-02 10:00:00.000".into(),
            last: "04-02 10:00:04.000".into(),
        },
        top_errors: vec![alnav::summary::ErrorEntry {
            pattern: "timeout after <N>ms".into(),
            count: 2,
            tag: "MyTag".into(),
            sample: "timeout after 100ms".into(),
        }],
        crashes: 1,
    }
}

#[test]
fn render_summary_panel_loading_shows_placeholder() {
    use crate::app::SummaryView;
    let mut app = App::new(100);
    app.summary_view = SummaryView::Loading;
    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 60, 20);
            render_summary_panel(&app, f, area);
        })
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(content.contains("Summary"));
    assert!(content.contains("computing"));
}

#[test]
fn render_summary_panel_ready_shows_sections() {
    use crate::app::SummaryView;
    let mut app = App::new(100);
    app.summary_view = SummaryView::Ready(fake_summary_report());
    let backend = TestBackend::new(60, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let area = Rect::new(0, 0, 60, 30);
            render_summary_panel(&app, f, area);
        })
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(content.contains("Rows: 5"));
    assert!(content.contains("Crashes: 1"));
    assert!(content.contains("Levels"));
    assert!(content.contains("Top tags"));
    assert!(content.contains("MyTag"));
    assert!(content.contains("Top errors"));
    assert!(content.contains("timeout after"));
}

#[test]
fn summary_content_row_count_matches_report_lines() {
    use crate::app::SummaryView;
    let mut app = App::new(100);
    assert_eq!(summary_content_row_count(&app), 1); // Closed clamps to >=1
    app.summary_view = SummaryView::Loading;
    assert_eq!(summary_content_row_count(&app), 1);
    let report = fake_summary_report();
    let expected = summary_report_lines(&report).len();
    app.summary_view = SummaryView::Ready(report);
    assert_eq!(summary_content_row_count(&app), expected);
}
