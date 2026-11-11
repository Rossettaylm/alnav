//! Picker / popup geometry tests.

use super::*;
use ratatui::layout::Rect;

#[test]
fn split_picker_lr_respects_ratio() {
    let area = Rect::new(0, 0, 100, 40);
    let (l, r) = split_picker_lr(area, 0.4);
    assert_eq!(l.width, 40);
    assert_eq!(r.width, 60);
    assert_eq!(l.height, r.height);
}

#[test]
fn picker_left_stack_search_at_bottom() {
    let left = Rect::new(0, 0, 40, 20);
    let (cand, search) = picker_left_stack(left, 0);
    assert_eq!(search.height, 3);
    assert_eq!(search.y + search.height, left.y + left.height);
    assert_eq!(cand.y, left.y);
    assert_eq!(cand.height, left.height - 3);

    let (cand, search) = picker_left_stack(left, 1);
    assert_eq!(search.height, 4);
    assert_eq!(search.y + search.height, left.y + left.height);
    assert_eq!(cand.height, left.height - 4);

    let (cand, search) = picker_left_stack(left, 2);
    assert_eq!(search.height, 5);
    assert_eq!(cand.height, left.height - 5);
}

#[test]
fn picker_frame_rect_compact_when_no_preview() {
    let frame = Rect::new(0, 0, 100, 40);
    let full = picker_frame_rect(frame, true);
    let compact = picker_frame_rect(frame, false);
    assert_eq!(compact.width, full.width / 2);
    assert_eq!(compact.height, full.height / 2);
    assert_eq!(
        compact.x,
        frame.x + (frame.width.saturating_sub(compact.width)) / 2
    );
    assert_eq!(
        compact.y,
        frame.y + (frame.height.saturating_sub(compact.height)) / 2
    );
}

#[test]
fn preview_popup_rect_fills_remaining_space() {
    let frame = Rect::new(0, 0, 80, 40);
    let anchor = Rect::new(10, 2, 40, 3);
    let prev = preview_popup_rect(anchor, frame);
    assert!(
        prev.height > 12,
        "should fill below modal, got {}",
        prev.height
    );
    assert_eq!(prev.y + prev.height, frame.y + frame.height);
}

#[test]
fn picker_preview_capacity_uses_right_pane_inner() {
    let frame = Rect::new(0, 0, 100, 40);
    let cap = picker_preview_capacity(frame, PICKER_PREVIEW_LEFT_RATIO);
    let picker = picker_frame_rect(frame, true);
    let (_l, r) = split_picker_lr_gapped(picker, PICKER_PREVIEW_LEFT_RATIO);
    assert_eq!(cap, r.height.saturating_sub(2) as usize);
    assert!(
        cap > 10,
        "tall picker should expose more than old PREVIEW_LIMIT"
    );
}

#[test]
fn picker_preview_inner_width_matches_right_pane() {
    let frame = Rect::new(0, 0, 120, 40);
    let w = picker_preview_inner_width(frame, PICKER_PREVIEW_LEFT_RATIO);
    let picker = picker_frame_rect(frame, true);
    let (_l, r) = split_picker_lr_gapped(picker, PICKER_PREVIEW_LEFT_RATIO);
    assert_eq!(w, r.width.saturating_sub(2));
    assert!(
        w > 40,
        "real preview pane must be wider than the old hardcode"
    );
}

#[test]
fn split_picker_lr_gapped_leaves_one_col() {
    let area = Rect::new(0, 0, 100, 40);
    let (l, r) = split_picker_lr_gapped(area, 0.4);
    assert_eq!(l.x + l.width + 1, r.x);
    assert_eq!(l.width + r.width + 1, area.width);
}
