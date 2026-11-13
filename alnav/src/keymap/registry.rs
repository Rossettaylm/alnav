//! Action registry: contexts, action metadata, and the declarative action table.
//!
//! Every keyboard action is one row of the `actions!` table below; the macro
//! generates [`ActionId`], [`ActionId::ALL`], [`ActionId::meta`], and
//! [`action_by_toml`] from it, so the lists cannot drift apart.

use super::stroke::Binding;
use crate::theme;

/// Action classification for chord trees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Arms a pending operator / leader; does not perform work alone.
    Prefix,
    /// Executes when its full binding is completed.
    Leaf,
}

/// Runtime capability filter (independent of whether a key is bound).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Only active for `-f` file sessions.
    FileOnly,
    /// Only active for `--hdc` / `--adb` live sessions.
    LiveOnly,
}

/// TOML section / runtime key context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyContext {
    Bookmark,
    ChipField,
    Confirm,
    Detail,
    Global,
    Help,
    HighlightModal,
    Leader,
    Lock,
    LogList,
    Open,
    Picker,
    Strip,
    StripD,
    Time,
    TimePanel,
    HistPanel,
    Visual,
    Yank,
    CommandPalette,
}

impl KeyContext {
    pub fn as_toml(self) -> &'static str {
        match self {
            Self::Bookmark => "bookmark",
            Self::ChipField => "chip_field",
            Self::Confirm => "confirm",
            Self::Detail => "detail",
            Self::Global => "global",
            Self::Help => "help",
            Self::HighlightModal => "highlight_modal",
            Self::Leader => "leader",
            Self::Lock => "lock",
            Self::LogList => "log_list",
            Self::Open => "open",
            Self::Picker => "picker",
            Self::Strip => "strip",
            Self::StripD => "strip_d",
            Self::Time => "time",
            Self::TimePanel => "time_panel",
            Self::HistPanel => "hist_panel",
            Self::Visual => "visual",
            Self::Yank => "yank",
            Self::CommandPalette => "command_palette",
        }
    }
    pub fn from_toml(s: &str) -> Option<Self> {
        match s {
            "bookmark" => Some(Self::Bookmark),
            "chip_field" => Some(Self::ChipField),
            "confirm" => Some(Self::Confirm),
            "detail" => Some(Self::Detail),
            "global" => Some(Self::Global),
            "help" => Some(Self::Help),
            "highlight_modal" => Some(Self::HighlightModal),
            "leader" => Some(Self::Leader),
            "lock" => Some(Self::Lock),
            "log_list" => Some(Self::LogList),
            "open" => Some(Self::Open),
            "picker" => Some(Self::Picker),
            "strip" => Some(Self::Strip),
            "strip_d" => Some(Self::StripD),
            "time" => Some(Self::Time),
            "time_panel" => Some(Self::TimePanel),
            "hist_panel" => Some(Self::HistPanel),
            "visual" => Some(Self::Visual),
            "yank" => Some(Self::Yank),
            "command_palette" => Some(Self::CommandPalette),
            _ => None,
        }
    }
}

/// Metadata for one registered action (defaults live here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMeta {
    pub id: ActionId,
    pub context: KeyContext,
    pub toml_key: &'static str,
    pub default: Binding,
    pub kind: ActionKind,
    pub capabilities: &'static [Capability],
    pub label: &'static str,
    pub detail: &'static str,
    /// Intent-command catalog (command palette). Unused when `in_palette` is false.
    pub in_palette: bool,
    pub palette_title: &'static str,
    pub palette_icon: &'static str,
}

impl ActionMeta {
    pub fn allowed(&self, file_mode: bool) -> bool {
        for c in self.capabilities {
            match c {
                Capability::FileOnly if !file_mode => return false,
                Capability::LiveOnly if file_mode => return false,
                _ => {}
            }
        }
        true
    }

    pub fn with_palette(mut self, title: &'static str, icon: &'static str) -> Self {
        self.in_palette = true;
        self.palette_title = title;
        self.palette_icon = icon;
        self
    }
}

impl ActionId {
    pub fn context(self) -> KeyContext {
        self.meta().context
    }
    pub fn toml_key(self) -> &'static str {
        self.meta().toml_key
    }
}

/// Default binding cell of an `actions!` row: `unbound` or a single stroke string.
macro_rules! default_binding {
    (unbound) => {
        Binding::unbound()
    };
    ($stroke:literal) => {
        Binding::parse_str($stroke).expect("default binding")
    };
}

/// Declarative action table. Row shape:
///
/// `Id: Context, "toml_key", default, Kind, [Capability, ..], "label", "detail"
/// [, palette("Title", GLYPH_CONST)];`
///
/// `default` is `unbound` or a stroke string; `GLYPH_CONST` names a `theme` constant.
macro_rules! actions {
    ($(
        $id:ident: $ctx:ident, $key:literal, $default:tt, $kind:ident, [$($cap:ident),*],
        $label:literal, $detail:literal $(, palette($ptitle:literal, $picon:ident))?;
    )*) => {
        /// Stable action identity (every keyboard action registers here).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ActionId {
            $($id,)*
        }

        impl ActionId {
            pub const ALL: &'static [ActionId] = &[$(Self::$id,)*];

            pub fn meta(self) -> ActionMeta {
                match self {
                    $(Self::$id => ActionMeta {
                        id: Self::$id,
                        context: KeyContext::$ctx,
                        toml_key: $key,
                        default: default_binding!($default),
                        kind: ActionKind::$kind,
                        capabilities: &[$(Capability::$cap),*],
                        label: $label,
                        detail: $detail,
                        in_palette: false,
                        palette_title: "",
                        palette_icon: "",
                    }
                    $(.with_palette($ptitle, theme::$picon))?,)*
                }
            }
        }

        pub(super) fn action_by_toml(ctx: KeyContext, key: &str) -> Option<ActionId> {
            match (ctx, key) {
                $((KeyContext::$ctx, $key) => Some(ActionId::$id),)*
                _ => None,
            }
        }
    };
}

#[rustfmt::skip]
actions! {
    GlobalQuit: Global, "quit", "q", Leaf, [], "quit", "quit the application",
        palette("Quit", GLYPH_QUIT);
    GlobalFocusNext: Global, "focus_next", "Tab", Leaf, [], "focus", "next focus region";
    GlobalFocusPrev: Global, "focus_prev", "BackTab", Leaf, [], "focus", "previous focus region";
    GlobalFocusFilter: Global, "focus_filter", "1", Leaf, [], "filter", "focus filter strip";
    GlobalFocusExclude: Global, "focus_exclude", "2", Leaf, [], "exclude", "focus exclude strip";
    GlobalFocusHighlight: Global, "focus_highlight", "3", Leaf, [],
        "highlight", "focus highlight strip";
    GlobalFocusLog: Global, "focus_log", "4", Leaf, [], "log", "focus log list";
    GlobalFocusInput: Global, "focus_input", "5", Leaf, [], "input", "open unified manage";
    GlobalFilterNew: Global, "filter_new", ";", Leaf, [],
        "filter", "open filter picker in new mode", palette("Add Filter", GLYPH_TITLE_FILTER);
    GlobalHighlightNew: Global, "highlight_new", "/", Leaf, [],
        "highlight", "find or create a highlight and jump to the first hit",
        palette("Find Highlight", GLYPH_TITLE_HIGHLIGHT);
    GlobalHighlightAdd: Global, "highlight_add", unbound, Leaf, [],
        "add highlight", "open highlight picker in new mode",
        palette("Add Highlight", GLYPH_TITLE_HIGHLIGHT);
    GlobalExcludeNew: Global, "exclude_new", "`", Leaf, [],
        "exclude", "open exclude picker in new mode", palette("Add Exclude", GLYPH_TITLE_EXCLUDE);
    GlobalOpenHelp: Global, "open_help", "?", Leaf, [], "help", "open help panel",
        palette("Open Help", GLYPH_HELP);
    GlobalCommandPalette: Global, "command_palette", "C-p", Leaf, [],
        "palette", "open command palette";
    LogListMoveDown: LogList, "move_down", "j", Leaf, [], "move", "move cursor down";
    LogListMoveUp: LogList, "move_up", "k", Leaf, [], "move", "move cursor up";
    LogListJumpDown: LogList, "jump_down", "S-j", Leaf, [], "jump", "move down fast";
    LogListJumpUp: LogList, "jump_up", "S-k", Leaf, [], "jump", "move up fast";
    LogListJumpTop: LogList, "jump_top", "g", Leaf, [], "top", "jump to top";
    LogListJumpBottom: LogList, "jump_bottom", "S-g", Leaf, [],
        "bottom", "jump to bottom and resume follow";
    LogListResumeFollow: LogList, "resume_follow", "Esc", Leaf, [],
        "follow", "resume following and pin to bottom",
        palette("Resume Following", GLYPH_FOLLOWING);
    LogListNextMatch: LogList, "next_match", "n", Leaf, [], "next", "next highlight match";
    LogListPrevMatch: LogList, "prev_match", "S-n", Leaf, [], "prev", "previous highlight match";
    LogListNextSevere: LogList, "next_severe", "e", Leaf, [], "error", "next severe line";
    LogListPrevSevere: LogList, "prev_severe", "S-e", Leaf, [], "error", "previous severe line";
    LogListDetailFields: LogList, "detail_fields", "p", Leaf, [], "detail", "toggle fields overlay",
        palette("Show Fields", GLYPH_VIEW_FOCUS);
    LogListDetailPretty: LogList, "detail_pretty", "S-p", Leaf, [],
        "pretty", "toggle pretty / swap overlay (crash rows show structured detail)",
        palette("Show Pretty", GLYPH_VIEW_FOCUS);
    LogListVisualLine: LogList, "visual_line", "S-v", Leaf, [], "visual", "enter visual line mode";
    LogListYankMsgLine: LogList, "yank_msg_line", "S-y", Leaf, [],
        "yank", "yank message of current line", palette("Yank Message", GLYPH_FIELD_MSG);
    LogListClearLive: LogList, "clear_live", "C-l", Leaf, [LiveOnly],
        "clear", "clear buffered live logs", palette("Clear Live Buffer", GLYPH_DISCONNECT);
    LogListPageDown: LogList, "page_down", "C-d", Leaf, [], "page", "page down";
    LogListPageUp: LogList, "page_up", "C-u", Leaf, [], "page", "page up";
    LogListLeader: LogList, "leader", "Space", Prefix, [], "menu", "leader prefix";
    LogListBookmark: LogList, "bookmark", "m", Prefix, [], "mark", "bookmark operator";
    LogListChip: LogList, "chip", "c", Prefix, [], "chip", "filter chip from row";
    LogListExcludeChip: LogList, "exclude_chip", "S-c", Prefix, [],
        "exclude", "exclude chip from row";
    LogListYank: LogList, "yank", "y", Prefix, [], "yank", "yank operator";
    LogListLock: LogList, "lock", "f", Prefix, [], "focus", "lock / view focus operator";
    LogListTime: LogList, "time", "t", Prefix, [FileOnly], "time", "time window operator";
    LogListWrapToggle: LogList, "wrap_toggle", "w", Leaf, [],
        "wrap", "toggle multi-line / single-line collapsed view",
        palette("Toggle Wrap", GLYPH_TITLE_LOG);
    LeaderManage: Leader, "manage", "Space", Leaf, [], "manage", "open unified manage panel",
        palette("Manage Rules", GLYPH_MODE_MANAGE);
    ClearAllRules: Picker, "clear_all_rules", "C-k", Leaf, [],
        "clear", "clear all filter / highlight / exclude rules",
        palette("Clear All Rules", GLYPH_TITLE_EXCLUDE);
    ClearSession: Global, "clear_session", unbound, Leaf, [FileOnly],
        "clear session", "delete the saved session for the current file",
        palette("Clear Session", GLYPH_TITLE_DASHBOARD);
    LeaderPresetSave: Global, "preset_save", "C-s", Leaf, [], "save", "save filter preset",
        palette("Save Preset", GLYPH_MODE_NEW);
    LeaderPresetOpen: Global, "preset_open", "C-o", Leaf, [], "open", "open filter preset",
        palette("Open Preset", GLYPH_SOURCE_DIR);
    LeaderSummary: Leader, "summary", "i", Leaf, [], "stats", "open summary panel",
        palette("Show Summary", GLYPH_TITLE_DASHBOARD);
    LeaderCancel: Leader, "cancel", "Esc", Leaf, [], "cancel", "cancel leader";
    BookmarkAdd: Bookmark, "add", "a", Leaf, [], "add", "bookmark current row",
        palette("Add Bookmark", GLYPH_BOOKMARK);
    BookmarkRemove: Bookmark, "remove", "d", Leaf, [], "delete", "remove bookmark on current row",
        palette("Remove Bookmark", GLYPH_BOOKMARK);
    BookmarkManage: Bookmark, "manage", "m", Leaf, [], "compare", "open bookmark compare panel",
        palette("Open Compare Panel", GLYPH_BOOKMARK);
    BookmarkCancel: Bookmark, "cancel", "Esc", Leaf, [], "cancel", "cancel bookmark operator";
    LockPid: Lock, "pid", "p", Leaf, [], "pid", "lock to pid", palette("Lock PID", GLYPH_LOCK);
    LockTid: Lock, "tid", "t", Leaf, [], "tid", "lock to tid", palette("Lock TID", GLYPH_LOCK);
    LockViewHighlight: Lock, "view_highlight", "h", Leaf, [], "hl", "toggle highlight-only view",
        palette("View Focus Highlight", GLYPH_VIEW_FOCUS);
    LockViewSevere: Lock, "view_severe", "e", Leaf, [], "err", "toggle severe-only view",
        palette("View Focus Severe", GLYPH_CRASH);
    LockClear: Lock, "clear", "u", Leaf, [], "clear", "clear session lock",
        palette("Clear Lock", GLYPH_LOCK);
    LockCancel: Lock, "cancel", "Esc", Leaf, [], "cancel", "cancel lock operator";
    OpenFile: Global, "open_file", "C-f", Leaf, [], "file", "open or switch to a file",
        palette("Open File", GLYPH_SOURCE_OPEN_FILE);
    OpenStream: Global, "open_stream", "C-g", Leaf, [], "stream", "open or switch to hdc/adb",
        palette("Open Stream", GLYPH_SOURCE_HDC);
    TimeSet: Time, "set", "t", Leaf, [FileOnly], "set", "open time window panel",
        palette("Set Time Window", GLYPH_TIME);
    TimeClear: Time, "clear", "u", Leaf, [FileOnly], "clear", "clear time window",
        palette("Clear Time Window", GLYPH_TIME);
    TimeCancel: Time, "cancel", "Esc", Leaf, [FileOnly], "cancel", "cancel time operator";
    TimeHistogram: Time, "histogram", "h", Leaf, [FileOnly], "hist", "open time histogram",
        palette("Time Histogram", GLYPH_TIME);
    HistPanelPrev: HistPanel, "prev", "k", Leaf, [FileOnly], "prev", "previous bucket";
    HistPanelNext: HistPanel, "next", "j", Leaf, [FileOnly], "next", "next bucket";
    HistPanelJumpDown: HistPanel, "jump_down", "S-j", Leaf, [FileOnly], "jump", "move down fast";
    HistPanelJumpUp: HistPanel, "jump_up", "S-k", Leaf, [FileOnly], "jump", "move up fast";
    HistPanelJumpTop: HistPanel, "jump_top", "g", Leaf, [FileOnly], "top", "jump to first bucket";
    HistPanelJumpBottom: HistPanel, "jump_bottom", "S-g", Leaf, [FileOnly],
        "bottom", "jump to last bucket";
    HistPanelZoomIn: HistPanel, "zoom_in", "BackTab", Leaf, [FileOnly],
        "finer", "cycle finer buckets (5m→1m→10s)";
    HistPanelZoomOut: HistPanel, "zoom_out", "Tab", Leaf, [FileOnly],
        "coarser", "cycle coarser buckets (10s→1m→5m)";
    HistPanelSubmit: HistPanel, "submit", "Enter", Leaf, [FileOnly], "jump", "jump to bucket";
    HistPanelApplyWindow: HistPanel, "apply_window", "s", Leaf, [FileOnly],
        "window", "set time window from bucket";
    HistPanelCancel: HistPanel, "cancel", "Esc", Leaf, [FileOnly], "close", "close histogram";
    ChipFieldTag: ChipField, "tag", "t", Leaf, [], "tag", "select tag field";
    ChipFieldMsg: ChipField, "msg", "m", Leaf, [], "msg", "select msg field";
    ChipFieldPkg: ChipField, "pkg", "g", Leaf, [], "pkg", "select pkg field";
    ChipFieldPid: ChipField, "pid", "p", Leaf, [], "pid", "select pid field";
    ChipFieldTid: ChipField, "tid", "S-t", Leaf, [], "tid", "select tid field";
    ChipFieldLevel: ChipField, "level", "l", Leaf, [], "level", "select level field";
    ChipFieldCancel: ChipField, "cancel", "Esc", Leaf, [], "cancel", "select cancel field";
    YankCli: Yank, "cli", "c", Leaf, [], "cli", "yank filters as alnav grep CLI",
        palette("Yank CLI", GLYPH_TITLE_LOG);
    YankTag: Yank, "tag", "t", Leaf, [], "tag", "yank tag";
    YankMsg: Yank, "msg", "m", Leaf, [], "msg", "yank msg tokens";
    YankPkg: Yank, "pkg", "g", Leaf, [], "pkg", "yank package";
    YankPid: Yank, "pid", "p", Leaf, [], "pid", "yank pid";
    YankTid: Yank, "tid", "S-t", Leaf, [], "tid", "yank tid";
    YankLevel: Yank, "level", "l", Leaf, [], "level", "yank level";
    YankRaw: Yank, "raw", "r", Leaf, [], "raw", "yank raw line";
    YankLine: Yank, "line", "y", Leaf, [], "line", "yank formatted line";
    YankTime: Yank, "time", "s", Leaf, [], "time", "yank timestamp";
    YankCancel: Yank, "cancel", "Esc", Leaf, [], "cancel", "cancel yank operator";
    StripDDelete: StripD, "delete", "d", Leaf, [], "delete", "delete selected strip group",
        palette("Delete Selected Group", GLYPH_TITLE_EXCLUDE);
    StripDDisable: StripD, "disable", "i", Leaf, [],
        "disable", "toggle disable selected strip group",
        palette("Toggle Selected Group", GLYPH_ACTION_TOGGLE_OFF);
    StripDCancel: StripD, "cancel", "Esc", Leaf, [], "cancel", "cancel strip delete";
    StripPendingD: Strip, "pending_d", "d", Prefix, [], "del…", "dd delete / di disable";
    StripPrevGroup: Strip, "prev_group", "h", Leaf, [], "group", "previous strip group";
    StripNextGroup: Strip, "next_group", "l", Leaf, [], "group", "next strip group";
    StripResumeFollow: Strip, "resume_follow", "Esc", Leaf, [], "follow", "resume following";
    StripOpenHelp: Strip, "open_help", "?", Leaf, [], "help", "open help";
    StripFocusNext: Strip, "focus_next", "Tab", Leaf, [], "focus", "cycle focus";
    VisualMoveDown: Visual, "move_down", "j", Leaf, [], "move", "extend selection down";
    VisualMoveUp: Visual, "move_up", "k", Leaf, [], "move", "extend selection up";
    VisualJumpDown: Visual, "jump_down", "S-j", Leaf, [], "jump", "extend selection down fast";
    VisualJumpUp: Visual, "jump_up", "S-k", Leaf, [], "jump", "extend selection up fast";
    VisualYankRaw: Visual, "yank_raw", "y", Leaf, [], "yank", "yank selection raw";
    VisualYankMsg: Visual, "yank_msg", "S-y", Leaf, [], "yank", "yank selection messages";
    VisualCancel: Visual, "cancel", "Esc", Leaf, [], "cancel", "leave visual mode";
    HelpClose: Help, "close", "Esc", Leaf, [], "close", "close help";
    HelpToggle: Help, "toggle", "?", Leaf, [], "close", "close help";
    HelpScrollDown: Help, "scroll_down", "j", Leaf, [], "scroll", "scroll help down";
    HelpScrollUp: Help, "scroll_up", "k", Leaf, [], "scroll", "scroll help up";
    HelpJumpDown: Help, "jump_down", "S-j", Leaf, [], "jump", "scroll help down fast";
    HelpJumpUp: Help, "jump_up", "S-k", Leaf, [], "jump", "scroll help up fast";
    HelpTop: Help, "top", "g", Leaf, [], "top", "scroll help to top";
    HelpBottom: Help, "bottom", "S-g", Leaf, [], "bottom", "scroll help to bottom";
    HelpBack: Help, "back", "h", Leaf, [], "back", "return to help home";
    HelpBackAlt: Help, "back_alt", "Backspace", Leaf, [], "back", "return to help home";
    HelpSearch: Help, "search", "/", Leaf, [], "search", "search help";
    HelpSearchNext: Help, "search_next", "n", Leaf, [], "next", "next help search hit";
    HelpSearchPrev: Help, "search_prev", "S-n", Leaf, [], "prev", "previous help search hit";
    HelpSubmit: Help, "submit", "Enter", Leaf, [], "open", "open help page or commit search";
    PickerSubmit: Picker, "submit", "Enter", Leaf, [], "toggle", "enable/disable or submit";
    PickerUp: Picker, "up", "Up", Leaf, [], "select", "previous candidate";
    PickerDown: Picker, "down", "Down", Leaf, [], "select", "next candidate";
    PickerMulti: Picker, "multi", "Tab", Leaf, [], "multi", "toggle multi-select";
    PickerEdit: Picker, "edit", "C-x", Leaf, [], "edit", "edit selected";
    PickerDelete: Picker, "delete", "Delete", Leaf, [], "delete", "delete with confirm";
    PickerDeleteAlt: Picker, "delete_alt", "C-Backspace", Leaf, [], "delete", "delete with confirm";
    PickerClose: Picker, "close", "Esc", Leaf, [], "close", "close picker";
    ConfirmYes: Confirm, "yes", "y", Leaf, [], "confirm", "confirm";
    ConfirmYesEnter: Confirm, "yes_enter", "Enter", Leaf, [], "confirm", "confirm";
    ConfirmNo: Confirm, "no", "n", Leaf, [], "cancel", "cancel";
    ConfirmCancel: Confirm, "cancel", "Esc", Leaf, [], "cancel", "cancel";
    DetailCloseFields: Detail, "close_fields", "p", Leaf, [], "close", "close detail";
    DetailSwap: Detail, "swap", "S-p", Leaf, [], "swap", "swap fields/pretty";
    DetailChip: Detail, "chip", "c", Prefix, [], "chip", "filter field from detail";
    DetailExclude: Detail, "exclude", "S-c", Prefix, [], "exclude", "exclude field from detail";
    DetailMoveDown: Detail, "move_down", "j", Leaf, [], "row", "next row";
    DetailMoveUp: Detail, "move_up", "k", Leaf, [], "row", "previous row";
    DetailClose: Detail, "close", "Esc", Leaf, [], "close", "close detail";
    TimePanelNext: TimePanel, "next", "Tab", Leaf, [], "next", "next field";
    TimePanelSubmit: TimePanel, "submit", "Enter", Leaf, [], "next", "next field / submit";
    TimePanelDateUp: TimePanel, "date_up", "Up", Leaf, [], "date", "previous date";
    TimePanelDateDown: TimePanel, "date_down", "Down", Leaf, [], "date", "next date";
    TimePanelCancel: TimePanel, "cancel", "Esc", Leaf, [], "cancel", "cancel time panel";
    HighlightModalDraftSpace: HighlightModal, "draft_space", "Space", Leaf, [],
        "draft", "space in draft";
    HighlightModalConfirm: HighlightModal, "confirm", "Enter", Leaf, [], "ok", "confirm pattern";
    HighlightModalConfirmTab: HighlightModal, "confirm_tab", "Tab", Leaf, [],
        "ok", "confirm pattern";
    HighlightModalCancel: HighlightModal, "cancel", "Esc", Leaf, [], "cancel", "cancel";
    PaletteSubmit: CommandPalette, "submit", "Enter", Leaf, [], "run", "run selected command";
    PaletteUp: CommandPalette, "up", "Up", Leaf, [], "up", "select previous command";
    PaletteDown: CommandPalette, "down", "Down", Leaf, [], "down", "select next command";
    PaletteClose: CommandPalette, "close", "Esc", Leaf, [], "close", "close command palette";
}
