//! Focus-aware keybinding hints for the status bar (H6).
//!
//! Two levels: L1 shows single keys / multi-key prefixes; L2 shows the
//! follow-up keys while an operator is pending. All copy uses `键:短名`.

use crate::app::{App, Focus};

/// Minimum remaining character budget before we bother showing help.
pub const MIN_HELP_WIDTH: usize = 8;

const L1_LOGLIST: &str =
    "j/k:移 Esc:随 Space:管 ;滤 /亮 `排 mm签 n/N:跳 e/E:错 m:书签 f:锁 t:时 c:滤 C:排 y:拷 p/P:详";
/// LogList L1 when session is `--hdc` (adds Ctrl-L clear; no interactive time window).
const L1_LOGLIST_HDC: &str =
    "j/k:移 Esc:随 Space:管 ;滤 /亮 `排 mm签 n/N:跳 e/E:错 m:书签 f:锁 c:滤 C:排 y:拷 p/P:详 ^L:清屏";
const L1_CHIP_STRIP: &str = "h/l:组 d:删… Tab:切 Esc:随";
const L1_EXCLUDE_STRIP: &str = "h/l:组 d:删… Tab:切 Esc:随";
const L1_HIGHLIGHT_STRIP: &str = "h/l:组 d:删… Tab:切 Esc:随";
const L1_INPUT: &str = "Space:草稿 Enter:收/交 !:排除 Esc:取消";
const L1_HIGHLIGHT_MODAL: &str = "Space:草稿 Enter/Tab:确认 Esc:取消";
const L1_PICKER: &str = "输入:过滤 ↑/↓:选择 Tab:多选 Enter:启停 ^X:改 Del/^⌫:删 Esc:关闭";
const L1_CONFIRM: &str = "y/Enter:确认 n/Esc:取消";
const L1_DETAIL: &str = "p:关 P:切 c/C:滤 j/k:行 Esc:关";
const L1_TIME_PANEL: &str = "Tab/Enter:下栏 ↑↓:日期 Esc:取消";

const L2_LEADER: &str = "Space:管理面板 Esc:取消";
const L2_BOOKMARK: &str = "a:新增 d:删除 m:管理 Esc:取消";
const L2_LOCK: &str = "p:pid t:tid u:清 Esc:取消";
const L2_TIME: &str = "s:设窗 u:清除 Esc:取消";
const L2_CHIP_FIELD: &str = "t:tag m:msg g:pkg p:pid T:tid l:级 Esc:取消";
const L2_YANK: &str = "c:CLI t:tag m:msg g:pkg p:pid T:tid l:级 r:原 y:行 s:时 Esc:取消";
const L2_STRIP_D: &str = "d:删 i:禁用 Esc:取消";

/// Short keybinding hint for the current focus / modal / pending state.
pub fn context_help(app: &App) -> &'static str {
    if app
        .picker
        .as_ref()
        .is_some_and(|session| session.confirm.is_some())
    {
        return L1_CONFIRM;
    }
    if app.picker.is_some() {
        return L1_PICKER;
    }
    if app.highlight_box.editing {
        return L1_HIGHLIGHT_MODAL;
    }
    if app.time_panel.is_some() {
        return L1_TIME_PANEL;
    }
    if app.detail_open() {
        return L1_DETAIL;
    }

    // Operator-pending (L2) takes priority over Focus L1.
    if app.pending_leader {
        return L2_LEADER;
    }
    if app.pending_m {
        return L2_BOOKMARK;
    }
    if app.pending_lock {
        return L2_LOCK;
    }
    if app.pending_time {
        return L2_TIME;
    }
    if app.pending_chip || app.pending_exclude {
        return L2_CHIP_FIELD;
    }
    if app.pending_yank {
        return L2_YANK;
    }
    if app.pending_d {
        return L2_STRIP_D;
    }

    match app.focus {
        Focus::Input => L1_INPUT,
        Focus::ChipStrip => L1_CHIP_STRIP,
        Focus::ExcludeStrip => L1_EXCLUDE_STRIP,
        Focus::HighlightStrip => L1_HIGHLIGHT_STRIP,
        Focus::LogList => {
            if matches!(app.export_source, crate::export::ExportSource::Hdc { .. }) {
                L1_LOGLIST_HDC
            } else {
                L1_LOGLIST
            }
        }
    }
}

/// Fit `help` into `max_chars` (character count, matching `ui::span_width`).
///
/// Returns `None` when the budget is below [`MIN_HELP_WIDTH`] so a tiny
/// stub does not clutter the bar.
pub fn fit_help(help: &str, max_chars: usize) -> Option<&str> {
    if max_chars < MIN_HELP_WIDTH {
        return None;
    }
    let count = help.chars().count();
    if count <= max_chars {
        return Some(help);
    }
    let end = help
        .char_indices()
        .nth(max_chars)
        .map(|(i, _)| i)
        .unwrap_or(help.len());
    Some(&help[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Focus;

    fn app_with_focus(focus: Focus) -> App {
        let mut app = App::new(100);
        app.focus = focus;
        app
    }

    #[test]
    fn context_help_by_focus() {
        assert_eq!(context_help(&app_with_focus(Focus::LogList)), L1_LOGLIST);
        assert_eq!(
            context_help(&app_with_focus(Focus::ChipStrip)),
            L1_CHIP_STRIP
        );
        assert_eq!(
            context_help(&app_with_focus(Focus::ExcludeStrip)),
            L1_EXCLUDE_STRIP
        );
        assert_eq!(
            context_help(&app_with_focus(Focus::HighlightStrip)),
            L1_HIGHLIGHT_STRIP
        );
        assert_eq!(context_help(&app_with_focus(Focus::Input)), L1_INPUT);
    }

    #[test]
    fn context_help_loglist_hdc_appends_clear_hint() {
        let mut app = app_with_focus(Focus::LogList);
        assert_eq!(context_help(&app), L1_LOGLIST);
        app.export_source = crate::export::ExportSource::Hdc { device: None };
        assert_eq!(context_help(&app), L1_LOGLIST_HDC);
        assert!(
            L1_LOGLIST_HDC.contains("^L:清屏"),
            "hdc LogList hint must expose Ctrl-L clear"
        );
    }

    #[test]
    fn context_help_search_modal_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.highlight_box.editing = true;
        assert_eq!(context_help(&app), L1_HIGHLIGHT_MODAL);
    }

    #[test]
    fn context_help_msg_chip_picker_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.open_picker(crate::picker::PickerKind::MsgChip { exclude: false });
        assert_eq!(context_help(&app), L1_PICKER);
    }

    #[test]
    fn context_help_confirm_overrides_picker() {
        use crate::picker::{UnifiedId, UnifiedKind};

        let mut app = app_with_focus(Focus::LogList);
        app.open_unified_picker();
        app.picker.as_mut().unwrap().request_delete_many(vec![
            UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index: 0,
            },
            UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index: 1,
            },
            UnifiedId {
                kind: UnifiedKind::Highlight,
                source_index: 2,
            },
        ]);
        assert_eq!(context_help(&app), L1_CONFIRM);
    }

    #[test]
    fn context_help_pending_leader_lists_picker_shortcuts() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_leader = true;
        assert_eq!(context_help(&app), L2_LEADER);
    }

    #[test]
    fn context_help_detail_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.detail = crate::app::DetailView::Fields;
        assert_eq!(context_help(&app), L1_DETAIL);
    }

    #[test]
    fn context_help_pending_m_is_l2() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_m = true;
        assert_eq!(context_help(&app), L2_BOOKMARK);
    }

    #[test]
    fn context_help_pending_lock_is_l2() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_lock = true;
        assert_eq!(context_help(&app), L2_LOCK);
    }

    #[test]
    fn context_help_pending_time_is_l2() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_time = true;
        assert_eq!(context_help(&app), L2_TIME);
    }

    #[test]
    fn context_help_pending_chip_and_exclude_share_field_l2() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_chip = true;
        assert_eq!(context_help(&app), L2_CHIP_FIELD);
        app.pending_chip = false;
        app.pending_exclude = true;
        assert_eq!(context_help(&app), L2_CHIP_FIELD);
    }

    #[test]
    fn context_help_pending_yank_is_l2() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_yank = true;
        assert_eq!(context_help(&app), L2_YANK);
    }

    #[test]
    fn context_help_pending_d_is_l2() {
        let mut app = app_with_focus(Focus::ChipStrip);
        app.pending_d = true;
        assert_eq!(context_help(&app), L2_STRIP_D);
    }

    #[test]
    fn context_help_modal_beats_pending() {
        let mut app = app_with_focus(Focus::LogList);
        app.pending_m = true;
        app.highlight_box.editing = true;
        assert_eq!(context_help(&app), L1_HIGHLIGHT_MODAL);
    }

    #[test]
    fn fit_help_hides_when_too_narrow() {
        assert_eq!(fit_help("abcdefghij", MIN_HELP_WIDTH - 1), None);
    }

    #[test]
    fn fit_help_passthrough_and_truncate() {
        let s = "abcdefghijklmnop";
        assert_eq!(fit_help(s, 20), Some(s));
        assert_eq!(fit_help(s, 8), Some("abcdefgh"));
    }
}
