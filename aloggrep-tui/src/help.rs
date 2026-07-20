//! Focus-aware keybinding hints for the status bar (H6).
//!
//! All user-facing short help copy lives here so later keybinding work
//! (H2/H7/…) only updates one table.

use crate::app::{App, Focus};

/// Minimum remaining character budget before we bother showing help.
pub const MIN_HELP_WIDTH: usize = 8;

/// Short keybinding hint for the current focus / modal state.
pub fn context_help(app: &App) -> &'static str {
    if app.search_box.editing {
        return "Space 入草稿  Enter/Tab 确认  Esc 取消";
    }
    if app.msg_chip_picker.is_some() {
        return "输入过滤  Enter/Tab 确认  Esc 取消";
    }
    if app.bookmark_picker.is_some() {
        return "输入过滤  Enter 跳转  Esc 取消";
    }
    if app.detail_open() {
        return "p 关  P 字段/Pretty  c/C+字段  j/k 换行  Esc 关浮层";
    }
    match app.focus {
        Focus::Input => "Space 入草稿  Enter 收pill/提交  ! 排除模式(空时)  Esc 取消",
        Focus::ChipStrip => "h/l 组  dd 删  di 禁用",
        Focus::ExcludeStrip => "h/l 排除  dd 删  di 禁用  (全局 AND NOT)",
        Focus::SearchStrip => "h/l 组  dd 删  di 禁用",
        Focus::LogList => {
            "j/k  e/E  p/P  ma/mm/md  yc  c/C+字段  fp/ft/fu  / 搜索  Esc 跟随"
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
        assert_eq!(
            context_help(&app_with_focus(Focus::LogList)),
            "j/k  e/E  p/P  ma/mm/md  yc  c/C+字段  fp/ft/fu  / 搜索  Esc 跟随"
        );
        assert_eq!(
            context_help(&app_with_focus(Focus::ChipStrip)),
            "h/l 组  dd 删  di 禁用"
        );
        assert_eq!(
            context_help(&app_with_focus(Focus::ExcludeStrip)),
            "h/l 排除  dd 删  di 禁用  (全局 AND NOT)"
        );
        assert_eq!(
            context_help(&app_with_focus(Focus::SearchStrip)),
            "h/l 组  dd 删  di 禁用"
        );
        assert_eq!(
            context_help(&app_with_focus(Focus::Input)),
            "Space 入草稿  Enter 收pill/提交  ! 排除模式(空时)  Esc 取消"
        );
    }

    #[test]
    fn context_help_search_modal_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.search_box.editing = true;
        assert_eq!(
            context_help(&app),
            "Space 入草稿  Enter/Tab 确认  Esc 取消"
        );
    }

    #[test]
    fn context_help_msg_chip_picker_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.msg_chip_picker = crate::input::MsgChipPicker::open("hello world", false);
        assert_eq!(
            context_help(&app),
            "输入过滤  Enter/Tab 确认  Esc 取消"
        );
    }

    #[test]
    fn context_help_detail_overrides_focus() {
        let mut app = app_with_focus(Focus::LogList);
        app.detail = crate::app::DetailView::Fields;
        assert_eq!(
            context_help(&app),
            "p 关  P 字段/Pretty  c/C+字段  j/k 换行  Esc 关浮层"
        );
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
