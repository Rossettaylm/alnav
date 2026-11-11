//! Chip strip layout and pill style tests.

use super::*;
use crate::app::{App, Focus};
use crate::theme;
use ratatui::backend::TestBackend;
use ratatui::style::Modifier;
use ratatui::Terminal;

#[test]
fn test_chip_pill_and_highlight_pill_spans() {
    let tag = theme::chip_pill_spans(crate::input::ChipField::Tag, "MyTag", false);
    assert!(tag[0].content.contains("MyTag"));
    assert_eq!(tag[0].style.bg, Some(theme::accent()));
    let disabled = theme::chip_pill_spans(crate::input::ChipField::Msg, "x", true);
    assert_eq!(disabled[0].style, theme::disabled_chip_style());
    let search = theme::highlight_pill_spans("error", 0, false, false);
    assert_eq!(search[0].style, theme::highlight_style(0));
    let active = theme::highlight_pill_spans("error", 0, false, true);
    assert_eq!(active[0].style, theme::highlight_style_active(0));
    assert!(active[0].style.add_modifier.contains(Modifier::UNDERLINED));
}

#[test]
fn test_chip_strip_selection_keeps_stable_layout() {
    use crate::filter_model::Group;
    use crate::input::{Chip, ChipField};

    let mut app = App::new(100);
    app.groups.groups.push(Group {
        label: "a".into(),
        chips: vec![Chip {
            field: ChipField::Tag,
            value: "A".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.groups.groups.push(Group {
        label: "b".into(),
        chips: vec![Chip {
            field: ChipField::Msg,
            value: "B".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.focus = Focus::ChipStrip;
    app.group_cursor = 0;

    // Single content row + outer rounded chrome = height 3.
    let backend = TestBackend::new(60, 3);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| render_chip_strip(&app, frame, frame.area()))
        .unwrap();
    let before = cell_text(terminal.backend().buffer());

    app.group_cursor = 1;
    terminal
        .draw(|frame| render_chip_strip(&app, frame, frame.area()))
        .unwrap();
    let after = cell_text(terminal.backend().buffer());

    // Selection only restyles the group dot — glyph layout stays put.
    assert!(before.contains('A') && after.contains('A'));
    assert!(before.contains('B') && after.contains('B'));
    assert_eq!(
        before.chars().filter(|c| *c == 'A' || *c == 'B').count(),
        after.chars().filter(|c| *c == 'A' || *c == 'B').count()
    );
    // divider_block draws top + bottom horizontal rules (─), no rounded corners.
    let rules = before.chars().filter(|c| *c == '─').count();
    assert!(
        rules >= 2,
        "strip should have top+bottom ─ rules, got {rules}"
    );
    let rounded = before.chars().filter(|c| "╭╮╰╯".contains(*c)).count();
    assert_eq!(
        rounded, 0,
        "Filter strip must stay divider-only (no rounded modal corners)"
    );
}

#[test]
fn test_filter_strip_wraps_and_grows_height() {
    use crate::filter_model::Group;
    use crate::input::{Chip, ChipField};

    let mut app = App::new(100);
    for label in ["AAAA", "BBBB", "CCCC", "DDDD"] {
        app.groups.groups.push(Group {
            label: label.into(),
            chips: vec![Chip {
                field: ChipField::Tag,
                value: label.into(),
            }],
            enabled: true,
            same_field_op: crate::fuzzy::SameFieldOp::And,
        });
    }
    let h = filter_strip_height(&app, 20);
    assert!(
        h > 3,
        "wrapped strip should exceed one content row + chrome, got {h}"
    );
    assert_eq!(
        filter_strip_height(&app, 20),
        h,
        "height is instantaneous (stable)"
    );
    app.groups.groups.clear();
    assert_eq!(filter_strip_height(&app, 20), 0);
}
