//! Popup / modal rect layout tests.

use crate::handlers::popup_rect;
use crate::ui;
use ratatui::layout::Rect;

#[test]
fn test_popup_rect_anchors_below_modal_and_clamps_to_space_below() {
    let modal = Rect {
        x: 12,
        y: 10,
        width: 40,
        height: 3,
    };
    let frame_area = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };
    let rect = popup_rect(modal, frame_area, 20); // way more matches than fit
                                                  // desired = min(8,20)+2 = 10; +1 gap → y = 14; space = 10; height = 10
    assert_eq!(rect.height, 10);
    assert_eq!(rect.y, 14, "popup should sit one row below the modal");
    assert_eq!(rect.x, modal.x);
    assert_eq!(rect.width, modal.width);
    assert!(
        rect.y > modal.y + modal.height,
        "popup must leave a gap below the modal"
    );
}

#[test]
fn test_popup_rect_clamps_when_little_space_below() {
    let modal = Rect {
        x: 12,
        y: 16,
        width: 40,
        height: 3,
    };
    let frame_area = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 20,
    };
    // space below = 1 (≤ gap) → pack flush; desired=8 → height=1
    let rect = popup_rect(modal, frame_area, 6);
    assert_eq!(rect.height, 1);
    assert_eq!(rect.y, 19);
}

#[test]
fn test_top_modal_rect_is_near_top() {
    let frame = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };
    let rect = ui::top_modal_rect(frame, 40, 3);
    assert_eq!(rect.y, 1);
    assert_eq!(rect.width, 40);
    let below = ui::stack_below_rect(rect, frame, 5);
    assert_eq!(below.y, rect.y + rect.height);
    assert_eq!(below.x, rect.x);
}

#[test]
fn test_centered_modal_rect_centers_and_clamps() {
    let frame = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 24,
    };
    let rect = ui::centered_modal_rect(frame, 40, 5);
    assert_eq!(rect.width, 40);
    assert_eq!(rect.height, 5);
    assert_eq!(rect.x, 20);
    assert_eq!(rect.y, 9);
    let tiny = ui::centered_modal_rect(
        Rect {
            x: 0,
            y: 0,
            width: 10,
            height: 4,
        },
        40,
        10,
    );
    assert_eq!(tiny.width, 10);
    assert_eq!(tiny.height, 4);
}
