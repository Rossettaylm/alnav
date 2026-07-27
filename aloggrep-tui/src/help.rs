//! Focus-aware keybinding hints for the status bar and Help panel.
//!
//! Two levels: L1 shows single keys / multi-key prefixes; L2 shows the
//! follow-up keys while an operator is pending. Status-bar rendering uses
//! dim keys + normal labels with spacing (no `:` / `|` separators). Help
//! reuses the same entries as a detailed Active block plus a full catalog.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::theme;

/// Minimum remaining character budget before we bother showing help.
pub const MIN_HELP_WIDTH: usize = 8;

/// Shared `J`/`K` step for LogList cursor movement and Help panel scroll.
pub const FAST_SCROLL_STEP: isize = 7;

/// One keybinding hint (status short label + optional longer Help detail).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintEntry {
    pub key: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
}

impl HintEntry {
    const fn new(key: &'static str, label: &'static str, detail: &'static str) -> Self {
        Self { key, label, detail }
    }

    const fn short(key: &'static str, label: &'static str) -> Self {
        Self {
            key,
            label,
            detail: label,
        }
    }
}

/// Which situational hint set is active (drives L1/L2 + Help Active).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    Confirm,
    Picker,
    HighlightModal,
    TimePanel,
    Detail,
    Leader,
    Bookmark,
    Lock,
    Time,
    ChipField,
    Yank,
    StripD,
    Input,
    ChipStrip,
    ExcludeStrip,
    HighlightStrip,
    LogList,
    LogListHdc,
}

impl ContextKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Confirm => "Confirm",
            Self::Picker => "Picker",
            Self::HighlightModal => "Highlight edit",
            Self::TimePanel => "Time window",
            Self::Detail => "Detail",
            Self::Leader => "Leader",
            Self::Bookmark => "Bookmark",
            Self::Lock => "Lock",
            Self::Time => "Time",
            Self::ChipField => "Field",
            Self::Yank => "Yank",
            Self::StripD => "Strip delete",
            Self::Input => "Input",
            Self::ChipStrip => "Filter strip",
            Self::ExcludeStrip => "Exclude strip",
            Self::HighlightStrip => "Highlight strip",
            Self::LogList => "Log list",
            Self::LogListHdc => "Log list (live)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionId {
    Navigation,
    LeaderPickers,
    Operators,
    Session,
    Overlays,
    Help,
}

/// Fixed catalog chapter.
#[derive(Debug, Clone, Copy)]
pub struct HintSection {
    pub id: SectionId,
    pub title: &'static str,
    pub entries: &'static [HintEntry],
}

// ---------------------------------------------------------------------------
// L1 / L2 entry tables
// ---------------------------------------------------------------------------

const L1_LOGLIST: &[HintEntry] = &[
    HintEntry::short("j/k", "move"),
    HintEntry::new("Esc", "follow", "resume following"),
    HintEntry::new("?", "help", "open help"),
    HintEntry::new("Space", "menu", "leader then Space for manage"),
    HintEntry::new(";", "filter", "open filter new"),
    HintEntry::new("/", "highlight", "open highlight new"),
    HintEntry::new("`", "exclude", "open exclude new"),
    HintEntry::new("mm", "marks", "bookmark manage"),
    HintEntry::short("n/N", "next"),
    HintEntry::short("e/E", "error"),
    HintEntry::new("m", "mark", "bookmark operator"),
    HintEntry::new("f", "lock", "lock pid/tid"),
    HintEntry::new("t", "time", "time window"),
    HintEntry::new("c", "chip", "filter from row"),
    HintEntry::new("C", "exclude", "exclude from row"),
    HintEntry::new("y", "yank", "yank operator"),
    HintEntry::new("p/P", "detail", "fields / pretty"),
];

const L1_LOGLIST_HDC: &[HintEntry] = &[
    HintEntry::short("j/k", "move"),
    HintEntry::new("Esc", "follow", "resume following"),
    HintEntry::new("?", "help", "open help"),
    HintEntry::new("Space", "menu", "leader then Space for manage"),
    HintEntry::new(";", "filter", "open filter new"),
    HintEntry::new("/", "highlight", "open highlight new"),
    HintEntry::new("`", "exclude", "open exclude new"),
    HintEntry::new("mm", "marks", "bookmark manage"),
    HintEntry::short("n/N", "next"),
    HintEntry::short("e/E", "error"),
    HintEntry::new("m", "mark", "bookmark operator"),
    HintEntry::new("f", "lock", "lock pid/tid"),
    HintEntry::new("c", "chip", "filter from row"),
    HintEntry::new("C", "exclude", "exclude from row"),
    HintEntry::new("y", "yank", "yank operator"),
    HintEntry::new("p/P", "detail", "fields / pretty"),
    HintEntry::new("^L", "clear", "clear buffered logs"),
];

const L1_CHIP_STRIP: &[HintEntry] = &[
    HintEntry::short("h/l", "group"),
    HintEntry::new("d", "del…", "dd delete / di disable"),
    HintEntry::short("Tab", "focus"),
    HintEntry::new("Esc", "follow", "resume following"),
    HintEntry::new("?", "help", "open help"),
];

const L1_EXCLUDE_STRIP: &[HintEntry] = L1_CHIP_STRIP;
const L1_HIGHLIGHT_STRIP: &[HintEntry] = L1_CHIP_STRIP;

const L1_INPUT: &[HintEntry] = &[
    HintEntry::new("Space", "draft", "space in draft"),
    HintEntry::new("Enter", "commit", "pill then submit group"),
    HintEntry::new("!", "exclude", "toggle exclude draft"),
    HintEntry::short("Esc", "cancel"),
];

const L1_HIGHLIGHT_MODAL: &[HintEntry] = &[
    HintEntry::new("Space", "draft", "space in draft"),
    HintEntry::new("Enter/Tab", "ok", "confirm pattern"),
    HintEntry::short("Esc", "cancel"),
];

const L1_PICKER: &[HintEntry] = &[
    HintEntry::short("type", "filter"),
    HintEntry::short("↑/↓", "select"),
    HintEntry::new("Tab", "multi", "toggle multi-select"),
    HintEntry::new("Enter", "toggle", "enable/disable or submit"),
    HintEntry::new("^X", "edit", "edit selected"),
    HintEntry::new("Del/^⌫", "delete", "delete with confirm"),
    HintEntry::short("Esc", "close"),
];

const L1_CONFIRM: &[HintEntry] = &[
    HintEntry::short("y/Enter", "confirm"),
    HintEntry::short("n/Esc", "cancel"),
];

const L1_DETAIL: &[HintEntry] = &[
    HintEntry::short("p", "close"),
    HintEntry::short("P", "swap"),
    HintEntry::new("c/C", "chip", "filter / exclude field"),
    HintEntry::short("j/k", "row"),
    HintEntry::short("Esc", "close"),
];

const L1_TIME_PANEL: &[HintEntry] = &[
    HintEntry::new("Tab/Enter", "next", "next field"),
    HintEntry::short("↑↓", "date"),
    HintEntry::short("Esc", "cancel"),
];

const L2_LEADER: &[HintEntry] = &[
    HintEntry::new("Space", "manage", "open manage panel"),
    HintEntry::short("Esc", "cancel"),
];

const L2_BOOKMARK: &[HintEntry] = &[
    HintEntry::short("a", "add"),
    HintEntry::short("d", "delete"),
    HintEntry::short("m", "manage"),
    HintEntry::short("Esc", "cancel"),
];

const L2_LOCK: &[HintEntry] = &[
    HintEntry::short("p", "pid"),
    HintEntry::short("t", "tid"),
    HintEntry::short("u", "clear"),
    HintEntry::short("Esc", "cancel"),
];

const L2_TIME: &[HintEntry] = &[
    HintEntry::short("s", "set"),
    HintEntry::short("u", "clear"),
    HintEntry::short("Esc", "cancel"),
];

const L2_CHIP_FIELD: &[HintEntry] = &[
    HintEntry::short("t", "tag"),
    HintEntry::short("m", "msg"),
    HintEntry::short("g", "pkg"),
    HintEntry::short("p", "pid"),
    HintEntry::short("T", "tid"),
    HintEntry::short("l", "level"),
    HintEntry::short("Esc", "cancel"),
];

const L2_YANK: &[HintEntry] = &[
    HintEntry::short("c", "cli"),
    HintEntry::short("t", "tag"),
    HintEntry::short("m", "msg"),
    HintEntry::short("g", "pkg"),
    HintEntry::short("p", "pid"),
    HintEntry::short("T", "tid"),
    HintEntry::short("l", "level"),
    HintEntry::short("r", "raw"),
    HintEntry::short("y", "line"),
    HintEntry::short("s", "time"),
    HintEntry::short("Esc", "cancel"),
];

const L2_STRIP_D: &[HintEntry] = &[
    HintEntry::short("d", "delete"),
    HintEntry::short("i", "disable"),
    HintEntry::short("Esc", "cancel"),
];

// ---------------------------------------------------------------------------
// Full catalog (file mode). Hdc omits time-interactive entries via filter.
// ---------------------------------------------------------------------------

const CAT_NAVIGATION: &[HintEntry] = &[
    HintEntry::new("j/k", "move", "move cursor one line"),
    HintEntry::new("J/K", "jump", "move 7 lines"),
    HintEntry::new(
        "g/G",
        "top/bottom",
        "jump top or bottom (G does not resume follow)",
    ),
    HintEntry::new("Esc", "follow", "resume following and pin to bottom"),
    HintEntry::new("n/N", "next hit", "next / previous highlight match"),
    HintEntry::new("e/E", "error", "next / previous severe line"),
    HintEntry::new(
        "1-5",
        "focus",
        "focus filter / exclude / highlight / log / input",
    ),
];

const CAT_LEADER: &[HintEntry] = &[
    HintEntry::new("Space Space", "manage", "unified manage picker"),
    HintEntry::new(";", "filter new", "open filter picker in new mode"),
    HintEntry::new("/", "highlight new", "open highlight picker in new mode"),
    HintEntry::new("`", "exclude new", "open exclude picker in new mode"),
    HintEntry::new("mm", "bookmarks", "open bookmark manage"),
];

const CAT_OPERATORS: &[HintEntry] = &[
    HintEntry::new("c", "chip", "filter chip from current row field"),
    HintEntry::new("C", "exclude", "exclude chip from current row field"),
    HintEntry::new("h/l", "strip", "prev / next group on focused strip"),
    HintEntry::new("dd", "delete", "delete selected strip group"),
    HintEntry::new("di", "disable", "toggle disable selected strip group"),
];

const CAT_SESSION: &[HintEntry] = &[
    HintEntry::new("f p/t/u", "lock", "lock pid / tid / clear"),
    HintEntry::new(
        "t s/u",
        "time",
        "set / clear global time window (file only)",
    ),
    HintEntry::new("ma/md", "bookmark", "add / remove bookmark on current row"),
    HintEntry::new("y c", "export", "yank current filters as aloggrep CLI"),
    HintEntry::new(
        "y …",
        "yank field",
        "yank tag/msg/pkg/pid/tid/level/raw/line/time",
    ),
    HintEntry::new("^L", "clear", "clear buffered logs (--hdc)"),
];

const CAT_OVERLAYS: &[HintEntry] = &[
    HintEntry::new("p/P", "detail", "toggle fields / pretty overlay"),
    HintEntry::new("V", "visual", "visual line mode"),
    HintEntry::new(
        "Picker",
        "fzf",
        "type to filter; Enter toggle; ^X edit; Del delete",
    ),
];

const CAT_HELP: &[HintEntry] = &[
    HintEntry::new("?", "help", "toggle this help panel"),
    HintEntry::new("j/k", "scroll", "scroll help content one line"),
    HintEntry::new("J/K", "jump", "scroll help content 7 lines"),
    HintEntry::new("Esc", "close", "close help without resuming follow"),
];

const CATALOG: &[HintSection] = &[
    HintSection {
        id: SectionId::Navigation,
        title: "Navigation",
        entries: CAT_NAVIGATION,
    },
    HintSection {
        id: SectionId::LeaderPickers,
        title: "Leader & pickers",
        entries: CAT_LEADER,
    },
    HintSection {
        id: SectionId::Operators,
        title: "Filter operators",
        entries: CAT_OPERATORS,
    },
    HintSection {
        id: SectionId::Session,
        title: "Session",
        entries: CAT_SESSION,
    },
    HintSection {
        id: SectionId::Overlays,
        title: "Overlays",
        entries: CAT_OVERLAYS,
    },
    HintSection {
        id: SectionId::Help,
        title: "Help",
        entries: CAT_HELP,
    },
];

/// Resolve the active hint context (modal > pending > focus).
pub fn context_kind(app: &App) -> ContextKind {
    if app
        .picker
        .as_ref()
        .is_some_and(|session| session.confirm.is_some())
    {
        return ContextKind::Confirm;
    }
    if app.picker.is_some() {
        return ContextKind::Picker;
    }
    if app.highlight_box.editing {
        return ContextKind::HighlightModal;
    }
    if app.time_panel.is_some() {
        return ContextKind::TimePanel;
    }
    if app.detail_open() {
        return ContextKind::Detail;
    }
    if app.pending_leader {
        return ContextKind::Leader;
    }
    if app.pending_m {
        return ContextKind::Bookmark;
    }
    if app.pending_lock {
        return ContextKind::Lock;
    }
    if app.pending_time {
        return ContextKind::Time;
    }
    if app.pending_chip || app.pending_exclude {
        return ContextKind::ChipField;
    }
    if app.pending_yank {
        return ContextKind::Yank;
    }
    if app.pending_d {
        return ContextKind::StripD;
    }
    match app.focus {
        Focus::Input => ContextKind::Input,
        Focus::ChipStrip => ContextKind::ChipStrip,
        Focus::ExcludeStrip => ContextKind::ExcludeStrip,
        Focus::HighlightStrip => ContextKind::HighlightStrip,
        Focus::LogList => {
            if matches!(app.export_source, crate::export::ExportSource::Hdc { .. }) {
                ContextKind::LogListHdc
            } else {
                ContextKind::LogList
            }
        }
    }
}

/// Entries for the current status-bar / Help Active context.
pub fn context_entries(app: &App) -> &'static [HintEntry] {
    match context_kind(app) {
        ContextKind::Confirm => L1_CONFIRM,
        ContextKind::Picker => L1_PICKER,
        ContextKind::HighlightModal => L1_HIGHLIGHT_MODAL,
        ContextKind::TimePanel => L1_TIME_PANEL,
        ContextKind::Detail => L1_DETAIL,
        ContextKind::Leader => L2_LEADER,
        ContextKind::Bookmark => L2_BOOKMARK,
        ContextKind::Lock => L2_LOCK,
        ContextKind::Time => L2_TIME,
        ContextKind::ChipField => L2_CHIP_FIELD,
        ContextKind::Yank => L2_YANK,
        ContextKind::StripD => L2_STRIP_D,
        ContextKind::Input => L1_INPUT,
        ContextKind::ChipStrip => L1_CHIP_STRIP,
        ContextKind::ExcludeStrip => L1_EXCLUDE_STRIP,
        ContextKind::HighlightStrip => L1_HIGHLIGHT_STRIP,
        ContextKind::LogList => L1_LOGLIST,
        ContextKind::LogListHdc => L1_LOGLIST_HDC,
    }
}

/// Map context → catalog section to emphasize.
pub fn active_section_id(kind: ContextKind) -> SectionId {
    match kind {
        ContextKind::Leader | ContextKind::Picker | ContextKind::Confirm => {
            SectionId::LeaderPickers
        }
        ContextKind::ChipField
        | ContextKind::StripD
        | ContextKind::ChipStrip
        | ContextKind::ExcludeStrip
        | ContextKind::HighlightStrip
        | ContextKind::Input
        | ContextKind::HighlightModal => SectionId::Operators,
        ContextKind::Bookmark | ContextKind::Lock | ContextKind::Time | ContextKind::Yank => {
            SectionId::Session
        }
        ContextKind::Detail | ContextKind::TimePanel => SectionId::Overlays,
        ContextKind::LogList | ContextKind::LogListHdc => SectionId::Navigation,
    }
}

/// Full catalog sections (hdc hides interactive time entry detail via filter in UI).
pub fn catalog_sections() -> &'static [HintSection] {
    CATALOG
}

/// Whether Help may open for the current app state.
pub fn help_available(app: &App) -> bool {
    if app.picker.is_some()
        || app.time_panel.is_some()
        || app.detail_open()
        || app.highlight_box.editing
    {
        return false;
    }
    if app.pending_leader
        || app.pending_m
        || app.pending_lock
        || app.pending_time
        || app.pending_chip
        || app.pending_exclude
        || app.pending_yank
        || app.pending_d
    {
        return false;
    }
    matches!(
        app.focus,
        Focus::LogList | Focus::ChipStrip | Focus::ExcludeStrip | Focus::HighlightStrip
    )
}

fn key_style() -> Style {
    theme::context_help_style()
}

fn label_style() -> Style {
    Style::default()
}

fn entry_width(entry: &HintEntry) -> usize {
    entry.key.chars().count() + 1 + entry.label.chars().count()
}

/// Fit situational hints into `max_chars` as styled spans (dim key + label).
pub fn context_hint_spans(app: &App, max_chars: usize) -> Option<Vec<Span<'static>>> {
    if max_chars < MIN_HELP_WIDTH {
        return None;
    }
    let entries = context_entries(app);
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let gap = if i == 0 { 0 } else { 2 };
        let need = gap + entry_width(entry);
        if used + need <= max_chars {
            if gap > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(entry.key.to_string(), key_style()));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(entry.label.to_string(), label_style()));
            used += need;
            continue;
        }
        // Partial last entry: keep key + truncated label when budget allows.
        let key_w = entry.key.chars().count();
        let remain = max_chars.saturating_sub(used + gap + key_w + 1);
        if remain >= 1 && used + gap + key_w + 1 < max_chars {
            if gap > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(entry.key.to_string(), key_style()));
            spans.push(Span::raw(" "));
            let trunc: String = entry.label.chars().take(remain).collect();
            spans.push(Span::styled(trunc, label_style()));
        }
        break;
    }
    if spans.is_empty() {
        None
    } else {
        Some(spans)
    }
}

/// Build scrollable Help body lines (Active + catalog).
pub fn help_body_lines(app: &App) -> Vec<Line<'static>> {
    let kind = context_kind(app);
    let active_id = active_section_id(kind);
    let hdc = matches!(kind, ContextKind::LogListHdc)
        || matches!(app.export_source, crate::export::ExportSource::Hdc { .. });

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            "Active  ",
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            kind.title().to_string(),
            Style::default().fg(theme::accent()),
        ),
    ]));

    for entry in context_entries(app) {
        lines.push(detail_line(entry));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "All commands",
        Style::default()
            .fg(theme::accent())
            .add_modifier(Modifier::BOLD),
    )));

    for section in catalog_sections() {
        let is_active = section.id == active_id;
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            section.title.to_string(),
            theme::help_section_style(is_active),
        )));
        for entry in section.entries {
            if hdc && entry.key.starts_with("t s") {
                continue;
            }
            if !hdc && entry.key == "^L" {
                continue;
            }
            lines.push(detail_line(entry));
        }
    }
    lines
}

fn detail_line(entry: &HintEntry) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<12}", entry.key), key_style()),
        Span::styled(entry.detail.to_string(), label_style()),
    ])
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
    fn context_loglist_hdc_appends_clear_hint() {
        let mut app = app_with_focus(Focus::LogList);
        assert_eq!(context_kind(&app), ContextKind::LogList);
        app.export_source = crate::export::ExportSource::Hdc { device: None };
        assert_eq!(context_kind(&app), ContextKind::LogListHdc);
        let entries = context_entries(&app);
        assert!(
            entries.iter().any(|e| e.key == "^L"),
            "hdc LogList hint must expose Ctrl-L clear"
        );
        assert!(
            !entries.iter().any(|e| e.key == "t"),
            "hdc LogList must not expose interactive time"
        );
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
        app.open_picker(crate::picker::PickerKind::MsgChip { exclude: false });
        assert_eq!(context_kind(&app), ContextKind::Picker);
    }

    #[test]
    fn context_confirm_overrides_picker() {
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
        assert!(text.contains("j/k move"), "dim-key spacing form: {text:?}");
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
    fn help_body_has_active_and_catalog() {
        let app = app_with_focus(Focus::LogList);
        let lines = help_body_lines(&app);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("Active"), "{text}");
        assert!(text.contains("All commands"), "{text}");
        assert!(text.contains("Navigation"), "{text}");
    }

    #[test]
    fn catalog_jk_details_match_fast_scroll_step() {
        let step = FAST_SCROLL_STEP.to_string();
        let nav_jk = CAT_NAVIGATION
            .iter()
            .find(|e| e.key == "J/K")
            .expect("nav J/K");
        let help_jk = CAT_HELP.iter().find(|e| e.key == "J/K").expect("help J/K");
        assert!(
            nav_jk.detail.contains(&step),
            "nav detail {:?} must mention FAST_SCROLL_STEP={step}",
            nav_jk.detail
        );
        assert!(
            help_jk.detail.contains(&step),
            "help detail {:?} must mention FAST_SCROLL_STEP={step}",
            help_jk.detail
        );
    }
}
