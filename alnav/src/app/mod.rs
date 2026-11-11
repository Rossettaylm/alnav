//! Application state (`App`), shared UI enums, and `App::new`; behaviour lives in child modules by concern.

use crate::bookmark::{BookmarkList, ComparePanel};
use crate::filter_model::{GroupList, TimeBound};
use crate::help::{HelpSearch, HelpView};
use crate::highlight_model::{HighlightBox, HighlightGroupList};
use crate::hist_panel::{HistReport, HistView};
use crate::scan::{HighlightDomain, HighlightScanState};
use crate::store::RowStore;
use crate::time_panel::TimePanel;
use crate::vocab::Vocab;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

mod bg_job;
mod bookmarks;
mod cursor;
mod help_panel;
mod intake;
mod operators;
mod overlays;
mod presets;
mod rules;
mod session_state;
mod summary_hist;
mod surface;
mod visible;
mod vocab_preview;

pub use surface::{Dialog, Surface};
pub use visible::Visible;

/// Hard cap on the matched-rows buffer (OOM safety). When a filter is active,
/// matching rows are retained in `App::matched` independently of `rows`'
/// rolling eviction; only this cap reclaims them.
const MATCHED_HARD_CAP: usize = 1_000_000;

/// Soft per-frame drain budget: protect the UI when the ingest ring was filled
/// by a burst. Remaining rows stay in the ring for subsequent frames.
const DRAIN_BUDGET_PER_FRAME: usize = 4096;

/// Global session view focus (`fh` / `fe`): AND-narrows `visible` after
/// chip/exclude/lock/time. Bits are independent; both on = intersection.
/// Not a Filter strip group; not exported by `yc`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ViewFocus {
    /// Keep rows matching any **enabled** highlight group (tag/msg OR).
    pub highlight: bool,
    /// Keep severe rows (E/F/crash), same predicate as `e`/`E`.
    pub severe: bool,
}

impl ViewFocus {
    pub fn is_active(self) -> bool {
        self.highlight || self.severe
    }
}

/// Which view-focus bit `fh` / `fe` toggles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewFocusKind {
    Highlight,
    Severe,
}

/// Outcome of `n`/`N` / `e`/`E` jumps (no wrapscan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindJumpResult {
    /// Cursor moved to a further hit in the requested direction.
    Moved,
    /// At least one hit exists, but none further in that direction.
    NoMore,
    /// No hits at all (or jump unavailable — e.g. no active highlight).
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    ChipStrip,
    ExcludeStrip,
    HighlightStrip,
    LogList,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

/// Operator / chord prefix armed by its first key; the next key completes or
/// cancels it. A single slot makes "at most one pending chord" structural.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pending {
    #[default]
    None,
    /// `Space` leader.
    Leader,
    /// `y` yank field.
    Yank,
    /// `c` chip-from-cursor (H7).
    Chip,
    /// `C` exclude-from-cursor (H9).
    Exclude,
    /// `f` session lock / view focus (H8).
    Lock,
    /// `t` global time window (file mode).
    Time,
    /// `m` bookmark.
    Mark,
    /// First `d` on a chip strip (`dd` delete / `di` disable).
    StripD,
}

impl Pending {
    pub fn is_armed(self) -> bool {
        self != Pending::None
    }

    /// Disarm only when `kind` is the armed chord.
    pub fn cancel(&mut self, kind: Pending) {
        if *self == kind {
            *self = Pending::None;
        }
    }

    /// Disarm when the armed chord is any of `kinds`.
    pub fn cancel_any(&mut self, kinds: &[Pending]) {
        if kinds.contains(self) {
            *self = Pending::None;
        }
    }
}

/// Which chip strip the shared `h`/`l`/`dd`/`di` ops target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripKind {
    Filter,
    Exclude,
    Highlight,
}

/// Second-key target for the `y` operator (`yy`/`yt`/`ym`/…).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YankField {
    Raw,
    Tag,
    Msg,
    Pid,
    Tid,
    Level,
    Pkg,
    Timestamp,
}

impl YankField {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'y' | 'r' => Some(Self::Raw),
            't' => Some(Self::Tag),
            'm' => Some(Self::Msg),
            'p' => Some(Self::Pid),
            'T' => Some(Self::Tid),
            'l' => Some(Self::Level),
            'g' => Some(Self::Pkg),
            's' => Some(Self::Timestamp),
            _ => None,
        }
    }
}

/// Result of mapping a second key after operator `c` (H7 field alphabet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipFieldKey {
    Field(crate::input::ChipField),
    /// `r`/`y` (raw) or `s` (timestamp) — not valid for filter chips.
    Unsupported,
    Unknown,
}

impl ChipFieldKey {
    /// Same letters as [`YankField::from_char`], minus raw/timestamp.
    pub fn from_char(c: char) -> Self {
        use crate::input::ChipField;
        match c {
            't' => Self::Field(ChipField::Tag),
            'm' => Self::Field(ChipField::Msg),
            'g' => Self::Field(ChipField::Pkg),
            'p' => Self::Field(ChipField::Pid),
            'T' => Self::Field(ChipField::Tid),
            'l' => Self::Field(ChipField::Level),
            'r' | 'y' | 's' => Self::Unsupported,
            _ => Self::Unknown,
        }
    }
}

/// Second key for operator `f` (H8 session lock).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockKind {
    Pid,
    Tid,
}

/// H4/H5 shared row-detail overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailView {
    #[default]
    Closed,
    Fields,
    Pretty,
}

/// H? Leader `i` summary panel: static snapshot over `visible`, computed on a
/// background thread (File: mmap+`LineSpan`; Stream: `Vec<EntryRow>` clone).
/// Never auto-refreshes; close + reopen to get a new snapshot.
pub enum SummaryView {
    Closed,
    Loading,
    Ready(alnav::summary::SummaryOutput),
}

type SummaryReport = alnav::summary::SummaryOutput;

pub struct App {
    /// Stream (live/tests) or mmap file (`-f`) row backend.
    pub store: RowStore,
    /// Hard cap on stream `matched` (OOM safety). Defaults to
    /// [`MATCHED_HARD_CAP`]; tests override. Ignored for File.
    pub matched_cap: usize,
    pub visible: Visible,
    /// Active filter-scan generation for FileStore (stale batches ignored).
    file_filter_gen: u64,
    pub groups: GroupList,
    pub highlight_groups: HighlightGroupList,
    pub highlight_box: HighlightBox,
    /// Globally active search group for `n`/`N`, match stats, and underline.
    /// Independent of [`Self::highlight_cursor`] (HighlightStrip keyboard focus).
    pub active_highlight: Option<usize>,
    pub cursor: usize,
    pub max_lines: usize,
    pub should_quit: bool,
    pub focus: Focus,
    pub mode: Mode,
    pub group_cursor: usize,
    pub exclude_cursor: usize,
    pub highlight_cursor: usize,
    /// Armed operator / chord prefix awaiting its second key (`d`, `y`, `c`, `C`, `f`, `t`, `m`, leader).
    pub pending: Pending,
    /// One-shot startup Dashboard (unbound source). `None` after first bind.
    pub dashboard: Option<crate::dashboard::DashboardState>,
    /// Persisted recent files (config dir).
    pub recent: crate::recent::RecentFiles,
    /// Configured log_dirs corpus for Open-file fuzzy search.
    pub log_corpus: crate::log_corpus::LogCorpus,
    /// Open-file source panel (`C-f` / Dashboard Open file…).
    pub open_file_panel: Option<crate::source_panel::OpenFilePanel>,
    /// Centered HDC/ADB panel (`C-g`).
    pub stream_source_panel: Option<crate::source_panel::StreamSourcePanel>,
    /// Open fzf-style picker session (Unified Manage / Filter / Highlight / Exclude).
    pub picker: Option<crate::picker::PickerSession>,
    /// Screen-centered destructive confirm (`DeleteMany` / `DeletePreset` / `ClearAll`).
    pub confirm: Option<crate::picker::ConfirmKind>,
    /// VS Code-style command palette (`C-p`). Independent of [`Self::picker`].
    pub command_palette: Option<crate::command_palette::CommandPalette>,
    /// Bookmark compare tray modal (`mm`). Not a [`crate::picker::PickerSession`].
    pub compare: Option<ComparePanel>,
    /// Session bookmarks (M2).
    pub bookmarks: BookmarkList,
    /// O(1) cache of bookmarked `row_id`s for LogList bg lookup (F1).
    pub bookmark_row_ids: HashSet<u64>,
    /// Next ingest `row_id` (M2).
    pub next_row_id: u64,
    /// Session lock: at most one of pid/tid is set (H8; AND after chip filter).
    pub lock_pid: Option<String>,
    pub lock_tid: Option<String>,
    /// Global session time window (AND after lock). Not stored on Filter groups.
    pub time_bound: Option<TimeBound>,
    /// Session view focus (`fh`/`fe`); AND after time. Esc resume does not clear.
    pub view_focus: ViewFocus,
    /// Open `tt` time-window editor (`None` when closed).
    pub time_panel: Option<TimePanel>,
    /// When `Some`, LogList is in visual-line mode; value is the anchor
    /// index into `visible` (same coordinate space as `cursor`).
    pub visual_anchor: Option<usize>,
    pub following: bool,
    pub list_offset: usize,
    /// Session-level LogList display density (`w`); `false` = multi-line
    /// wrap (default), `true` = single-line collapsed with `…` truncation.
    /// Never persisted to `config.toml`; resets to `false` on new sessions.
    pub collapsed_view: bool,
    /// Transient flash toast (`YANKED`, `NO ERROR`, errors); auto-clears after 3s.
    pub status_msg: Option<String>,
    /// When `status_msg` flash should disappear (`None` = not a timed flash).
    pub status_flash_until: Option<Instant>,
    /// Last text prepared for the clipboard (set even if clipboard I/O fails).
    pub last_yanked: Option<String>,
    /// H4 field detail overlay (same shell reserved for H5 Pretty).
    pub detail: DetailView,
    /// Read-only Help panel (`?`); Esc/`?` close without resuming follow.
    pub help_open: bool,
    /// Home TOC vs zone page (offsets live on the variant).
    pub help_view: HelpView,
    /// Optional `/` search session (prompt + hits).
    pub help_search: Option<HelpSearch>,
    /// Last rendered Help page body height (rows). Clamps page scroll so the
    /// last line can sit at the bottom of the viewport rather than the top.
    pub help_body_view_h: usize,
    /// Session source for H10 `yc` CLI export (`-f` / live backend).
    pub export_source: crate::export::ExportSource,
    /// App settings loaded from config.toml (picker layout, etc.).
    pub config: crate::config::AppConfig,
    /// Effective TUI keymap (builtin defaults deep-merged with keymap.toml).
    pub keymap: crate::keymap::KeymapStore,
    /// Config directory (`--config-path` / `$ALNAV_HOME` / `~/.config/alnav`).
    pub config_dir: PathBuf,
    /// Cached preset catalog for `PickerKind::Preset` Manage.
    pub preset_catalog: Vec<crate::preset::Preset>,
    /// Save / rename name dialog (`C-s` / Ctrl-X).
    pub preset_name: Option<crate::preset::PresetNameDialog>,
    /// Vocabulary accumulated from ingested rows (tag/pkg/msg tokens).
    pub vocab: Vocab,
    /// Async vocab fuzzy for Picker New candidates (gen-cancel).
    pub vocab_match: crate::candidate_match::CandidateMatchService,
    /// Dirty flag for the highlight match stats cache (P1 perf optimisation).
    /// Set true on any change to visible / active highlight / highlight patterns.
    /// Cleared when `highlight_match_stats()` recomputes.
    pub match_stats_stale: bool,
    /// Cursor value used when `cached_match_stats` was last computed.
    /// Detects direct `cursor` field assignments that bypass `mark_match_stats_stale`.
    match_stats_cursor: usize,
    /// Cached result of `highlight_match_stats`. Valid when stale=false and cursor unchanged.
    pub cached_match_stats: Option<(Option<usize>, usize)>,
    /// File-mode Vis-domain highlight hit index (async Inc scan). Unused for Stream.
    pub highlight_scan: HighlightScanState,
    /// Shared domain snapshot for the in-flight File highlight worker (Inc growth).
    highlight_domain: Option<Arc<HighlightDomain>>,
    /// Jump to first hit of this highlight group once the async scan yields a hit.
    pending_jump_first: Option<usize>,
    /// Set to true the first time `drain` finds the ingest channel disconnected
    /// (file fully read or live session ended). Used by P4 draw throttle.
    pub ingest_done: bool,
    /// Throttled Filter/Exclude draft Preview cache (key = chips fingerprint).
    /// `RefCell` so picker render can refresh under `&App`.
    preview_cache: std::cell::RefCell<PreviewThrottleCache>,
    /// Leader `i` summary panel state (static `visible` snapshot).
    pub summary_view: SummaryView,
    /// Leader `i` summary background job (stale generations dropped).
    summary_job: bg_job::BgJob<SummaryReport>,
    /// Scroll offset (lines) inside the summary panel body.
    pub summary_scroll: usize,
    pub hist_view: HistView,
    pub hist_cursor: usize,
    hist_restore_key: Option<String>,
    hist_job: bg_job::BgJob<HistReport>,
    /// Restored `row_id` awaiting a resolvable visible slot. The File line index
    /// and the filter scan both grow in the background, so the jump is applied in
    /// [`Self::poll_file_store`] rather than at restore time.
    pub pending_restore_cursor: Option<u64>,
    /// `list_offset` to apply together with `pending_restore_cursor`.
    pending_restore_offset: usize,
    /// Suppresses session read **and** write for this process (R5): CLI filter
    /// flags were passed, or `restore_session = false`.
    pub session_restore_disabled: bool,
    /// Set when a restore was actually applied (gates the `RESTORED` flash).
    pub session_restored: bool,
}

#[derive(Debug, Default)]
struct PreviewThrottleCache {
    key: String,
    lines: Vec<crate::preview::PreviewHit>,
    at: Option<std::time::Instant>,
}

impl App {
    pub fn new(max_lines: usize) -> Self {
        Self {
            store: RowStore::stream(max_lines, MATCHED_HARD_CAP),
            matched_cap: MATCHED_HARD_CAP,
            visible: Visible::default(),
            file_filter_gen: 0,
            groups: GroupList::default(),
            highlight_groups: HighlightGroupList::default(),
            highlight_box: HighlightBox::default(),
            active_highlight: None,
            cursor: 0,
            max_lines,
            should_quit: false,
            focus: Focus::LogList,
            mode: Mode::Normal,
            group_cursor: 0,
            exclude_cursor: 0,
            highlight_cursor: 0,
            pending: Pending::None,
            dashboard: None,
            recent: crate::recent::RecentFiles::default(),
            log_corpus: crate::log_corpus::LogCorpus::new(),
            open_file_panel: None,
            stream_source_panel: None,
            picker: None,
            confirm: None,
            command_palette: None,
            compare: None,
            bookmarks: BookmarkList::default(),
            bookmark_row_ids: HashSet::new(),
            next_row_id: 1,
            lock_pid: None,
            lock_tid: None,
            time_bound: None,
            view_focus: ViewFocus::default(),
            time_panel: None,
            visual_anchor: None,
            following: true,
            list_offset: 0,
            collapsed_view: false,
            status_msg: None,
            status_flash_until: None,
            last_yanked: None,
            detail: DetailView::Closed,
            help_open: false,
            help_view: HelpView::default(),
            help_search: None,
            help_body_view_h: 1,
            export_source: crate::export::ExportSource::default(),
            config: crate::config::AppConfig::default_config(),
            keymap: crate::keymap::KeymapStore::builtin(),
            config_dir: crate::config::resolve_config_dir(None),
            preset_catalog: Vec::new(),
            preset_name: None,
            vocab: Vocab::default(),
            vocab_match: crate::candidate_match::CandidateMatchService::default(),
            match_stats_stale: true,
            match_stats_cursor: usize::MAX, // sentinel: force first computation
            cached_match_stats: None,
            highlight_scan: HighlightScanState::default(),
            highlight_domain: None,
            pending_jump_first: None,
            ingest_done: false,
            preview_cache: std::cell::RefCell::new(PreviewThrottleCache::default()),
            summary_view: SummaryView::Closed,
            summary_job: bg_job::BgJob::new(),
            summary_scroll: 0,
            hist_view: HistView::Closed,
            hist_cursor: 0,
            hist_restore_key: None,
            hist_job: bg_job::BgJob::new(),
            pending_restore_cursor: None,
            pending_restore_offset: 0,
            session_restore_disabled: false,
            session_restored: false,
        }
    }
}

#[cfg(test)]
mod tests;
