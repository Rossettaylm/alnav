//! Input box cursor tests.

use super::*;
use crate::app::Mode;
use crate::input::InputBox;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

#[test]
fn test_render_input_box_hw_cursor_after_committed_pill() {
    use crate::input::{Chip, ChipField};

    let mut input = InputBox::default();
    input.chips.push(Chip {
        field: ChipField::Tag,
        value: "MyTag".into(),
    });
    // Continue typing after the pill — the historical bug skipped caret
    // entirely once chips were non-empty.
    input.draft = "x".into();

    let backend = TestBackend::new(40, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut cursor = None;
    terminal
        .draw(|frame| {
            cursor = render_input_box(&input, Mode::Insert, true, frame, frame.area());
            if let Some(pos) = cursor {
                frame.set_cursor_position(pos);
            }
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    let content = cell_text(buf);
    assert!(content.contains("MyTag"));
    assert!(content.contains('x'));
    let pos = cursor.expect("Insert mode must report a hardware cursor");
    // Cursor sits after draft char 'x' on the content row.
    assert!(pos.x > 0, "cursor x={pos:?}");
    assert_eq!(pos.y, 1);
}
