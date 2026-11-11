//! Tests for Help hints, pages, search and status-bar hint fitting.

use super::*;
use crate::app::Focus;
use ratatui::text::Line;

fn app_with_focus(focus: Focus) -> App {
    let mut app = App::new(100);
    app.focus = focus;
    app
}

#[test]
fn context_kind_by_focus() {
    assert_eq!(
        context_kind(&app_with_focus(Focus::LogList)),
        ContextKind::LogList
    );
    assert_eq!(
        context_kind(&app_with_focus(Focus::ChipStrip)),
        ContextKind::ChipStrip
    );
    assert_eq!(
        context_kind(&app_with_focus(Focus::ExcludeStrip)),
        ContextKind::ExcludeStrip
    );
    assert_eq!(
        context_kind(&app_with_focus(Focus::HighlightStrip)),
        ContextKind::HighlightStrip
    );
    assert_eq!(
        context_kind(&app_with_focus(Focus::Input)),
        ContextKind::Input
    );
}

#[test]
fn loglist_entries_include_help() {
    let entries = context_entries(&app_with_focus(Focus::LogList));
    assert!(
        entries.iter().any(|e| e.key == "?" && e.label == "help"),
        "LogList L1 must expose ? help"
    );
}

#[test]
fn loglist_entries_include_wrap_toggle() {
    let entries = context_entries(&app_with_focus(Focus::LogList));
    assert!(
        entries.iter().any(|e| e.key == "w" && e.label == "wrap"),
        "LogList L1 must expose w wrap toggle"
    );
}

#[test]
fn context_loglist_live_appends_clear_hint() {
    let mut app = app_with_focus(Focus::LogList);
    assert_eq!(context_kind(&app), ContextKind::LogList);
    app.export_source = crate::export::ExportSource::Hdc { device: None };
    assert_eq!(context_kind(&app), ContextKind::LogListLive);
    let entries = context_entries(&app);
    assert!(
        entries.iter().any(|e| e.key == "C-l"),
        "live LogList hint must expose Ctrl-L clear"
    );
    assert!(
        !entries.iter().any(|e| e.key == "t"),
        "live LogList must not expose interactive time"
    );

    app.export_source = crate::export::ExportSource::Adb { device: None };
    assert_eq!(context_kind(&app), ContextKind::LogListLive);
}

#[test]
fn context_search_modal_overrides_focus() {
    let mut app = app_with_focus(Focus::LogList);
    app.highlight_box.editing = true;
    assert_eq!(context_kind(&app), ContextKind::HighlightModal);
}

#[test]
fn context_msg_chip_picker_overrides_focus() {
    let mut app = app_with_focus(Focus::LogList);
    app.open_picker(crate::picker::PickerKind::MsgChip {
        purpose: crate::picker::MsgChipPurpose::Chip { exclude: false },
    });
    assert_eq!(context_kind(&app), ContextKind::Picker);
}

#[test]
fn context_confirm_overrides_picker() {
    use crate::picker::{UnifiedId, UnifiedKind};

    let mut app = app_with_focus(Focus::LogList);
    app.open_unified_picker();
    app.request_delete_many(vec![
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
    assert_eq!(context_kind(&app), ContextKind::Confirm);
}

#[test]
fn context_pending_leader_is_l2() {
    let mut app = app_with_focus(Focus::LogList);
    app.pending_leader = true;
    assert_eq!(context_kind(&app), ContextKind::Leader);
}

#[test]
fn context_detail_overrides_focus() {
    let mut app = app_with_focus(Focus::LogList);
    app.detail = crate::app::DetailView::Fields;
    assert_eq!(context_kind(&app), ContextKind::Detail);
}

#[test]
fn context_pending_ops_are_l2() {
    let mut app = app_with_focus(Focus::LogList);
    app.pending_m = true;
    assert_eq!(context_kind(&app), ContextKind::Bookmark);
    app.pending_m = false;
    app.pending_lock = true;
    assert_eq!(context_kind(&app), ContextKind::Lock);
    app.pending_lock = false;
    app.pending_time = true;
    assert_eq!(context_kind(&app), ContextKind::Time);
    app.pending_time = false;
    app.pending_chip = true;
    assert_eq!(context_kind(&app), ContextKind::ChipField);
    app.pending_chip = false;
    app.pending_exclude = true;
    assert_eq!(context_kind(&app), ContextKind::ChipField);
    app.pending_exclude = false;
    app.pending_yank = true;
    assert_eq!(context_kind(&app), ContextKind::Yank);
    app.pending_yank = false;
    app.focus = Focus::ChipStrip;
    app.pending_d = true;
    assert_eq!(context_kind(&app), ContextKind::StripD);
}

#[test]
fn context_modal_beats_pending() {
    let mut app = app_with_focus(Focus::LogList);
    app.pending_m = true;
    app.highlight_box.editing = true;
    assert_eq!(context_kind(&app), ContextKind::HighlightModal);
}

#[test]
fn hint_spans_hide_when_too_narrow() {
    let app = app_with_focus(Focus::LogList);
    assert!(context_hint_spans(&app, MIN_HELP_WIDTH - 1).is_none());
}

#[test]
fn hint_spans_fit_without_colon() {
    let app = app_with_focus(Focus::LogList);
    let spans = context_hint_spans(&app, 200).expect("wide enough");
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(!text.contains(':'), "no colon separators: {text:?}");
    assert!(text.contains("help"), "expected help hint: {text:?}");
    assert!(
        text.contains("filter"),
        "idle LogList must show filter: {text:?}"
    );
    assert!(
        !text.contains("j/k move"),
        "idle LogList must not dump full L1: {text:?}"
    );
}

#[test]
fn status_idle_loglist_is_help_and_filter() {
    let app = app_with_focus(Focus::LogList);
    let entries = status_hint_entries(&app);
    let labels: Vec<&str> = entries.iter().map(|e| e.label).collect();
    assert_eq!(labels, ["help", "filter"], "{entries:?}");
    let live = {
        let mut app = app_with_focus(Focus::LogList);
        app.export_source = crate::export::ExportSource::Hdc { device: None };
        status_hint_entries(&app)
    };
    let live_labels: Vec<&str> = live.iter().map(|e| e.label).collect();
    assert_eq!(live_labels, ["help", "filter"], "{live:?}");
}

#[test]
fn status_idle_strip_is_help_and_del() {
    for focus in [Focus::ChipStrip, Focus::ExcludeStrip, Focus::HighlightStrip] {
        let entries = status_hint_entries(&app_with_focus(focus));
        let labels: Vec<&str> = entries.iter().map(|e| e.label).collect();
        assert_eq!(labels, ["help", "del…"], "{focus:?} {entries:?}");
    }
}

#[test]
fn status_pending_chip_lists_fields() {
    let mut app = app_with_focus(Focus::LogList);
    app.pending_chip = true;
    let spans = context_hint_spans(&app, 200).expect("wide enough");
    let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.contains("tag"), "{text:?}");
    assert!(text.contains("msg"), "{text:?}");
    assert!(
        !text.contains("c…"),
        "pending prefix must not leak: {text:?}"
    );
}

#[test]
fn status_pending_and_modal_keep_full_context_entries() {
    let labels = |app: &crate::app::App| -> Vec<&str> {
        status_hint_entries(app).iter().map(|e| e.label).collect()
    };
    let full = |app: &crate::app::App| -> Vec<&str> {
        context_entries(app).iter().map(|e| e.label).collect()
    };

    let mut app = app_with_focus(Focus::LogList);
    app.pending_chip = true;
    assert_eq!(labels(&app), full(&app), "pending must not use idle 1–2");

    app.pending_chip = false;
    app.detail = crate::app::DetailView::Fields;
    assert_eq!(labels(&app), full(&app), "Detail must expand full set");
    assert!(
        labels(&app).len() > 2,
        "Detail must not keep idle help+filter: {:?}",
        labels(&app)
    );

    app.detail = crate::app::DetailView::Closed;
    app.open_picker(crate::picker::PickerKind::Filter);
    assert_eq!(labels(&app), full(&app), "Picker must expand full set");
}

fn joined_lines(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn help_body_still_lists_move_cursor() {
    let app = app_with_focus(Focus::LogList);
    let home = joined_lines(&help_body_lines(&app));
    assert!(home.contains("move"), "{home}");
    let active = context_entries(&app);
    assert!(
        active.iter().any(|e| e.label == "move"),
        "Help Active must keep full LogList L1: {active:?}"
    );
    let log = joined_lines(
        &page_doc_lines(&app, HelpPage::Log)
            .into_iter()
            .map(|l| l.line)
            .collect::<Vec<_>>(),
    );
    assert!(log.contains("move cursor") || log.contains("move"), "{log}");
}

#[test]
fn help_available_on_strips_not_when_pending() {
    let mut app = app_with_focus(Focus::ChipStrip);
    assert!(help_available(&app));
    app.pending_d = true;
    assert!(!help_available(&app));
    app.pending_d = false;
    app.open_picker(crate::picker::PickerKind::Filter);
    assert!(!help_available(&app));
}

#[test]
fn help_home_has_active_and_toc() {
    let app = app_with_focus(Focus::LogList);
    let text = joined_lines(&help_body_lines(&app));
    assert!(text.contains("Active"), "{text}");
    assert!(text.contains("Filter"), "{text}");
    assert!(text.contains("1"), "{text}");
    assert!(!text.contains("All commands"), "{text}");
}

#[test]
fn adb_help_hides_time_and_shows_clear() {
    let mut app = app_with_focus(Focus::LogList);
    app.export_source = crate::export::ExportSource::Adb { device: None };
    let text = joined_lines(
        &page_doc_lines(&app, HelpPage::Session)
            .into_iter()
            .map(|l| l.line)
            .collect::<Vec<_>>(),
    );

    assert!(!text.contains("set / clear global time window"), "{text}");
    assert!(text.contains("clear buffered live logs"), "{text}");
}

#[test]
fn catalog_jk_details_match_fast_scroll_step() {
    let app = app_with_focus(Focus::LogList);
    let step = FAST_SCROLL_STEP.to_string();
    let log_entries = page_entries(&app, HelpPage::Log);
    let nav_jk = log_entries
        .iter()
        .find(|e| e.key.contains('/') && e.label == "jump")
        .expect("log jump");
    assert!(
        nav_jk.detail.contains(&step),
        "log jump detail {:?} must mention FAST_SCROLL_STEP={step}",
        nav_jk.detail
    );
}

#[test]
fn page_blurbs_are_at_most_five_lines() {
    for page in HelpPage::ALL {
        let n = page_blurb(page).len();
        assert!(n <= 5, "{:?} blurb has {n} lines", page);
        assert!(n >= 1, "{:?} missing blurb", page);
    }
}

#[test]
fn filter_page_lists_filter_new_overlays_omits_help_search() {
    let app = app_with_focus(Focus::LogList);
    let filter = page_entries(&app, HelpPage::Filter);
    assert!(filter.iter().any(|e| e.label == "filter new"), "{filter:?}");
    let highlight = page_entries(&app, HelpPage::Highlight);
    assert!(
        highlight.iter().any(|e| e.label == "find highlight"),
        "{highlight:?}"
    );
    let picker = joined_lines(
        &page_doc_lines(&app, HelpPage::Picker)
            .into_iter()
            .map(|l| l.line)
            .collect::<Vec<_>>(),
    );
    assert!(
        !picker.contains("force New for Filter / Highlight"),
        "Picker blurb must not call / force New: {picker}"
    );
    assert!(
        picker.contains("finds or creates") || picker.contains("find-or-create"),
        "{picker}"
    );
    let overlays = joined_lines(
        &page_doc_lines(&app, HelpPage::Overlays)
            .into_iter()
            .map(|l| l.line)
            .collect::<Vec<_>>(),
    );
    assert!(
        !overlays.contains("search help") && !overlays.to_lowercase().contains("/ search"),
        "Overlays must not list Help chrome /: {overlays}"
    );
    assert!(
        overlays.contains("command palette") || overlays.contains("C-p"),
        "{overlays}"
    );
}

#[test]
fn log_page_owns_wrap_visual_yank_overlays_do_not() {
    let app = app_with_focus(Focus::LogList);
    let log = page_entries(&app, HelpPage::Log);
    let labels: Vec<&str> = log.iter().map(|e| e.label).collect();
    assert!(labels.contains(&"wrap"), "{labels:?}");
    assert!(labels.contains(&"visual"), "{labels:?}");
    assert!(labels.contains(&"yank msg"), "{labels:?}");
    assert!(labels.contains(&"yank field"), "{labels:?}");
    let overlays = page_entries(&app, HelpPage::Overlays);
    let overlay_labels: Vec<&str> = overlays.iter().map(|e| e.label).collect();
    assert!(
        !overlay_labels.contains(&"wrap") && !overlay_labels.contains(&"visual"),
        "{overlay_labels:?}"
    );
    let session = page_entries(&app, HelpPage::Session);
    assert!(
        session.iter().any(|e| e.label == "bookmarks"),
        "{session:?}"
    );
}

#[test]
fn search_chip_is_ignore_case_substring() {
    let app = app_with_focus(Focus::LogList);
    let hits = search_help_hits(&app, "CHIP");
    assert!(!hits.is_empty(), "expected substring hits for CHIP");
    assert!(hits.iter().any(|h| {
        let text = if let Some(page) = h.page {
            page_doc_lines(&app, page)[h.line].text.clone()
        } else {
            home_doc_lines(&app)[h.line].text.clone()
        };
        text[h.start..h.end].eq_ignore_ascii_case("chip")
    }));
}

#[test]
fn rebound_key_shows_in_status_hints() {
    let mut app = app_with_focus(Focus::LogList);
    app.keymap = crate::keymap::merge_user_toml(
        r#"
[log_list]
move_down = "Down"
"#,
    )
    .unwrap();
    let entries = context_entries(&app);
    assert!(
        entries.iter().any(|e| e.key.contains("Down")),
        "custom move_down must appear: {entries:?}"
    );
}

#[test]
fn catalog_and_loglist_include_command_palette() {
    let app = app_with_focus(Focus::LogList);
    let entries = context_entries(&app);
    assert!(
        entries
            .iter()
            .any(|e| e.key == "C-p" && e.label == "palette"),
        "LogList Active must list C-p palette: {entries:?}"
    );
    let overlay = joined_lines(
        &page_doc_lines(&app, HelpPage::Overlays)
            .into_iter()
            .map(|l| l.line)
            .collect::<Vec<_>>(),
    );
    assert!(
        overlay.contains("open command palette") || overlay.contains("C-p"),
        "Overlays page must mention the palette binding: {overlay}"
    );
    let idle = status_hint_entries(&app);
    let labels: Vec<&str> = idle.iter().map(|e| e.label).collect();
    assert_eq!(labels, ["help", "filter"], "idle status stays two hints");
}

#[test]
fn command_palette_open_blocks_help_and_uses_palette_context() {
    let mut app = app_with_focus(Focus::LogList);
    app.open_command_palette();
    assert!(!help_available(&app));
    assert_eq!(context_kind(&app), ContextKind::CommandPalette);
    let labels: Vec<&str> = status_hint_entries(&app).iter().map(|e| e.label).collect();
    assert!(
        labels.contains(&"close") || labels.contains(&"run"),
        "{labels:?}"
    );
    assert!(!labels.contains(&"help") || labels.len() > 2);
}

#[test]
fn compare_panel_blocks_help_and_uses_compare_context() {
    let mut app = app_with_focus(Focus::LogList);
    let mut row = crate::model::EntryRow::from_line_or_raw("04-02 10:00:00.000  1  1 I T : x");
    row.row_id = 1;
    app.bookmarks
        .try_add(crate::bookmark::Bookmark::from_row(row))
        .unwrap();
    app.bookmark_row_ids.insert(1);
    app.open_compare_panel();
    assert!(!help_available(&app));
    assert_eq!(context_kind(&app), ContextKind::Compare);
    let labels: Vec<&str> = context_entries(&app).iter().map(|e| e.label).collect();
    assert!(labels.contains(&"jump"), "{labels:?}");
    assert!(labels.contains(&"yank"), "{labels:?}");
    assert!(!labels.iter().any(|l| *l == "manage"), "{labels:?}");
}

#[test]
fn pending_m_says_compare_not_manage() {
    let mut app = app_with_focus(Focus::LogList);
    app.pending_m = true;
    let labels: Vec<&str> = context_entries(&app).iter().map(|e| e.label).collect();
    assert!(labels.contains(&"compare"), "{labels:?}");
    assert!(!labels.contains(&"manage"), "{labels:?}");
}

#[test]
fn page_max_scroll_keeps_last_line_at_bottom() {
    assert_eq!(page_max_scroll(10, 20), 0);
    assert_eq!(page_max_scroll(25, 10), 15);
    assert_eq!(page_max_scroll(1, 1), 0);
    assert_eq!(page_max_scroll(8, 0), 7);
}
