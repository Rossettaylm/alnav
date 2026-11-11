//! Modal / picker geometry: widths, centered and stacked rects, picker splits and capacities.

use super::PICKER_SEARCH_HEIGHT;
use ratatui::layout::Rect;

/// Shared centered-modal width: leave 2 cols margin each side, clamp to a
/// readable band so Input and Search share one visual scale.
pub const MODAL_WIDTH_MIN: u16 = 24;

pub const MODAL_WIDTH_MAX: u16 = 56;

/// fzf picker outer frame: leave 2 cols margin each side.
const PICKER_FRAME_WIDTH_MARGIN: u16 = 4;

/// fzf picker height ≈ 75% of frame, clamped to this minimum.
const PICKER_FRAME_MIN_HEIGHT: u16 = 10;

/// Compact (no-preview) picker minimum height — lower so half-height can shrink.
const PICKER_FRAME_COMPACT_MIN_HEIGHT: u16 = 6;

/// Minimum width for each left/right pane inside the picker.
const PICKER_LR_MIN_WIDTH: u16 = 10;

/// Gap between adjacent popup surfaces (Picker L/R, modal → candidates → Preview).
const POPUP_GAP: u16 = 1;

/// Left/right split when the picker shows a Preview pane (right-biased).
pub const PICKER_PREVIEW_LEFT_RATIO: f32 = 0.3;

/// Unified width for centered Input / Search modals.
pub fn modal_width(frame_width: u16) -> u16 {
    frame_width
        .saturating_sub(4)
        .clamp(MODAL_WIDTH_MIN, MODAL_WIDTH_MAX)
}

/// Horizontally and vertically center a `width`×`height` rect inside `frame`.
pub fn centered_modal_rect(frame: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(frame.width).max(1);
    let height = height.min(frame.height).max(1);
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame.y + (frame.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Horizontally centered, vertically near the top (H1 Input/Search stack).
pub fn top_modal_rect(frame: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(frame.width).max(1);
    let height = height.min(frame.height).max(1);
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame
        .y
        .saturating_add(1)
        .min(frame.y.saturating_add(frame.height.saturating_sub(height)));
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Place a rect of `height` directly below `anchor`, clamped to `frame`.
pub fn stack_below_rect(anchor: Rect, frame: Rect, height: u16) -> Rect {
    let y = anchor.y.saturating_add(anchor.height);
    let frame_bottom = frame.y.saturating_add(frame.height);
    let space = frame_bottom.saturating_sub(y);
    let height = height.min(space).max(if space > 0 { 1 } else { 0 });
    Rect {
        x: anchor.x,
        y,
        width: anchor.width,
        height,
    }
}

/// Like [`stack_below_rect`], but leave [`POPUP_GAP`] rows when there is room;
/// if the remaining space is ≤ gap, pack flush (no gap) so a 1-row sliver still fits.
pub fn stack_below_rect_gapped(anchor: Rect, frame: Rect, height: u16) -> Rect {
    let flush_y = anchor.y.saturating_add(anchor.height);
    let frame_bottom = frame.y.saturating_add(frame.height);
    let space_flush = frame_bottom.saturating_sub(flush_y);
    if space_flush > POPUP_GAP {
        let gapped_anchor = Rect {
            x: anchor.x,
            y: anchor.y,
            width: anchor.width,
            height: anchor.height.saturating_add(POPUP_GAP),
        };
        stack_below_rect(gapped_anchor, frame, height)
    } else {
        stack_below_rect(anchor, frame, height)
    }
}

/// Horizontal center, height ≈ 75% of `frame` (clamped to a readable minimum).
/// When `show_preview` is false, width is ≈ half of the full picker width and
/// height is ≈ half of the full picker height (≈ 3/8 of frame).
pub fn picker_frame_rect(frame: Rect, show_preview: bool) -> Rect {
    let full_w = frame
        .width
        .saturating_sub(PICKER_FRAME_WIDTH_MARGIN)
        .max(PICKER_LR_MIN_WIDTH.saturating_mul(2));
    let width = if show_preview {
        full_w
    } else {
        (full_w / 2).max(PICKER_LR_MIN_WIDTH)
    };
    let full_height = (frame.height * 3 / 4)
        .max(PICKER_FRAME_MIN_HEIGHT)
        .min(frame.height);
    let height = if show_preview {
        full_height
    } else {
        (full_height / 2)
            .max(PICKER_FRAME_COMPACT_MIN_HEIGHT)
            .min(frame.height)
    };
    let x = frame.x + (frame.width.saturating_sub(width)) / 2;
    let y = frame.y + (frame.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// Split `area` into left/right panes by `left_ratio`; each pane is at least
/// [`PICKER_LR_MIN_WIDTH`] columns wide.
pub fn split_picker_lr(area: Rect, left_ratio: f32) -> (Rect, Rect) {
    let total = area.width;
    let mut left_w = ((total as f32) * left_ratio).round() as u16;
    left_w = left_w
        .max(PICKER_LR_MIN_WIDTH)
        .min(total.saturating_sub(PICKER_LR_MIN_WIDTH));
    let right_w = total.saturating_sub(left_w);
    let left = Rect {
        x: area.x,
        y: area.y,
        width: left_w,
        height: area.height,
    };
    let right = Rect {
        x: area.x + left_w,
        y: area.y,
        width: right_w,
        height: area.height,
    };
    (left, right)
}

/// Like [`split_picker_lr`], but leave [`POPUP_GAP`] columns between panes.
pub fn split_picker_lr_gapped(area: Rect, left_ratio: f32) -> (Rect, Rect) {
    let gap = POPUP_GAP.min(
        area.width
            .saturating_sub(PICKER_LR_MIN_WIDTH.saturating_mul(2)),
    );
    let usable = Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(gap),
        height: area.height,
    };
    let (left, right_inner) = split_picker_lr(usable, left_ratio);
    let right = Rect {
        x: left.x.saturating_add(left.width).saturating_add(gap),
        y: area.y,
        width: right_inner.width,
        height: area.height,
    };
    (left, right)
}

/// Left pane vertical stack: candidates fill the top, search area pinned to bottom.
/// `chip_rows` is the committed-chip band height above the rounded search input
/// (0 when empty); search total height is `PICKER_SEARCH_HEIGHT + chip_rows`.
pub fn picker_left_stack(left: Rect, chip_rows: u16) -> (Rect, Rect) {
    let search_h = PICKER_SEARCH_HEIGHT
        .saturating_add(chip_rows)
        .min(left.height);
    let cand_h = left.height.saturating_sub(search_h);
    let candidates = Rect {
        x: left.x,
        y: left.y,
        width: left.width,
        height: cand_h,
    };
    let search = Rect {
        x: left.x,
        y: left.y + cand_h,
        width: left.width,
        height: search_h,
    };
    (candidates, search)
}

/// Candidate popup height: `clamp(count,1,8)+2` for border, clamped to
/// space below the modal anchor (Input / Search / H7 msg share this).
pub fn candidate_popup_rect(anchor: Rect, frame: Rect, match_count: usize) -> Rect {
    let desired = match_count.clamp(1, 8) as u16 + 2;
    stack_below_rect_gapped(anchor, frame, desired)
}

/// H1 Preview window: fill remaining space below the previous stack item
/// (candidates or modal), leaving [`POPUP_GAP`] when possible.
pub fn preview_popup_rect(anchor: Rect, frame: Rect) -> Rect {
    let flush_y = anchor.y.saturating_add(anchor.height);
    let frame_bottom = frame.y.saturating_add(frame.height);
    let space_flush = frame_bottom.saturating_sub(flush_y);
    let height = if space_flush > POPUP_GAP {
        space_flush.saturating_sub(POPUP_GAP)
    } else {
        space_flush
    };
    stack_below_rect_gapped(anchor, frame, height)
}

/// Content rows inside a bordered Preview shell (`height - 2` for borders).
pub fn preview_content_capacity(area: Rect) -> usize {
    area.height.saturating_sub(2) as usize
}

/// Content rows available in the picker's right Preview pane for `frame`.
pub fn picker_preview_capacity(frame: Rect, left_ratio: f32) -> usize {
    let picker = picker_frame_rect(frame, true);
    let (_left, right) = split_picker_lr_gapped(picker, left_ratio);
    preview_content_capacity(right)
}

/// Content columns inside the picker's right Preview shell (`width - 2` for borders).
pub fn picker_preview_inner_width(frame: Rect, left_ratio: f32) -> u16 {
    let picker = picker_frame_rect(frame, true);
    let (_left, right) = split_picker_lr_gapped(picker, left_ratio);
    right.width.saturating_sub(2)
}

/// Search modal outer height: draft row + borders (candidates float below).
pub fn search_modal_height() -> u16 {
    3
}

/// Outer height for H4 Detail modal (border + content), clamped to `frame`.
pub fn detail_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let desired = (content_rows as u16).saturating_add(2).max(3);
    let max = frame.height.saturating_mul(3) / 5;
    let max = max.max(5).min(frame.height.saturating_sub(1));
    desired.min(max).max(3)
}

/// Height for the summary panel modal given frame size and content.
pub fn summary_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let max = frame.height.saturating_sub(4).max(8);
    let want = (content_rows as u16).saturating_add(2); // border
    want.min(max).max(8)
}

pub fn hist_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let max = frame.height.saturating_sub(4).max(10);
    let want = (content_rows as u16).saturating_add(2);
    want.min(max).max(10)
}

/// Outer height for the global time-window panel (`tt`).
pub fn time_panel_height(frame: Rect) -> u16 {
    // border(2) + 4 field rows + section labels(2) + up to 5 candidate rows
    let desired = 13u16;
    let max = frame.height.saturating_mul(3) / 5;
    let max = max.max(8).min(frame.height.saturating_sub(1));
    desired.min(max).max(8)
}

/// Command palette width: 60% of frame, clamped 40–72.
pub fn command_palette_width(frame: Rect) -> u16 {
    let sixty = (u32::from(frame.width) * 60 / 100) as u16;
    sixty.clamp(40, 72).min(frame.width).max(1)
}

/// Height for the Help modal given frame size and content.
pub fn help_modal_height(frame: Rect, content_rows: usize) -> u16 {
    let max = frame.height.saturating_sub(4).max(8);
    let want = (content_rows as u16).saturating_add(2); // border
    want.min(max).max(8)
}

/// Vertically centered Help shell (unlike Input/Search which stay near the top).
pub fn help_modal_rect(frame: Rect, width: u16, content_rows: usize) -> Rect {
    centered_modal_rect(frame, width, help_modal_height(frame, content_rows))
}

/// Large rounded compare tray covering most of the frame (the main log).
pub fn compare_modal_rect(frame: Rect) -> Rect {
    let width = frame.width.saturating_sub(4).max(40);
    let height = frame.height.saturating_sub(4).max(8);
    centered_modal_rect(frame, width, height)
}
