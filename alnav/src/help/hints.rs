//! Status-bar / Help Active hint entries: `HintEntry`, `ContextKind`, L1 sets, `context_hint_spans`.

use super::MIN_HELP_WIDTH;
use crate::app::Pending;
use crate::app::{App, Focus};
use crate::keymap::ActionId;
use crate::theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// One keybinding hint (status short label + optional longer Help detail).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintEntry {
    pub key: String,
    pub label: &'static str,
    pub detail: &'static str,
}

impl HintEntry {
    pub(super) fn new(key: String, label: &'static str, detail: &'static str) -> Self {
        Self { key, label, detail }
    }
}

/// Which situational hint set is active (drives L1/L2 + Help Active).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextKind {
    Confirm,
    Picker,
    HighlightModal,
    TimePanel,
    HistPanel,
    Detail,
    Leader,
    Bookmark,
    Lock,
    Time,
    ChipField,
    Yank,
    StripD,
    ChipStrip,
    ExcludeStrip,
    HighlightStrip,
    LogList,
    LogListLive,
    CommandPalette,
    Compare,
}

impl ContextKind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Confirm => "Confirm",
            Self::Picker => "Picker",
            Self::HighlightModal => "Highlight edit",
            Self::TimePanel => "Time window",
            Self::HistPanel => "Histogram",
            Self::Detail => "Detail",
            Self::Leader => "Leader",
            Self::Bookmark => "Bookmark",
            Self::Lock => "Lock",
            Self::Time => "Time",
            Self::ChipField => "Field",
            Self::Yank => "Yank",
            Self::StripD => "Strip delete",
            Self::ChipStrip => "Filter strip",
            Self::ExcludeStrip => "Exclude strip",
            Self::HighlightStrip => "Highlight strip",
            Self::LogList => "Log list",
            Self::LogListLive => "Log list (live)",
            Self::CommandPalette => "Command palette",
            Self::Compare => "Compare",
        }
    }
}

pub(super) fn key_of(app: &App, id: ActionId) -> Option<String> {
    let file_mode = app.is_file_mode();
    if !id.meta().allowed(file_mode) {
        return None;
    }
    app.keymap.display(id)
}

fn agg(app: &App, ids: &[ActionId]) -> Option<String> {
    let file_mode = app.is_file_mode();
    let mut parts = Vec::new();
    for &id in ids {
        if !id.meta().allowed(file_mode) {
            continue;
        }
        if let Some(s) = app.keymap.display(id) {
            parts.push(s);
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

pub(super) fn push_single(
    out: &mut Vec<HintEntry>,
    app: &App,
    id: ActionId,
    label: &'static str,
    detail: &'static str,
) {
    if let Some(key) = key_of(app, id) {
        out.push(HintEntry::new(key, label, detail));
    }
}

pub(super) fn push_agg(
    out: &mut Vec<HintEntry>,
    app: &App,
    ids: &[ActionId],
    label: &'static str,
    detail: &'static str,
) {
    if let Some(key) = agg(app, ids) {
        out.push(HintEntry::new(key, label, detail));
    }
}

pub(super) fn push_short(out: &mut Vec<HintEntry>, app: &App, id: ActionId, label: &'static str) {
    push_single(out, app, id, label, label);
}

pub(super) fn push_literal(
    out: &mut Vec<HintEntry>,
    key: &str,
    label: &'static str,
    detail: &'static str,
) {
    out.push(HintEntry::new(key.to_string(), label, detail));
}

/// Resolve the active hint context (modal > pending > focus).
pub fn context_kind(app: &App) -> ContextKind {
    if app.confirm.is_some() {
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
    if app.hist_open() {
        return ContextKind::HistPanel;
    }
    if app.compare.is_some() {
        return ContextKind::Compare;
    }
    if app.command_palette.is_some() {
        return ContextKind::CommandPalette;
    }
    if app.detail_open() {
        return ContextKind::Detail;
    }
    match app.pending {
        Pending::Leader => return ContextKind::Leader,
        Pending::Mark => return ContextKind::Bookmark,
        Pending::Lock => return ContextKind::Lock,
        Pending::Time => return ContextKind::Time,
        Pending::Chip | Pending::Exclude => return ContextKind::ChipField,
        Pending::Yank => return ContextKind::Yank,
        Pending::StripD => return ContextKind::StripD,
        Pending::None => {}
    }
    match app.focus {
        Focus::ChipStrip => ContextKind::ChipStrip,
        Focus::ExcludeStrip => ContextKind::ExcludeStrip,
        Focus::HighlightStrip => ContextKind::HighlightStrip,
        Focus::LogList => {
            if app.export_source.is_live() {
                ContextKind::LogListLive
            } else {
                ContextKind::LogList
            }
        }
    }
}

fn l1_loglist(app: &App, live: bool) -> Vec<HintEntry> {
    let mut out = Vec::new();
    push_agg(
        &mut out,
        app,
        &[ActionId::LogListMoveDown, ActionId::LogListMoveUp],
        "move",
        "move",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListResumeFollow,
        "follow",
        "resume following",
    );
    push_single(&mut out, app, ActionId::GlobalOpenHelp, "help", "open help");
    push_single(
        &mut out,
        app,
        ActionId::GlobalCommandPalette,
        "palette",
        "open command palette",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListLeader,
        "menu",
        "leader then Space for manage",
    );
    push_single(
        &mut out,
        app,
        ActionId::GlobalFilterNew,
        "filter",
        "open filter new",
    );
    push_single(
        &mut out,
        app,
        ActionId::GlobalHighlightNew,
        "highlight",
        "find or create a highlight",
    );
    push_single(
        &mut out,
        app,
        ActionId::GlobalExcludeNew,
        "exclude",
        "open exclude new",
    );
    // mm = bookmark prefix + compare
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::LogListBookmark),
        key_of(app, ActionId::BookmarkManage),
    ) {
        out.push(HintEntry::new(
            format!("{a}{b}"),
            "marks",
            "open compare panel",
        ));
    }
    push_agg(
        &mut out,
        app,
        &[ActionId::LogListNextMatch, ActionId::LogListPrevMatch],
        "next",
        "next",
    );
    push_agg(
        &mut out,
        app,
        &[ActionId::LogListNextSevere, ActionId::LogListPrevSevere],
        "error",
        "error",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListBookmark,
        "mark",
        "bookmark operator",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListLock,
        "focus",
        "lock pid/tid or view focus",
    );
    if !live {
        push_single(&mut out, app, ActionId::LogListTime, "time", "time window");
    }
    push_single(
        &mut out,
        app,
        ActionId::LogListChip,
        "chip",
        "filter from row",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListExcludeChip,
        "exclude",
        "exclude from row",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListYank,
        "yank",
        "yank operator",
    );
    push_agg(
        &mut out,
        app,
        &[ActionId::OpenFile, ActionId::OpenStream],
        "source",
        "open or switch file / stream source",
    );
    push_agg(
        &mut out,
        app,
        &[ActionId::LogListDetailFields, ActionId::LogListDetailPretty],
        "detail",
        "fields / pretty",
    );
    push_single(
        &mut out,
        app,
        ActionId::LogListWrapToggle,
        "wrap",
        "toggle single-line collapsed view",
    );
    if live {
        push_single(
            &mut out,
            app,
            ActionId::LogListClearLive,
            "clear",
            "clear buffered logs",
        );
    }
    out
}

fn l1_strip(app: &App) -> Vec<HintEntry> {
    let mut out = Vec::new();
    push_agg(
        &mut out,
        app,
        &[ActionId::StripPrevGroup, ActionId::StripNextGroup],
        "group",
        "group",
    );
    push_single(
        &mut out,
        app,
        ActionId::StripPendingD,
        "del…",
        "dd delete / di disable",
    );
    push_short(&mut out, app, ActionId::StripFocusNext, "focus");
    push_single(
        &mut out,
        app,
        ActionId::StripResumeFollow,
        "follow",
        "resume following",
    );
    push_single(&mut out, app, ActionId::StripOpenHelp, "help", "open help");
    out
}

/// Full L1/L2 for Help Active + catalog. Status bar uses [`status_hint_entries`].
pub fn context_entries(app: &App) -> Vec<HintEntry> {
    match context_kind(app) {
        ContextKind::Confirm => {
            let mut out = Vec::new();
            push_agg(
                &mut out,
                app,
                &[ActionId::ConfirmYes, ActionId::ConfirmYesEnter],
                "confirm",
                "confirm",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::ConfirmNo, ActionId::ConfirmCancel],
                "cancel",
                "cancel",
            );
            out
        }
        ContextKind::Picker => {
            let mut out = Vec::new();
            push_literal(&mut out, "type", "filter", "filter");
            push_agg(
                &mut out,
                app,
                &[ActionId::PickerUp, ActionId::PickerDown],
                "select",
                "select",
            );
            push_single(
                &mut out,
                app,
                ActionId::PickerMulti,
                "multi",
                "toggle multi-select",
            );
            push_single(
                &mut out,
                app,
                ActionId::PickerSubmit,
                "toggle",
                "enable/disable or submit",
            );
            push_single(&mut out, app, ActionId::PickerEdit, "edit", "edit selected");
            push_agg(
                &mut out,
                app,
                &[ActionId::PickerDelete, ActionId::PickerDeleteAlt],
                "delete",
                "delete with confirm",
            );
            push_single(
                &mut out,
                app,
                ActionId::ClearAllRules,
                "clear",
                "clear all rules",
            );
            push_short(&mut out, app, ActionId::PickerClose, "close");
            out
        }
        ContextKind::HighlightModal => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::HighlightModalDraftSpace,
                "draft",
                "space in draft",
            );
            push_agg(
                &mut out,
                app,
                &[
                    ActionId::HighlightModalConfirm,
                    ActionId::HighlightModalConfirmTab,
                ],
                "ok",
                "confirm pattern",
            );
            push_short(&mut out, app, ActionId::HighlightModalCancel, "cancel");
            out
        }
        ContextKind::TimePanel => {
            let mut out = Vec::new();
            push_agg(
                &mut out,
                app,
                &[ActionId::TimePanelNext, ActionId::TimePanelSubmit],
                "next",
                "next field",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::TimePanelDateUp, ActionId::TimePanelDateDown],
                "date",
                "date",
            );
            push_short(&mut out, app, ActionId::TimePanelCancel, "cancel");
            out
        }
        ContextKind::Detail => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::DetailCloseFields, "close");
            push_short(&mut out, app, ActionId::DetailSwap, "swap");
            push_agg(
                &mut out,
                app,
                &[ActionId::DetailChip, ActionId::DetailExclude],
                "chip",
                "filter / exclude field",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::DetailMoveDown, ActionId::DetailMoveUp],
                "row",
                "row",
            );
            push_short(&mut out, app, ActionId::DetailClose, "close");
            out
        }
        ContextKind::Leader => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::LeaderManage,
                "manage",
                "open manage panel",
            );
            push_single(
                &mut out,
                app,
                ActionId::LeaderSummary,
                "stats",
                "open summary panel",
            );
            push_short(&mut out, app, ActionId::LeaderCancel, "cancel");
            out
        }
        ContextKind::Bookmark => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::BookmarkAdd, "add");
            push_short(&mut out, app, ActionId::BookmarkRemove, "delete");
            push_short(&mut out, app, ActionId::BookmarkManage, "compare");
            push_short(&mut out, app, ActionId::BookmarkCancel, "cancel");
            out
        }
        ContextKind::Lock => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::LockPid, "pid");
            push_short(&mut out, app, ActionId::LockTid, "tid");
            push_short(&mut out, app, ActionId::LockViewHighlight, "hl");
            push_short(&mut out, app, ActionId::LockViewSevere, "err");
            push_short(&mut out, app, ActionId::LockClear, "clear");
            push_short(&mut out, app, ActionId::LockCancel, "cancel");
            out
        }
        ContextKind::Time => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::TimeSet, "set");
            push_short(&mut out, app, ActionId::TimeHistogram, "hist");
            push_short(&mut out, app, ActionId::TimeClear, "clear");
            push_short(&mut out, app, ActionId::TimeCancel, "cancel");
            out
        }
        ContextKind::HistPanel => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::HistPanelNext, "next");
            push_short(&mut out, app, ActionId::HistPanelPrev, "prev");
            push_agg(
                &mut out,
                app,
                &[ActionId::HistPanelJumpDown, ActionId::HistPanelJumpUp],
                "fast",
                "move 7 buckets",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::HistPanelJumpTop, ActionId::HistPanelJumpBottom],
                "ends",
                "first or last bucket",
            );
            push_short(&mut out, app, ActionId::HistPanelSubmit, "jump");
            push_short(&mut out, app, ActionId::HistPanelApplyWindow, "window");
            push_agg(
                &mut out,
                app,
                &[ActionId::HistPanelZoomOut, ActionId::HistPanelZoomIn],
                "interval",
                "cycle bucket width 10s/1m/5m",
            );
            push_short(&mut out, app, ActionId::HistPanelCancel, "close");
            out
        }
        ContextKind::ChipField => {
            let mut out = Vec::new();
            for (id, label) in [
                (ActionId::ChipFieldTag, "tag"),
                (ActionId::ChipFieldMsg, "msg"),
                (ActionId::ChipFieldPkg, "pkg"),
                (ActionId::ChipFieldPid, "pid"),
                (ActionId::ChipFieldTid, "tid"),
                (ActionId::ChipFieldLevel, "level"),
                (ActionId::ChipFieldCancel, "cancel"),
            ] {
                push_short(&mut out, app, id, label);
            }
            out
        }
        ContextKind::Yank => {
            let mut out = Vec::new();
            for (id, label) in [
                (ActionId::YankCli, "cli"),
                (ActionId::YankTag, "tag"),
                (ActionId::YankMsg, "msg"),
                (ActionId::YankPkg, "pkg"),
                (ActionId::YankPid, "pid"),
                (ActionId::YankTid, "tid"),
                (ActionId::YankLevel, "level"),
                (ActionId::YankRaw, "raw"),
                (ActionId::YankLine, "line"),
                (ActionId::YankTime, "time"),
                (ActionId::YankCancel, "cancel"),
            ] {
                push_short(&mut out, app, id, label);
            }
            out
        }
        ContextKind::StripD => {
            let mut out = Vec::new();
            push_short(&mut out, app, ActionId::StripDDelete, "delete");
            push_short(&mut out, app, ActionId::StripDDisable, "disable");
            push_short(&mut out, app, ActionId::StripDCancel, "cancel");
            out
        }
        ContextKind::ChipStrip | ContextKind::ExcludeStrip | ContextKind::HighlightStrip => {
            l1_strip(app)
        }
        ContextKind::LogList => l1_loglist(app, false),
        ContextKind::LogListLive => l1_loglist(app, true),
        ContextKind::CommandPalette => {
            let mut out = Vec::new();
            push_literal(&mut out, "type", "filter", "type to filter commands");
            push_agg(
                &mut out,
                app,
                &[ActionId::PaletteUp, ActionId::PaletteDown],
                "select",
                "select",
            );
            push_single(
                &mut out,
                app,
                ActionId::PaletteSubmit,
                "run",
                "run selected command",
            );
            push_short(&mut out, app, ActionId::PaletteClose, "close");
            out
        }
        ContextKind::Compare => {
            let mut out = Vec::new();
            push_literal(&mut out, "j/k", "pin", "move by pin");
            push_literal(&mut out, "g/G", "first", "first / last pin");
            push_literal(&mut out, "yy", "yank", "yank selected snapshot raw");
            push_literal(&mut out, "dd", "delete", "delete selected pin");
            push_literal(&mut out, "Enter", "jump", "jump to origin in log");
            push_literal(&mut out, "Esc", "close", "close compare panel");
            out
        }
    }
}

/// Status-bar hint subset: idle LogList/Strip are curated 1–2 keys;
/// pending/modal surfaces keep the full [`context_entries`] set.
pub fn status_hint_entries(app: &App) -> Vec<HintEntry> {
    match context_kind(app) {
        ContextKind::LogList | ContextKind::LogListLive => {
            let mut out = Vec::new();
            push_single(&mut out, app, ActionId::GlobalOpenHelp, "help", "open help");
            push_single(
                &mut out,
                app,
                ActionId::GlobalFilterNew,
                "filter",
                "open filter new",
            );
            out
        }
        ContextKind::ChipStrip | ContextKind::ExcludeStrip | ContextKind::HighlightStrip => {
            let mut out = Vec::new();
            push_single(&mut out, app, ActionId::StripOpenHelp, "help", "open help");
            push_single(
                &mut out,
                app,
                ActionId::StripPendingD,
                "del…",
                "dd delete / di disable",
            );
            out
        }
        _ => context_entries(app),
    }
}

pub(super) fn key_style() -> Style {
    theme::context_help_style()
}

pub(super) fn label_style() -> Style {
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
    let entries = status_hint_entries(app);
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (i, entry) in entries.iter().enumerate() {
        let gap = if i == 0 { 0 } else { 2 };
        let need = gap + entry_width(entry);
        if used + need <= max_chars {
            if gap > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(entry.key.clone(), key_style()));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(entry.label.to_string(), label_style()));
            used += need;
            continue;
        }
        let key_w = entry.key.chars().count();
        let remain = max_chars.saturating_sub(used + gap + key_w + 1);
        if remain >= 1 && used + gap + key_w + 1 < max_chars {
            if gap > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(entry.key.clone(), key_style()));
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

pub(super) fn detail_line(entry: &HintEntry) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{:<12}", entry.key), key_style()),
        Span::styled(entry.detail.to_string(), label_style()),
    ])
}
