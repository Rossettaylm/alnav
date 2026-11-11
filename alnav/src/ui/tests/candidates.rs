//! Candidate list viewport tests.

use super::*;
use ratatui::backend::TestBackend;
use ratatui::style::Style;
use ratatui::Terminal;

#[test]
fn candidate_viewport_range_bounds_paint_window() {
    assert_eq!(candidate_viewport_range(0, 0, 10), (0, 0));
    assert_eq!(candidate_viewport_range(5, 0, 10), (0, 5));
    assert_eq!(candidate_viewport_range(100, 0, 8), (0, 8));
    let (off, end) = candidate_viewport_range(100, 50, 8);
    assert_eq!(end - off, 8);
    assert!(off <= 50 && 50 < end);
    let (off, end) = candidate_viewport_range(100, 99, 8);
    assert_eq!(end, 100);
    assert_eq!(end - off, 8);
}

#[test]
fn candidate_list_viewport_paint_stays_fast_with_many_labels() {
    use std::time::Instant;
    let labels: Vec<String> = (0..crate::fuzzy::CANDIDATE_RESULT_CAP)
        .map(|i| format!("CandidateLabel_{i:03}"))
        .collect();
    let styles = vec![Style::default(); labels.len()];
    let backend = TestBackend::new(40, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    let t = Instant::now();
    terminal
        .draw(|f| {
            render_candidate_list(
                "list",
                &labels,
                &styles,
                &[],
                &[],
                200,
                "empty",
                "Cand",
                f,
                f.area(),
                false,
            );
        })
        .unwrap();
    // ViewportPaint: even 256 labels must not approach the old O(n) ~90ms path.
    assert!(
        t.elapsed().as_millis() < 50,
        "viewport paint took {:?} (expected << 50ms)",
        t.elapsed()
    );
}
