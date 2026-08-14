//! Focus-aware keybinding hints for the status bar and Help panel.
//!
//! Two levels: L1 shows single keys / multi-key prefixes; L2 shows the
//! follow-up keys while an operator is pending. Help Active + catalog use
//! the full [`context_entries`] set. The status bar uses
//! [`status_hint_entries`] (idle LogList/Strip are curated 1–2 keys; pending
//! and modal surfaces keep the full set). Rendering is dim keys + normal
//! labels with spacing (no `:` / `|` separators).
//!
//! Key strings come from [`App::keymap`]; labels/details stay in this module.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::{App, Focus};
use crate::keymap::ActionId;
use crate::theme;

/// Minimum remaining character budget before we bother showing help.
pub const MIN_HELP_WIDTH: usize = 8;

/// Shared `J`/`K` step for LogList cursor movement and Help panel scroll.
pub const FAST_SCROLL_STEP: isize = 7;

/// One keybinding hint (status short label + optional longer Help detail).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintEntry {
    pub key: String,
    pub label: &'static str,
    pub detail: &'static str,
}

impl HintEntry {
    fn new(key: String, label: &'static str, detail: &'static str) -> Self {
        Self { key, label, detail }
    }

    fn short(key: String, label: &'static str) -> Self {
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
    LogListLive,
    CommandPalette,
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
            Self::LogListLive => "Log list (live)",
            Self::CommandPalette => "Command palette",
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

fn key_of(app: &App, id: ActionId) -> Option<String> {
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

fn push_single(
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

fn push_agg(
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

fn push_short(out: &mut Vec<HintEntry>, app: &App, id: ActionId, label: &'static str) {
    push_single(out, app, id, label, label);
}

fn push_literal(out: &mut Vec<HintEntry>, key: &str, label: &'static str, detail: &'static str) {
    out.push(HintEntry::new(key.to_string(), label, detail));
}

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
    if app.command_palette.is_some() {
        return ContextKind::CommandPalette;
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
        "open highlight new",
    );
    push_single(
        &mut out,
        app,
        ActionId::GlobalExcludeNew,
        "exclude",
        "open exclude new",
    );
    // mm = bookmark prefix + manage
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::LogListBookmark),
        key_of(app, ActionId::BookmarkManage),
    ) {
        out.push(HintEntry::new(
            format!("{a}{b}"),
            "marks",
            "bookmark manage",
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
            push_short(&mut out, app, ActionId::BookmarkManage, "manage");
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
            push_short(&mut out, app, ActionId::TimeClear, "clear");
            push_short(&mut out, app, ActionId::TimeCancel, "cancel");
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
        ContextKind::Input => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::InputDraftSpace,
                "draft",
                "space in draft",
            );
            push_single(
                &mut out,
                app,
                ActionId::InputCommit,
                "commit",
                "pill then submit group",
            );
            push_single(
                &mut out,
                app,
                ActionId::InputToggleExclude,
                "exclude",
                "toggle exclude draft",
            );
            push_short(&mut out, app, ActionId::InputCancel, "cancel");
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
        ContextKind::Bookmark
        | ContextKind::Lock
        | ContextKind::Time
        | ContextKind::Yank => SectionId::Session,
        ContextKind::Detail | ContextKind::TimePanel => SectionId::Overlays,
        ContextKind::LogList | ContextKind::LogListLive => SectionId::Navigation,
        ContextKind::CommandPalette => SectionId::LeaderPickers,
    }
}

fn catalog_entries(app: &App, live: bool) -> Vec<(SectionId, &'static str, Vec<HintEntry>)> {
    let mut nav = Vec::new();
    push_agg(
        &mut nav,
        app,
        &[ActionId::LogListMoveDown, ActionId::LogListMoveUp],
        "move",
        "move cursor one line",
    );
    push_agg(
        &mut nav,
        app,
        &[ActionId::LogListJumpDown, ActionId::LogListJumpUp],
        "jump",
        "move 7 lines",
    );
    push_agg(
        &mut nav,
        app,
        &[ActionId::LogListJumpTop, ActionId::LogListJumpBottom],
        "top/bottom",
        "jump top or bottom (G resumes follow)",
    );
    push_single(
        &mut nav,
        app,
        ActionId::LogListResumeFollow,
        "follow",
        "resume following and pin to bottom",
    );
    push_agg(
        &mut nav,
        app,
        &[ActionId::LogListNextMatch, ActionId::LogListPrevMatch],
        "next hit",
        "next / previous highlight match",
    );
    push_agg(
        &mut nav,
        app,
        &[ActionId::LogListNextSevere, ActionId::LogListPrevSevere],
        "error",
        "next / previous severe line",
    );
    push_literal(
        &mut nav,
        "1-5",
        "focus",
        "focus filter / exclude / highlight / log / input",
    );

    let mut leader = Vec::new();
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::LogListLeader),
        key_of(app, ActionId::LeaderManage),
    ) {
        leader.push(HintEntry::new(
            format!("{a} {b}"),
            "manage",
            "unified manage picker",
        ));
    }
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::LogListLeader),
        key_of(app, ActionId::LeaderSummary),
    ) {
        leader.push(HintEntry::new(
            format!("{a} {b}"),
            "stats",
            "open summary panel (level / tags / errors)",
        ));
    }
    push_single(
        &mut leader,
        app,
        ActionId::GlobalFilterNew,
        "filter new",
        "open filter picker in new mode",
    );
    push_single(
        &mut leader,
        app,
        ActionId::GlobalHighlightNew,
        "highlight new",
        "open highlight picker in new mode",
    );
    push_single(
        &mut leader,
        app,
        ActionId::GlobalExcludeNew,
        "exclude new",
        "open exclude picker in new mode",
    );
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::LogListBookmark),
        key_of(app, ActionId::BookmarkManage),
    ) {
        leader.push(HintEntry::new(
            format!("{a}{b}"),
            "bookmarks",
            "open bookmark manage",
        ));
    }

    let mut ops = Vec::new();
    push_single(
        &mut ops,
        app,
        ActionId::LogListChip,
        "chip",
        "filter/highlight from row (msg → tokens → Filter|Highlight)",
    );
    push_single(
        &mut ops,
        app,
        ActionId::LogListExcludeChip,
        "exclude",
        "exclude chip from current row field",
    );
    push_agg(
        &mut ops,
        app,
        &[ActionId::StripPrevGroup, ActionId::StripNextGroup],
        "strip",
        "prev / next group on focused strip",
    );
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::StripPendingD),
        key_of(app, ActionId::StripDDelete),
    ) {
        ops.push(HintEntry::new(
            format!("{a}{b}"),
            "delete",
            "delete selected strip group",
        ));
    }
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::StripPendingD),
        key_of(app, ActionId::StripDDisable),
    ) {
        ops.push(HintEntry::new(
            format!("{a}{b}"),
            "disable",
            "toggle disable selected strip group",
        ));
    }

    let mut session = Vec::new();
    push_single(
        &mut session,
        app,
        ActionId::LeaderPresetSave,
        "save preset",
        "save Filter/Exclude/Highlight preset",
    );
    push_single(
        &mut session,
        app,
        ActionId::LeaderPresetOpen,
        "open preset",
        "search and apply named preset",
    );
    push_agg(
        &mut session,
        app,
        &[ActionId::OpenFile, ActionId::OpenStream],
        "source",
        "open or switch file / stream source",
    );
    if let (Some(p), Some(fp), Some(ft), Some(fu)) = (
        key_of(app, ActionId::LogListLock),
        key_of(app, ActionId::LockPid),
        key_of(app, ActionId::LockTid),
        key_of(app, ActionId::LockClear),
    ) {
        session.push(HintEntry::new(
            format!("{p} {fp}/{ft}/{fu}"),
            "lock",
            "lock pid / tid / clear",
        ));
    }
    if let (Some(p), Some(h), Some(e)) = (
        key_of(app, ActionId::LogListLock),
        key_of(app, ActionId::LockViewHighlight),
        key_of(app, ActionId::LockViewSevere),
    ) {
        session.push(HintEntry::new(
            format!("{p} {h}/{e}"),
            "view",
            "highlight-only / severe-only (independent toggles; both = AND)",
        ));
    }
    if !live {
        if let (Some(t), Some(tt), Some(tu)) = (
            key_of(app, ActionId::LogListTime),
            key_of(app, ActionId::TimeSet),
            key_of(app, ActionId::TimeClear),
        ) {
            session.push(HintEntry::new(
                format!("{t} {tt}/{tu}"),
                "time",
                "set / clear global time window (file only)",
            ));
        }
    }
    if let (Some(m), Some(a), Some(d)) = (
        key_of(app, ActionId::LogListBookmark),
        key_of(app, ActionId::BookmarkAdd),
        key_of(app, ActionId::BookmarkRemove),
    ) {
        session.push(HintEntry::new(
            format!("{m}{a}/{m}{d}"),
            "bookmark",
            "add / remove bookmark on current row",
        ));
    }
    if let (Some(y), Some(c)) = (
        key_of(app, ActionId::LogListYank),
        key_of(app, ActionId::YankCli),
    ) {
        session.push(HintEntry::new(
            format!("{y} {c}"),
            "export",
            "yank filters as alnav grep CLI (literal approx)",
        ));
    }
    if let Some(y) = key_of(app, ActionId::LogListYank) {
        session.push(HintEntry::new(
            format!("{y} …"),
            "yank field",
            "yank tag/msg(token picker)/pkg/pid/tid/level/raw/line/time",
        ));
    }
    if live {
        push_single(
            &mut session,
            app,
            ActionId::LogListClearLive,
            "clear",
            "clear buffered live logs",
        );
    }

    let mut overlays = Vec::new();
    push_agg(
        &mut overlays,
        app,
        &[ActionId::LogListDetailFields, ActionId::LogListDetailPretty],
        "detail",
        "toggle fields / pretty overlay",
    );
    push_single(
        &mut overlays,
        app,
        ActionId::LogListWrapToggle,
        "wrap",
        "toggle multi-line wrap / single-line collapsed view",
    );
    push_single(
        &mut overlays,
        app,
        ActionId::LogListVisualLine,
        "visual",
        "visual line mode",
    );
    push_literal(
        &mut overlays,
        "Picker",
        "fuzzy",
        "type to fuzzy-filter; Enter toggle; ^X edit; Del delete",
    );

    let mut help = Vec::new();
    push_single(
        &mut help,
        app,
        ActionId::GlobalOpenHelp,
        "help",
        "toggle this help panel",
    );
    push_single(
        &mut help,
        app,
        ActionId::GlobalCommandPalette,
        "palette",
        "open command palette",
    );
    push_agg(
        &mut help,
        app,
        &[ActionId::HelpScrollDown, ActionId::HelpScrollUp],
        "scroll",
        "scroll help content one line",
    );
    push_agg(
        &mut help,
        app,
        &[ActionId::HelpJumpDown, ActionId::HelpJumpUp],
        "jump",
        "scroll help content 7 lines",
    );
    push_single(
        &mut help,
        app,
        ActionId::HelpClose,
        "close",
        "close help without resuming follow",
    );

    vec![
        (SectionId::Navigation, "Navigation", nav),
        (SectionId::LeaderPickers, "Leader & pickers", leader),
        (SectionId::Operators, "Filter operators", ops),
        (SectionId::Session, "Session", session),
        (SectionId::Overlays, "Overlays", overlays),
        (SectionId::Help, "Help", help),
    ]
}

/// Whether Help may open for the current app state.
pub fn help_available(app: &App) -> bool {
    if app.picker.is_some()
        || app.time_panel.is_some()
        || app.detail_open()
        || app.highlight_box.editing
        || app.command_palette.is_some()
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

/// Build scrollable Help body lines (Active + catalog).
pub fn help_body_lines(app: &App) -> Vec<Line<'static>> {
    let kind = context_kind(app);
    let active_id = active_section_id(kind);
    let live = matches!(kind, ContextKind::LogListLive) || app.export_source.is_live();

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
        lines.push(detail_line(&entry));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "All commands",
        Style::default()
            .fg(theme::accent())
            .add_modifier(Modifier::BOLD),
    )));

    for (id, title, entries) in catalog_entries(app, live) {
        let is_active = id == active_id;
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            title.to_string(),
            theme::help_section_style(is_active),
        )));
        for entry in entries {
            lines.push(detail_line(&entry));
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

    #[test]
    fn help_body_still_lists_move_cursor() {
        let app = app_with_focus(Focus::LogList);
        let lines = help_body_lines(&app);
        let text: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect::<Vec<_>>()
            .join("");
        assert!(text.contains("j/k") || text.contains("move"), "{text}");
        assert!(
            text.contains("move cursor") || text.contains("move"),
            "{text}"
        );
        let active = context_entries(&app);
        assert!(
            active.iter().any(|e| e.label == "move"),
            "Help Active must keep full LogList L1: {active:?}"
        );
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
    fn adb_help_hides_time_and_shows_clear() {
        let mut app = app_with_focus(Focus::LogList);
        app.export_source = crate::export::ExportSource::Adb { device: None };
        let lines = help_body_lines(&app);
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<Vec<_>>()
            .join("");

        assert!(!text.contains("set / clear global time window"), "{text}");
        assert!(text.contains("clear buffered live logs"), "{text}");
    }

    #[test]
    fn catalog_jk_details_match_fast_scroll_step() {
        let app = app_with_focus(Focus::LogList);
        let step = FAST_SCROLL_STEP.to_string();
        let catalog = catalog_entries(&app, false);
        let nav = &catalog
            .iter()
            .find(|(id, _, _)| *id == SectionId::Navigation)
            .expect("nav")
            .2;
        let help = &catalog
            .iter()
            .find(|(id, _, _)| *id == SectionId::Help)
            .expect("help")
            .2;
        let nav_jk = nav
            .iter()
            .find(|e| e.key.contains('/') && e.label == "jump")
            .expect("nav jump");
        let help_jk = help.iter().find(|e| e.label == "jump").expect("help jump");
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
        let body: String = help_body_lines(&app)
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            body.contains("open command palette") || body.contains("C-p"),
            "Help catalog must mention the palette binding: {body}"
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
}
