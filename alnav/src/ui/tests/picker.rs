//! Picker, preset preview and confirm dialog tests.

use super::*;
use crate::theme;
use crate::ui::picker::confirm_dialog_question;
use crate::ui::strips::committed_chip_rows;
use ratatui::backend::TestBackend;
use ratatui::layout::{Position, Rect};
use ratatui::Terminal;

#[test]
fn picker_search_line_has_rounded_border_padding_and_icon_gap() {
    let backend = TestBackend::new(20, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut cursor = None;
    terminal
        .draw(|frame| {
            cursor = render_picker_search_line(
                &crate::picker::PickerMode::Manage,
                "abc",
                3,
                &[],
                false,
                None,
                None,
                0,
                frame,
                frame.area(),
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    assert_eq!(buf[(0, 0)].symbol(), "╭");
    assert_eq!(buf[(19, 0)].symbol(), "╮");
    assert_eq!(buf[(0, 2)].symbol(), "╰");
    assert_eq!(buf[(19, 2)].symbol(), "╯");
    assert_eq!(buf[(1, 1)].symbol(), " ");
    assert_eq!(buf[(2, 1)].symbol(), theme::GLYPH_MODE_MANAGE);
    assert_eq!(buf[(3, 1)].symbol(), " ");
    assert_eq!(buf[(4, 1)].symbol(), " ");
    assert_eq!(buf[(5, 1)].symbol(), "a");
    assert_eq!(cursor, Some(Position { x: 8, y: 1 }));
}

#[test]
fn picker_search_line_cursor_after_last_char_when_full() {
    // area 20: border 2 → inner 18 → pad 1+1 → content 16; prefix icon+"  " = 3
    // → draft budget 13. Fill with 13 chars at end caret.
    let text = "abcdefghijklm"; // 13
    assert_eq!(text.len(), 13);
    let backend = TestBackend::new(20, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut cursor = None;
    terminal
        .draw(|frame| {
            cursor = render_picker_search_line(
                &crate::picker::PickerMode::Manage,
                text,
                text.chars().count(),
                &[],
                false,
                None,
                None,
                0,
                frame,
                frame.area(),
            );
        })
        .unwrap();

    let pos = cursor.expect("hardware cursor");
    // content.x = 2; content.width = 16 → caret after last char at x = 18
    // (right padding), still left of the right border at x = 19.
    assert_eq!(pos.x, 18, "cursor should sit after last visible char");
    assert_eq!(pos.y, 1);
    let buf = terminal.backend().buffer();
    assert_eq!(buf[(19, 1)].symbol(), "│");
    // Last draft char 'm' occupies content col 15 → screen x = 2+15 = 17
    assert_eq!(buf[(17, 1)].symbol(), "m");
    assert!(
        pos.x > 17,
        "cursor must be to the right of last char, got {:?}",
        pos
    );
}

#[test]
fn picker_search_line_keeps_committed_chips_above_border() {
    use crate::input::{Chip, ChipField};

    let chips = vec![Chip {
        field: ChipField::Tag,
        value: "MyTag".into(),
    }];
    let backend = TestBackend::new(40, 4);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let _ = render_picker_search_line(
                &crate::picker::PickerMode::Edit { index: 0 },
                "",
                0,
                &chips,
                false,
                None,
                None,
                1,
                frame,
                frame.area(),
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let content = cell_text(buf);
    assert!(content.lines().next().unwrap_or_default().contains("MyTag"));
    assert_eq!(buf[(0, 1)].symbol(), "╭");
    assert_eq!(buf[(39, 3)].symbol(), "╯");
}

#[test]
fn picker_committed_chips_wrap_and_squeeze_candidates() {
    use crate::input::{Chip, ChipField};

    // Narrow width forces each long pill onto its own row.
    let chips = vec![
        Chip {
            field: ChipField::Tag,
            value: "AAAAAAAA".into(),
        },
        Chip {
            field: ChipField::Msg,
            value: "BBBBBBBB".into(),
        },
    ];
    let width = 18u16;
    let left_h = 20u16;
    let rows = committed_chip_rows(&chips, false, width, left_h);
    assert!(rows >= 2, "expected wrap to >=2 rows, got {rows}");

    let left = Rect::new(0, 0, width, left_h);
    let (cand, search) = picker_left_stack(left, rows);
    assert_eq!(search.height, PICKER_SEARCH_HEIGHT + rows);
    assert_eq!(cand.height, left_h - search.height);
    assert!(cand.height >= 1);

    let backend = TestBackend::new(width, search.height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let _ = render_picker_search_line(
                &crate::picker::PickerMode::New,
                "",
                0,
                &chips,
                false,
                None,
                None,
                rows,
                frame,
                frame.area(),
            );
        })
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("AAAAAAAA"),
        "first chip visible: {content}"
    );
    assert!(
        content.contains("BBBBBBBB"),
        "wrapped second chip must remain visible: {content}"
    );
    // Second chip should not share the first chip row when wrapped.
    let lines: Vec<&str> = content.lines().collect();
    assert!(
        lines.len() >= 2,
        "chip band + input need multiple rows: {lines:?}"
    );
    assert!(
        lines[0].contains("AAAAAAAA"),
        "row0 should hold first chip: {:?}",
        lines[0]
    );
    assert!(
        lines[1].contains("BBBBBBBB"),
        "row1 should hold second chip: {:?}",
        lines[1]
    );
}

#[test]
fn picker_search_area_shows_committed_chip() {
    use crate::input::{Chip, ChipField};

    let chips = vec![Chip {
        field: ChipField::Tag,
        value: "MyTag".into(),
    }];
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let _ = render_picker(
                "Filter · Edit",
                &crate::picker::PickerMode::Edit { index: 0 },
                "",
                0,
                "",
                &chips,
                false,
                None,
                &[],
                &[],
                &[],
                &[],
                0,
                "no items",
                PickerRightPane::Hits(&[]),
                0.4,
                true,
                None,
                frame,
                frame.area(),
            );
        })
        .unwrap();

    let content = cell_text(terminal.backend().buffer());
    assert!(content.contains("MyTag"));
}

#[test]
fn picker_search_area_shows_confirmed_draft_field() {
    use crate::input::ChipField;

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let _ = render_picker(
                "Filter · New",
                &crate::picker::PickerMode::New,
                "",
                0,
                "",
                &[],
                false,
                Some(ChipField::Tag),
                &[],
                &[],
                &[],
                &[],
                0,
                "no items",
                PickerRightPane::Hits(&[]),
                0.4,
                true,
                None,
                frame,
                frame.area(),
            );
        })
        .unwrap();

    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("tag:"),
        "confirmed field must appear as tag: prefix, got: {content:?}"
    );
}

#[test]
fn confirm_dialog_renders_delete_one_copy_over_picker() {
    use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

    let confirm = ConfirmKind::DeleteMany {
        items: vec![UnifiedId {
            kind: UnifiedKind::Highlight,
            source_index: 0,
        }],
    };
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render_picker(
                "Manage",
                &crate::picker::PickerMode::Manage,
                "",
                0,
                "",
                &[],
                false,
                None,
                &["error".into()],
                &[theme::muted()],
                &[],
                &[],
                0,
                "no items",
                PickerRightPane::Hits(&[]),
                0.4,
                true,
                None,
                frame,
                frame.area(),
            );
            render_confirm_dialog(&confirm, frame, frame.area());
        })
        .unwrap();

    let content = cell_text(terminal.backend().buffer());
    assert_eq!(confirm_dialog_question(&confirm), "Delete selected?");
    assert!(content.contains("y/Enter"));
    assert!(content.contains("n/Esc"));
}

#[test]
fn render_picker_detail_shows_fields() {
    let row =
        crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA    : hello detail")
            .unwrap();
    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render_picker_detail(Some(&row), frame, frame.area());
        })
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(content.contains("Detail"));
    assert!(content.contains("TagA"));
    assert!(content.contains("hello detail"));
}

#[test]
fn render_picker_detail_stale_placeholder() {
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render_picker_detail(None, frame, frame.area());
        })
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(
        content.contains("Detail") && content.contains("row gone"),
        "stale detail must show placeholder, got: {content:?}"
    );
}

#[test]
fn confirm_dialog_renders_delete_many_count() {
    use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

    let confirm = ConfirmKind::DeleteMany {
        items: (0..12)
            .map(|i| UnifiedId {
                kind: UnifiedKind::Filter,
                source_index: i,
            })
            .collect(),
    };
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            render_confirm_dialog(&confirm, frame, frame.area());
        })
        .unwrap();

    let content = cell_text(terminal.backend().buffer());
    assert_eq!(confirm_dialog_question(&confirm), "Delete 12 items?");
    assert!(content.contains("12"));
}

#[test]
fn preset_preview_uses_width_so_chips_stay_unwrapped() {
    use crate::preset::{Preset, PresetChip, PresetFilterGroup, PRESET_VERSION};

    let preset = Preset {
        version: PRESET_VERSION,
        name: "trace".into(),
        filters: vec![PresetFilterGroup {
            chips: vec![
                PresetChip {
                    field: "tag".into(),
                    value: "NTKernel".into(),
                },
                PresetChip {
                    field: "msg".into(),
                    value: "OidbSvcTrpcTcp".into(),
                },
                PresetChip {
                    field: "msg".into(),
                    value: "trace=".into(),
                },
            ],
        }],
        excludes: vec![],
        highlights: vec![],
    };
    let narrow = preset_preview_lines(&preset, 40);
    let wide = preset_preview_lines(&preset, 80);
    // Title "Filter" + wrapped chip rows. Narrow width forces an extra wrap.
    let narrow_chip_rows = narrow.len().saturating_sub(1);
    let wide_chip_rows = wide.len().saturating_sub(1);
    assert!(
        narrow_chip_rows > wide_chip_rows,
        "width=40 wraps earlier than width=80: narrow={narrow_chip_rows} wide={wide_chip_rows}"
    );
    assert_eq!(
        wide_chip_rows, 1,
        "three chips of the trace preset fit on one row at width=80"
    );
}

#[test]
fn confirm_dialog_centers_on_frame() {
    use crate::picker::{ConfirmKind, UnifiedId, UnifiedKind};

    let confirm = ConfirmKind::DeleteMany {
        items: vec![UnifiedId {
            kind: UnifiedKind::Filter,
            source_index: 0,
        }],
    };
    let frame = Rect::new(0, 0, 100, 40);
    let width = 34.min(frame.width).max(1);
    let height = 5.min(frame.height).max(1);
    let area = centered_modal_rect(frame, width, height);
    assert_eq!(area.x, frame.x + (frame.width.saturating_sub(width)) / 2);
    assert_eq!(area.y, frame.y + (frame.height.saturating_sub(height)) / 2);

    let backend = TestBackend::new(100, 40);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_confirm_dialog(&confirm, f, f.area());
        })
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(content.contains("y/Enter"));
    assert!(content.contains("n/Esc"));
}

#[test]
fn confirm_dialog_clear_all_copy() {
    let confirm = crate::picker::ConfirmKind::ClearAll;
    assert_eq!(confirm_dialog_question(&confirm), "Clear all rules?");
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            render_confirm_dialog(&confirm, f, f.area());
        })
        .unwrap();
    let content = cell_text(terminal.backend().buffer());
    assert!(content.contains("Clear all rules?"), "{content:?}");
}
