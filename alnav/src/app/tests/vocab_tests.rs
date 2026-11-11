//! Vocab intake tests.

use super::*;

#[test]
fn push_row_feeds_vocab() {
    let mut app = App::new(100);
    let row = crate::model::EntryRow::from_line(
        "01-01 00:00:00.000  1234  1234 I VocabTag: hello world test123",
    );
    app.push_row(row.unwrap());
    let cands = app.vocab.tag_candidates("Vocab");
    assert!(
        !cands.is_empty(),
        "VocabTag should appear in tag candidates"
    );
    assert_eq!(cands[0], "VocabTag");
}
