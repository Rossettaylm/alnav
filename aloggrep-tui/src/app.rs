use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::bookmark::{bookmark_label, AddError, Bookmark, BookmarkList, JumpResult};
use crate::filter_model::{ExcludeEntry, Group, GroupList, TimeBound};
use crate::highlight_model::{HighlightBox, HighlightGroup, HighlightGroupList};
use crate::model::{is_severe_row, EntryRow};
use crate::scan::{HighlightDomain, HighlightScanState};
use crate::store::{FileEvent, FileStore, RowRef, RowStore, StreamStore};
use crate::time_panel::TimePanel;
use crate::vocab::Vocab;

/// Max synchronous `row_at` parses per `find_severe` keypress in File mode
/// when the severe cache has not yet been filled (prefetch miss).
const SEVERE_SYNC_PARSE_BUDGET: usize = 256;

/// Hard cap on the matched-rows buffer (OOM safety). When a filter is active,
/// matching rows are retained in `App::matched` independently of `rows`'
/// rolling eviction; only this cap reclaims them.
const MATCHED_HARD_CAP: usize = 1_000_000;

/// Soft per-frame drain budget: protect the UI when the ingest ring was filled
/// by a burst. Remaining rows stay in the ring for subsequent frames.
const DRAIN_BUDGET_PER_FRAME: usize = 4096;

/// Visible-row index set into the active row source.
///
/// Stream path (`--hdc`) always uses [`Visible::All`]: an identity mapping
/// `0..len` — never a materialised `Vec<usize>`. File path uses [`Visible::All`]
/// when unfiltered and [`Visible::Subset`] (hit line numbers) when filtered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Visible {
    /// Identity: visible slot `i` maps to source index `i`.
    All { len: usize },
    /// Sparse hits (file/mmap filter): visible slot `i` → `hits[i]` line index.
    Subset(Vec<usize>),
}

impl Visible {
    pub fn len(&self) -> usize {
        match self {
            Visible::All { len } => *len,
            Visible::Subset(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        *self = Visible::All { len: 0 };
    }

    /// Source index for visible slot `vis_i`.
    pub fn source_idx(&self, vis_i: usize) -> Option<usize> {
        match self {
            Visible::All { len } if vis_i < *len => Some(vis_i),
            Visible::All { .. } => None,
            Visible::Subset(v) => v.get(vis_i).copied(),
        }
    }
}

impl Default for Visible {
    fn default() -> Self {
        Visible::All { len: 0 }
    }
}

fn group_to_exclude_entry(group: Group) -> Option<ExcludeEntry> {
    if group.chips.len() != 1 {
        return None;
    }
    let chip = group.chips.first()?.clone();
    let expr = group.expr?;
    Some(ExcludeEntry {
        chip,
        expr,
        enabled: group.enabled,
    })
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

pub struct App {
    /// Stream (`--hdc`/tests) or mmap file (`-f`) row backend.
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
    /// Armed by first `d` on a chip strip; second `d` deletes, `i` toggles disable.
    pub pending_d: bool,
    pub pending_yank: bool,
    /// Armed by `c` on LogList; second key picks a field (H7).
    pub pending_chip: bool,
    /// Armed by `C` on LogList; second key picks a field to exclude (H9).
    pub pending_exclude: bool,
    /// Armed by `f` on LogList; second key locks pid/tid or clears (H8).
    pub pending_lock: bool,
    /// Armed by `t` on LogList (file mode); second key opens/clears time window.
    pub pending_time: bool,
    /// Armed by `m` on LogList; second key is `a`/`d` (M2).
    pub pending_m: bool,
    /// Armed by leader key (`Space`); second key opens fzf-style picker (Task 5).
    pub pending_leader: bool,
    /// Open fzf-style picker session (Unified Manage / Filter / Highlight / Bookmark / Exclude).
    pub picker: Option<crate::picker::PickerSession>,
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
    /// Open `ts` time-window editor (`None` when closed).
    pub time_panel: Option<TimePanel>,
    /// When `Some`, LogList is in visual-line mode; value is the anchor
    /// index into `visible` (same coordinate space as `cursor`).
    pub visual_anchor: Option<usize>,
    pub following: bool,
    pub list_offset: usize,
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
    /// Scroll offset (lines) inside the Help body.
    pub help_scroll: usize,
    /// Session source for H10 `yc` CLI export (`-f` / `--hdc`).
    pub export_source: crate::export::ExportSource,
    /// App settings loaded from config.toml (picker layout, etc.).
    pub config: crate::config::AppConfig,
    /// Vocabulary accumulated from ingested rows (tag/pkg/msg tokens).
    pub vocab: Vocab,
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
    /// (file fully read or --hdc session ended). Used by P4 draw throttle.
    pub ingest_done: bool,
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
            pending_d: false,
            pending_yank: false,
            pending_chip: false,
            pending_exclude: false,
            pending_lock: false,
            pending_time: false,
            pending_m: false,
            pending_leader: false,
            picker: None,
            bookmarks: BookmarkList::default(),
            bookmark_row_ids: HashSet::new(),
            next_row_id: 1,
            lock_pid: None,
            lock_tid: None,
            time_bound: None,
            time_panel: None,
            visual_anchor: None,
            following: true,
            list_offset: 0,
            status_msg: None,
            status_flash_until: None,
            last_yanked: None,
            detail: DetailView::Closed,
            help_open: false,
            help_scroll: 0,
            export_source: crate::export::ExportSource::default(),
            config: crate::config::AppConfig::default_config(),
            vocab: Vocab::default(),
            match_stats_stale: true,
            match_stats_cursor: usize::MAX, // sentinel: force first computation
            cached_match_stats: None,
            highlight_scan: HighlightScanState::default(),
            highlight_domain: None,
            pending_jump_first: None,
            ingest_done: false,
        }
    }

    /// Install a mmap file backend (replaces the default stream store).
    pub fn set_file_store(&mut self, file: FileStore) {
        let n = file.line_count();
        let done = file.index_done();
        self.store = RowStore::File(file);
        self.visible = Visible::All { len: n };
        self.ingest_done = done && !self.filter_active();
        self.file_filter_gen = 0;
        // Drain priming events (e.g. open_sync IndexDone) + start filter if needed.
        self.poll_file_store();
        if self.filter_active() {
            self.rebuild_visible();
        } else if self.following {
            self.jump_bottom();
        }
        self.restart_highlight_scan();
    }

    /// Stream rolling buffer (tests / hdc). Empty static for File.
    pub fn rows(&self) -> &VecDeque<EntryRow> {
        static EMPTY: std::sync::OnceLock<VecDeque<EntryRow>> = std::sync::OnceLock::new();
        match &self.store {
            RowStore::Stream(s) => &s.rows,
            RowStore::File(_) => EMPTY.get_or_init(VecDeque::new),
        }
    }

    /// Stream matched buffer (tests / hdc). Empty static for File.
    pub fn matched(&self) -> &VecDeque<EntryRow> {
        static EMPTY: std::sync::OnceLock<VecDeque<EntryRow>> = std::sync::OnceLock::new();
        match &self.store {
            RowStore::Stream(s) => &s.matched,
            RowStore::File(_) => EMPTY.get_or_init(VecDeque::new),
        }
    }

    fn stream_mut(&mut self) -> &mut StreamStore {
        self.store
            .as_stream_mut()
            .expect("stream operation on file store")
    }

    /// Visible slot → row (lazy-parse for File).
    pub fn row_at(&self, vis_i: usize) -> Option<RowRef<'_>> {
        let src = self.source_idx_for_visible(vis_i)?;
        // File ignores filter_active for buffer selection (source is line idx).
        let filter_active = matches!(self.store, RowStore::Stream(_)) && self.filter_active();
        self.store.row_at_source(src, filter_active)
    }

    /// Open the unified Manage picker (aggregated Filter/Highlight/Exclude/Bookmark).
    pub fn open_unified_picker(&mut self) {
        self.open_picker(crate::picker::PickerKind::Unified);
    }

    /// Open the requested fzf-style picker in Manage mode. Clears operator-pending.
    /// Does not auto-switch to New (use [`Self::open_picker_new`]).
    pub fn open_picker(&mut self, kind: crate::picker::PickerKind) {
        self.open_picker_with(kind, false);
    }

    /// Open the picker forced into New mode (`:` `/` `` ` `` `mm`).
    pub fn open_picker_new(&mut self, kind: crate::picker::PickerKind) {
        self.open_picker_with(kind, true);
    }

    fn open_picker_with(&mut self, kind: crate::picker::PickerKind, prefer_new: bool) {
        use crate::picker::PickerSession;

        self.pending_d = false;
        self.pending_yank = false;
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_lock = false;
        self.pending_time = false;
        self.pending_m = false;
        self.pending_leader = false;
        let mut session = PickerSession::open(kind);
        if prefer_new {
            session.enter_new();
        }
        self.picker = Some(session);
    }

    /// Close the fzf-style picker and return focus to LogList.
    /// Does not change live-follow state.
    pub fn close_picker(&mut self) {
        self.picker = None;
        self.pending_leader = false;
        self.focus = Focus::LogList;
    }

    /// H10: one-line `aloggrep` command for the current filter state.
    pub fn export_cli_command(&self) -> String {
        crate::export::build_cli_command(
            &self.export_source,
            &self.groups,
            self.lock_pid.as_deref(),
            self.lock_tid.as_deref(),
            self.time_bound.as_ref(),
        )
    }

    /// File-mode sessions may interactively edit the global time window.
    pub fn is_file_mode(&self) -> bool {
        matches!(self.export_source, crate::export::ExportSource::File(_))
    }

    pub fn detail_open(&self) -> bool {
        !matches!(self.detail, DetailView::Closed)
    }

    /// Open the read-only Help panel. Does not change `following`.
    pub fn open_help(&mut self) {
        self.help_open = true;
        self.help_scroll = 0;
    }

    /// Close Help without resuming follow (same as Detail Esc).
    pub fn close_help(&mut self) {
        self.help_open = false;
        self.help_scroll = 0;
    }

    pub fn scroll_help(&mut self, delta: isize) {
        if !self.help_open {
            return;
        }
        let lines = crate::help::help_body_lines(self).len();
        if delta < 0 {
            self.help_scroll = self.help_scroll.saturating_sub((-delta) as usize);
        } else {
            let max = lines.saturating_sub(1);
            self.help_scroll = (self.help_scroll + delta as usize).min(max);
        }
    }

    /// Toggle overlay with `p`: open → Fields; any open mode → Closed.
    /// Does not change `following`.
    pub fn toggle_detail_fields(&mut self) {
        self.detail = match self.detail {
            DetailView::Closed => DetailView::Fields,
            DetailView::Fields | DetailView::Pretty => DetailView::Closed,
        };
    }

    /// H5 `P`: Closed/Fields → Pretty; Pretty → Fields. Does not change `following`.
    pub fn toggle_detail_pretty(&mut self) {
        self.detail = match self.detail {
            DetailView::Closed | DetailView::Fields => DetailView::Pretty,
            DetailView::Pretty => DetailView::Fields,
        };
    }

    /// Close detail overlay without touching `following`.
    pub fn close_detail(&mut self) {
        self.detail = DetailView::Closed;
    }

    /// Chip filter → session lock (H8) → global time window. Used by drain/rebuild.
    ///
    /// CLI-aligned: when any filter is active, unparsed (raw-fallback) rows never pass.
    pub fn row_passes_filters(&self, row: &EntryRow) -> bool {
        Self::row_passes_filter_parts(
            row,
            &self.groups,
            self.lock_pid.as_deref(),
            self.lock_tid.as_deref(),
            self.time_bound.as_ref(),
            /*reject_unparsed*/ self.filter_active(),
        )
    }

    /// Shared predicate for stream `row_passes_filters` and file `FilterPred`.
    /// File scans only run while filters are active → pass `reject_unparsed = true`.
    pub(crate) fn row_passes_filter_parts(
        row: &EntryRow,
        groups: &crate::filter_model::GroupList,
        lock_pid: Option<&str>,
        lock_tid: Option<&str>,
        time_bound: Option<&crate::filter_model::TimeBound>,
        reject_unparsed: bool,
    ) -> bool {
        if reject_unparsed && !row.is_parsed() {
            return false;
        }
        if !groups.matches(row) {
            return false;
        }
        if let Some(pid) = lock_pid {
            if row.pid != *pid {
                return false;
            }
        } else if let Some(tid) = lock_tid {
            if row.tid != *tid {
                return false;
            }
        }
        if let Some(bound) = time_bound {
            if bound.is_active() && !bound.matches(&row.as_log_entry()) {
                return false;
            }
        }
        true
    }

    /// Whether any include/exclude/lock/time filter is currently active. When false,
    /// `visible` indexes `rows` directly (every row shown); when true, `visible`
    /// indexes `matched` (only filter-passing rows, retained across `rows` churn).
    pub fn filter_active(&self) -> bool {
        self.groups.has_any_enabled()
            || self.groups.excludes.iter().any(|e| e.enabled)
            || self.lock_pid.is_some()
            || self.lock_tid.is_some()
            || self.time_bound.as_ref().is_some_and(|t| t.is_active())
    }

    /// Stream-only: the buffer `visible` indexes (`matched` when filter
    /// active). Prefer [`Self::row_at`] for render paths (works for File too).
    pub fn view_source(&self) -> &VecDeque<EntryRow> {
        match &self.store {
            RowStore::Stream(s) => s.view_source(self.filter_active()),
            RowStore::File(_) => self.rows(), // empty
        }
    }

    /// Number of currently visible rows.
    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    /// Source index for visible slot `vis_i`.
    pub fn source_idx_for_visible(&self, vis_i: usize) -> Option<usize> {
        self.visible.source_idx(vis_i)
    }

    /// Whether `row_id` is still present (bookmark liveness).
    fn row_alive(&self, row_id: u64) -> bool {
        self.store.row_alive(row_id)
    }

    /// Drain pending rows from a stream ingest source without blocking.
    /// No-op for File (use [`Self::poll_file_store`]).
    pub fn drain(&mut self, ingest: &impl crate::ingest::TryRecvRow) {
        if self.store.is_file() {
            return;
        }
        let mut drained = 0usize;
        loop {
            if drained >= DRAIN_BUDGET_PER_FRAME {
                break;
            }
            match ingest.try_recv_row() {
                Ok(row) => {
                    self.push_row(row);
                    drained += 1;
                }
                Err(crate::ingest::TryRecvKind::Empty) => break,
                Err(crate::ingest::TryRecvKind::Disconnected) => {
                    self.ingest_done = true;
                    break;
                }
            }
        }
    }

    /// Poll mmap indexer / filter / highlight worker events (file mode). Call each frame.
    pub fn poll_file_store(&mut self) {
        let Some(_) = self.store.as_file() else {
            return;
        };
        let events = self
            .store
            .as_file()
            .map(|f| f.drain_events())
            .unwrap_or_default();
        let mut index_grew = false;
        let mut filter_batches: Vec<(u64, Vec<usize>)> = Vec::new();
        let mut filter_done_gen = None;
        let mut index_done = false;
        let mut hl_batches: Vec<(u64, Vec<usize>, usize)> = Vec::new();
        let mut hl_done_gen = None;
        for ev in events {
            match ev {
                FileEvent::IndexProgress { line_count, .. } => {
                    index_grew = true;
                    if let Some(f) = self.store.as_file() {
                        f.grow_severe_cache();
                    }
                    if !self.filter_active() {
                        self.visible = Visible::All { len: line_count };
                        if let Some(dom) = &self.highlight_domain {
                            dom.set_identity_len(line_count);
                        }
                    }
                }
                FileEvent::IndexDone { line_count } => {
                    index_grew = true;
                    index_done = true;
                    if let Some(f) = self.store.as_file() {
                        f.grow_severe_cache();
                    }
                    if !self.filter_active() {
                        self.visible = Visible::All { len: line_count };
                        if let Some(dom) = &self.highlight_domain {
                            dom.set_identity_len(line_count);
                            dom.seal();
                        }
                    }
                    // One-shot sampled vocab after index completes.
                    if let Some(f) = self.store.as_file() {
                        if f.mark_vocab_started() {
                            let mut feeds = Vec::new();
                            f.feed_vocab_sample(|tag, pkg, tokens| {
                                feeds.push((tag.to_string(), pkg.to_string(), tokens.to_vec()));
                            });
                            for (tag, pkg, tokens) in feeds {
                                self.vocab.feed(&tag, &pkg, &tokens);
                            }
                        }
                    }
                }
                FileEvent::FilterBatch { gen, hits, .. } => {
                    filter_batches.push((gen, hits));
                }
                FileEvent::FilterDone { gen, .. } => {
                    filter_done_gen = Some(gen);
                }
                FileEvent::HighlightBatch { gen, hits, scanned } => {
                    hl_batches.push((gen, hits, scanned));
                }
                FileEvent::HighlightDone { gen, scanned } => {
                    hl_done_gen = Some((gen, scanned));
                }
            }
        }
        for (gen, hits) in filter_batches {
            if gen != self.file_filter_gen {
                continue;
            }
            // Inc: grow highlight domain with Subset so the worker can continue.
            if let Some(dom) = &self.highlight_domain {
                dom.extend_subset(&hits);
            }
            match &mut self.visible {
                Visible::Subset(v) => v.extend(hits),
                _ => self.visible = Visible::Subset(hits),
            }
            // Never O(visible) parse here — highlight Inc follows Subset growth.
        }
        if let Some(gen) = filter_done_gen {
            if gen == self.file_filter_gen {
                // ensure Subset even if zero hits
                if !matches!(self.visible, Visible::Subset(_)) && self.filter_active() {
                    self.visible = Visible::Subset(Vec::new());
                }
                if let Some(dom) = &self.highlight_domain {
                    dom.seal();
                }
                // Refresh cached ordinal from hit index (no full parse).
                self.match_stats_stale = true;
            }
        }
        for (gen, hits, scanned) in hl_batches {
            if gen != self.highlight_scan.gen {
                continue;
            }
            self.highlight_scan.hits.extend(hits);
            self.highlight_scan.scanned_vis = scanned;
            self.match_stats_stale = true;
            self.try_pending_jump_first();
        }
        if let Some((gen, scanned)) = hl_done_gen {
            if gen == self.highlight_scan.gen {
                self.highlight_scan.scanned_vis = scanned;
                self.highlight_scan.done = true;
                self.match_stats_stale = true;
                self.try_pending_jump_first();
            }
        }
        if index_grew || index_done {
            if self.following {
                self.jump_bottom();
            } else if self.cursor >= self.visible.len() {
                self.cursor = self.visible.len().saturating_sub(1);
            }
        }
        if index_done {
            // Cursor-only refresh from hit index — never full UI parse.
            self.match_stats_stale = true;
            if !self.filter_active() {
                self.ingest_done = true;
            }
        }
        if let Some(gen) = filter_done_gen {
            if gen == self.file_filter_gen && self.filter_active() {
                self.ingest_done = true;
            }
        }
    }

    fn try_pending_jump_first(&mut self) {
        let Some(group_idx) = self.pending_jump_first else {
            return;
        };
        if self.active_highlight != Some(group_idx) {
            self.pending_jump_first = None;
            return;
        }
        if let Some(h) = self.highlight_scan.first_hit() {
            self.following = false;
            self.cursor = h;
            self.match_stats_stale = true;
            self.pending_jump_first = None;
        } else if self.highlight_scan.done {
            self.pending_jump_first = None;
        }
    }

    /// Status fragment for file load/filter progress (None when idle/stream).
    pub fn file_progress_label(&self) -> Option<String> {
        let f = self.store.as_file()?;
        let p = f.progress();
        if !p.index_done {
            let pct = if p.file_bytes == 0 {
                100
            } else {
                (p.indexed_bytes.saturating_mul(100) / p.file_bytes).min(100)
            };
            return Some(format!("INDEX {}% ({} lines)", pct, p.indexed_lines));
        }
        if self.filter_active() && !p.filter_done {
            let total = p.indexed_lines.max(1);
            let pct = (p.filter_scanned.saturating_mul(100) / total).min(100);
            return Some(format!("FILTER {pct}%"));
        }
        None
    }

    /// LogList title/banner loading text (index / filter / highlight). Free: does
    /// not block input. Stream sessions always return `None`.
    pub fn log_loading_label(&self) -> Option<String> {
        let f = self.store.as_file()?;
        let p = f.progress();
        if !p.index_done {
            let pct = if p.file_bytes == 0 {
                100
            } else {
                (p.indexed_bytes.saturating_mul(100) / p.file_bytes).min(100)
            };
            return Some(format!("Indexing {pct}%…"));
        }
        if self.filter_active() && !p.filter_done {
            let total = p.indexed_lines.max(1);
            let pct = (p.filter_scanned.saturating_mul(100) / total).min(100);
            return Some(format!("Filtering {pct}%…"));
        }
        if self.active_highlight_group().is_some() && !self.highlight_scan.done {
            let total = self.visible.len().max(1);
            let pct = (self.highlight_scan.scanned_vis.saturating_mul(100) / total).min(100);
            return Some(format!("Highlight {pct}%…"));
        }
        None
    }

    /// Cancel + restart File highlight Vis scan for the active group (or clear).
    pub fn restart_highlight_scan(&mut self) {
        self.pending_jump_first = None;
        if !self.store.is_file() {
            self.highlight_scan.clear();
            self.highlight_domain = None;
            self.match_stats_stale = true;
            return;
        }
        let Some(group) = self.active_highlight_group().cloned() else {
            if let Some(f) = self.store.as_file_mut() {
                f.cancel_highlight_scan();
            }
            self.highlight_scan.clear();
            self.highlight_scan.done = true;
            self.highlight_domain = None;
            self.match_stats_stale = true;
            return;
        };
        let domain = match &self.visible {
            Visible::All { len } => {
                let d = HighlightDomain::identity(*len);
                // Seal when index already done and no filter growth expected.
                if let Some(f) = self.store.as_file() {
                    let p = f.progress();
                    if p.index_done && !self.filter_active() {
                        d.seal();
                    }
                }
                d
            }
            Visible::Subset(v) => {
                let d = HighlightDomain::subset(v.clone());
                if let Some(f) = self.store.as_file() {
                    if f.progress().filter_done {
                        d.seal();
                    }
                }
                d
            }
        };
        self.highlight_domain = Some(Arc::clone(&domain));
        self.highlight_scan.clear();
        let gen = self
            .store
            .as_file_mut()
            .expect("file")
            .start_highlight_scan(domain, &group.re);
        self.highlight_scan.gen = gen;
        self.highlight_scan.done = false;
        self.match_stats_stale = true;
    }

    fn push_row(&mut self, mut row: EntryRow) {
        let msg_tokens = crate::input::tokenize_msg_for_vocab(&row.msg);
        self.vocab.feed(&row.tag, &row.pkg, &msg_tokens);
        row.row_id = self.next_row_id;
        self.next_row_id = self.next_row_id.wrapping_add(1);
        // P2: compute severe once at ingest so minimap/find_severe never re-run CrashDetector.
        row.severe = is_severe_row(&row);
        let active = self.filter_active();
        let matches = active && self.row_passes_filters(&row);
        let max_lines = self.max_lines;
        let matched_cap = self.matched_cap;

        let mut evict_rows = false;
        let mut evict_matched = false;
        {
            let stream = self.stream_mut();
            stream.max_lines = max_lines;
            stream.matched_cap = matched_cap;
            if stream.rows.len() >= stream.max_lines && stream.rows.pop_front().is_some() && !active
            {
                evict_rows = true;
            }
            if matches
                && stream.matched.len() >= stream.matched_cap
                && stream.matched.pop_front().is_some()
            {
                evict_matched = true;
            }
        }
        if evict_rows || evict_matched {
            self.adjust_after_identity_front_evict();
        }

        let stream = self.stream_mut();
        if matches {
            stream.rows.push_back(row.clone());
            stream.matched.push_back(row);
            let len = stream.matched.len();
            self.visible = Visible::All { len };
        } else {
            stream.rows.push_back(row);
            if !active {
                let len = stream.rows.len();
                self.visible = Visible::All { len };
            }
        }
        self.follow_tick();
        self.match_stats_stale = true;
    }

    /// O(1) cursor/viewport adjustment after the front row of the identity
    /// `view_source` was evicted and a replacement is about to be (or was)
    /// appended — `Visible::All.len` is unchanged across the pop+push pair.
    fn adjust_after_identity_front_evict(&mut self) {
        // No visible slots ⇒ nothing was on-screen at source index 0.
        if self.visible.is_empty() {
            return;
        }
        // Identity: source index 0 was always visible slot 0 when non-empty.
        if self.list_offset > 0 {
            self.list_offset -= 1;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
        }
        // `visual_anchor` shares `cursor`'s coordinate space (index into
        // visible slots). Evicting the oldest visible row shifts that space.
        match self.visual_anchor {
            Some(0) => self.visual_anchor = None,
            Some(a) => self.visual_anchor = Some(a - 1),
            None => {}
        }
    }

    /// Full rescan, used when the filter groups or session lock change.
    /// Stream: rebuilds `matched` from `rows`. File: starts a cancellable
    /// background filter scan into [`Visible::Subset`] (or All when inactive).
    pub fn rebuild_visible(&mut self) {
        let active = self.filter_active();
        if self.store.is_file() {
            if !active {
                if let Some(file) = self.store.as_file_mut() {
                    file.cancel_filter_scan();
                    let n = file.line_count();
                    let done = file.index_done();
                    self.file_filter_gen = 0;
                    self.visible = Visible::All { len: n };
                    self.ingest_done = done;
                }
            } else {
                self.visible = Visible::Subset(Vec::new());
                self.ingest_done = false;
                let groups = self.groups.clone();
                let lock_pid = self.lock_pid.clone();
                let lock_tid = self.lock_tid.clone();
                let time_bound = self.time_bound.clone();
                let pred: crate::store::FilterPred = Arc::new(move |row: &EntryRow| {
                    Self::row_passes_filter_parts(
                        row,
                        &groups,
                        lock_pid.as_deref(),
                        lock_tid.as_deref(),
                        time_bound.as_ref(),
                        true,
                    )
                });
                let gen = self
                    .store
                    .as_file_mut()
                    .expect("file")
                    .start_filter_scan(pred);
                self.file_filter_gen = gen;
            }
        } else if active {
            let rows: Vec<EntryRow> = self.stream_mut().rows.iter().cloned().collect();
            let passing: Vec<EntryRow> = rows
                .into_iter()
                .filter(|r| self.row_passes_filters(r))
                .collect();
            let stream = self.stream_mut();
            stream.matched.clear();
            stream.matched.extend(passing);
            self.visible = Visible::All {
                len: stream.matched.len(),
            };
        } else {
            let stream = self.stream_mut();
            stream.matched.clear();
            self.visible = Visible::All {
                len: stream.rows.len(),
            };
        }

        if self.following {
            self.jump_bottom();
        } else if self.cursor >= self.visible.len() {
            self.cursor = self.visible.len().saturating_sub(1);
        }
        self.match_stats_stale = true;
        // File: filter change invalidates Vis domain — restart highlight Inc.
        if self.store.is_file() {
            self.restart_highlight_scan();
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let new = self.cursor as isize + delta;
        self.cursor = new.clamp(0, self.visible.len() as isize - 1) as usize;
        self.match_stats_stale = true;
    }

    pub fn jump_top(&mut self) {
        self.cursor = 0;
        self.match_stats_stale = true;
    }

    pub fn jump_bottom(&mut self) {
        self.cursor = self.visible.len().saturating_sub(1);
        self.match_stats_stale = true;
    }

    /// Call after any new rows are appended in `drain`/`push_row`'s path: if
    /// following, keep the cursor pinned to the last visible row.
    pub fn follow_tick(&mut self) {
        if self.following {
            self.jump_bottom();
        }
    }

    /// Any manual cursor movement pauses following. Resume only via Esc on
    /// LogList (also Visual Esc / successful filter-group submit).
    pub fn move_cursor_manual(&mut self, delta: isize) {
        self.following = false;
        self.move_cursor(delta);
    }

    /// Pin to bottom and resume live follow (Esc on LogList / Visual Esc /
    /// filter-group submit).
    pub fn resume_following(&mut self) {
        self.following = true;
        self.jump_bottom();
    }

    /// Clear buffered log rows for `--hdc` Ctrl-L: drops `rows` / `matched` /
    /// `visible` and bookmarks, keeps filter/highlight/exclude/lock, resumes
    /// following, and flashes `CLEARED`.
    pub fn clear_buffered_logs(&mut self) {
        if let Some(stream) = self.store.as_stream_mut() {
            stream.clear();
        }
        self.visible.clear();
        self.clear_bookmarks();
        self.clear_visual();
        self.pending_d = false;
        self.pending_yank = false;
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_lock = false;
        self.pending_time = false;
        self.pending_m = false;
        self.pending_leader = false;
        self.cursor = 0;
        self.list_offset = 0;
        self.match_stats_stale = true;
        self.resume_following();
        self.set_flash("CLEARED");
    }

    /// Collect currently visible rows (owned clones; prefer [`Self::row_at`]
    /// for hot paths).
    pub fn visible_rows(&self) -> Vec<EntryRow> {
        let len = self.visible.len();
        (0..len)
            .filter_map(|i| self.row_at(i).map(|r| r.into_owned()))
            .collect()
    }

    pub fn cycle_focus_forward(&mut self) {
        self.focus = match self.focus {
            Focus::ChipStrip => Focus::ExcludeStrip,
            Focus::ExcludeStrip => Focus::HighlightStrip,
            Focus::HighlightStrip => Focus::LogList,
            Focus::LogList => Focus::Input,
            Focus::Input => Focus::ChipStrip,
        };
    }

    pub fn cycle_focus_backward(&mut self) {
        self.focus = match self.focus {
            Focus::ChipStrip => Focus::Input,
            Focus::ExcludeStrip => Focus::ChipStrip,
            Focus::HighlightStrip => Focus::ExcludeStrip,
            Focus::LogList => Focus::HighlightStrip,
            Focus::Input => Focus::LogList,
        };
    }

    /// Cycle focus forward among *visible* regions only (Normal mode Tab).
    ///
    /// Visible = non-empty Filter/Exclude/Highlight strips + LogList. Empty
    /// (collapsed) strips are skipped. Never returns `Focus::Input`, so the
    /// unified picker is never opened via Tab.
    pub fn cycle_visible_focus_forward(&mut self) {
        self.focus = self.next_visible_focus(true);
    }

    /// Backward counterpart of [`cycle_visible_focus_forward`].
    pub fn cycle_visible_focus_backward(&mut self) {
        self.focus = self.next_visible_focus(false);
    }

    fn visible_regions(&self) -> Vec<Focus> {
        let mut v = Vec::with_capacity(4);
        if self.strip_len(StripKind::Filter) > 0 {
            v.push(Focus::ChipStrip);
        }
        if self.strip_len(StripKind::Exclude) > 0 {
            v.push(Focus::ExcludeStrip);
        }
        if self.strip_len(StripKind::Highlight) > 0 {
            v.push(Focus::HighlightStrip);
        }
        v.push(Focus::LogList);
        v
    }

    fn next_visible_focus(&self, forward: bool) -> Focus {
        let regions = self.visible_regions();
        // LogList is always present, so `regions` is never empty.
        let cur = regions
            .iter()
            .position(|&f| f == self.focus)
            .unwrap_or_else(|| regions.iter().position(|&f| f == Focus::LogList).unwrap());
        let step = if forward { 1 } else { regions.len() - 1 };
        regions[(cur + step) % regions.len()]
    }

    fn strip_len(&self, kind: StripKind) -> usize {
        match kind {
            StripKind::Filter => self.groups.groups.len(),
            StripKind::Exclude => self.groups.excludes.len(),
            StripKind::Highlight => self.highlight_groups.groups.len(),
        }
    }

    fn strip_cursor_mut(&mut self, kind: StripKind) -> &mut usize {
        match kind {
            StripKind::Filter => &mut self.group_cursor,
            StripKind::Exclude => &mut self.exclude_cursor,
            StripKind::Highlight => &mut self.highlight_cursor,
        }
    }

    pub fn move_strip_cursor(&mut self, kind: StripKind, delta: isize) {
        let len = self.strip_len(kind);
        if len == 0 {
            return;
        }
        let cursor = *self.strip_cursor_mut(kind);
        let new = (cursor as isize + delta).clamp(0, len as isize - 1) as usize;
        *self.strip_cursor_mut(kind) = new;
    }

    pub fn move_group_cursor(&mut self, delta: isize) {
        self.move_strip_cursor(StripKind::Filter, delta);
    }

    /// Delete the focused group on `kind`. Empty filter strip returns focus
    /// to LogList and rebuilds visible; empty search strip only clamps cursor.
    pub fn delete_focused_strip_group(&mut self, kind: StripKind) {
        if self.strip_len(kind) == 0 {
            return;
        }
        let cursor = *self.strip_cursor_mut(kind);
        let deleted = match kind {
            StripKind::Filter => self.delete_filter_group_at(cursor),
            StripKind::Exclude => self.delete_exclude_group_at(cursor),
            StripKind::Highlight => self.delete_highlight_group_at(cursor),
        };
        debug_assert!(deleted);
        let empty = match kind {
            StripKind::Filter => self.groups.groups.is_empty(),
            StripKind::Exclude => self.groups.excludes.is_empty(),
            StripKind::Highlight => self.highlight_groups.groups.is_empty(),
        };
        if empty {
            self.focus = Focus::LogList;
        }
    }

    /// After removing search group at `removed`, keep `active_highlight` valid.
    /// Deleting the active group (or emptying the list) falls back to the
    /// newest remaining group; deleting a group left of active shifts the index.
    fn fix_active_highlight_after_delete(&mut self, removed: usize) {
        let len = self.highlight_groups.groups.len();
        if len == 0 {
            self.active_highlight = None;
            self.match_stats_stale = true;
            return;
        }
        match self.active_highlight {
            Some(active) if active == removed => {
                self.active_highlight = Some(len - 1);
                self.match_stats_stale = true;
            }
            Some(active) if active > removed => {
                self.active_highlight = Some(active - 1);
                self.match_stats_stale = true;
            }
            _ => {}
        }
    }

    /// Enabled search group currently marked as global active, if any.
    pub fn active_highlight_group(&self) -> Option<&HighlightGroup> {
        let idx = self.active_highlight?;
        let g = self.highlight_groups.groups.get(idx)?;
        if g.enabled {
            Some(g)
        } else {
            None
        }
    }

    pub fn delete_focused_group(&mut self) {
        self.delete_focused_strip_group(StripKind::Filter);
    }

    /// Toggle `enabled` on the focused group (`di`). Does not change focus
    /// when all groups become disabled.
    pub fn toggle_disable_focused(&mut self, kind: StripKind) {
        let len = self.strip_len(kind);
        if len == 0 {
            return;
        }
        let cursor = *self.strip_cursor_mut(kind);
        match kind {
            StripKind::Filter => {
                let g = &mut self.groups.groups[cursor];
                g.enabled = !g.enabled;
                self.rebuild_visible();
            }
            StripKind::Exclude => {
                let e = &mut self.groups.excludes[cursor];
                e.enabled = !e.enabled;
                self.rebuild_visible();
            }
            StripKind::Highlight => {
                let g = &mut self.highlight_groups.groups[cursor];
                g.enabled = !g.enabled;
                self.match_stats_stale = true;
                if self.store.is_file() {
                    self.restart_highlight_scan();
                }
            }
        }
    }

    pub fn current_row(&self) -> Option<RowRef<'_>> {
        self.row_at(self.cursor)
    }

    /// Flash a short status-bar toast that auto-hides after 3 seconds.
    pub fn set_flash(&mut self, msg: impl Into<String>) {
        self.status_msg = Some(msg.into());
        self.status_flash_until = Some(Instant::now() + Duration::from_secs(3));
    }

    /// Clear any timed flash toast immediately.
    pub fn clear_flash(&mut self) {
        self.status_msg = None;
        self.status_flash_until = None;
    }

    /// Drop flash toast when its deadline has passed (call each frame).
    pub fn tick_flash(&mut self) {
        if let Some(until) = self.status_flash_until {
            if Instant::now() >= until {
                self.clear_flash();
            }
        }
    }

    /// Arm `m` operator-pending (M2 bookmarks).
    pub fn begin_bookmark_op(&mut self) {
        self.clear_visual();
        self.pending_yank = false;
        self.pending_d = false;
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_lock = false;
        self.pending_time = false;
        self.pending_m = false;
        self.pending_leader = false;
        self.pending_m = true;
    }

    pub fn cancel_bookmark_op(&mut self) {
        self.pending_m = false;
        self.pending_leader = false;
    }

    /// `ma`: bookmark current LogList row.
    pub fn bookmark_add_current(&mut self) {
        self.pending_m = false;
        self.pending_leader = false;
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return;
        };
        let row_id = row.row_id;
        let bm = Bookmark {
            row_id,
            label: bookmark_label(&row.tag, &row.msg),
        };
        match self.bookmarks.try_add(bm) {
            Ok(()) => {
                self.bookmark_row_ids.insert(row_id);
                self.set_flash("BOOKMARKED");
            }
            Err(AddError::Duplicate) => self.set_flash("EXISTS"),
            Err(AddError::Full) => self.set_flash("BOOKMARKS FULL"),
        }
    }

    /// `md`: remove bookmark for current row.
    pub fn bookmark_remove_current(&mut self) {
        self.pending_m = false;
        self.pending_leader = false;
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return;
        };
        let row_id = row.row_id;
        if self.bookmarks.remove_id(row_id) {
            self.bookmark_row_ids.remove(&row_id);
            self.set_flash("REMOVED");
        } else {
            self.set_flash("NOT BOOKMARKED");
        }
    }

    /// Map `row_id` → visible slot without parsing file lines.
    /// `Subset` hits are append-ordered, so lookup is binary search.
    pub fn visible_idx_for_row_id(&self, row_id: u64) -> Option<usize> {
        let filter_active = matches!(self.store, RowStore::Stream(_)) && self.filter_active();
        let src_idx = self.store.find_row_id(row_id, filter_active)?;
        match &self.visible {
            Visible::All { len } if src_idx < *len => Some(src_idx),
            Visible::All { .. } => None,
            Visible::Subset(v) => v.binary_search(&src_idx).ok(),
        }
    }

    /// Jump to a bookmarked row_id; sets `following=false` on success.
    /// A bookmark is jumpable iff its row is still alive (`row_alive`).
    /// A row retained in `matched` (evicted from `rows`) is still jumpable.
    pub fn jump_to_bookmark(&mut self, row_id: u64) -> JumpResult {
        let Some(vis_idx) = self.visible_idx_for_row_id(row_id) else {
            return if self.row_alive(row_id) {
                JumpResult::Filtered
            } else {
                JumpResult::Evicted
            };
        };
        self.following = false;
        self.cursor = vis_idx;
        self.match_stats_stale = true;
        JumpResult::Ok
    }

    /// Whether `row_id` is still present in either ring buffer.
    pub fn bookmark_alive(&self, row_id: u64) -> bool {
        self.row_alive(row_id)
    }

    /// Inclusive `[lo, hi]` range over `visible` indices while in visual-line
    /// mode; `None` when not selecting.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.visual_anchor?;
        if self.visible.is_empty() {
            return None;
        }
        let cur = self.cursor.min(self.visible.len() - 1);
        let anchor = anchor.min(self.visible.len() - 1);
        Some((anchor.min(cur), anchor.max(cur)))
    }

    pub fn enter_visual_line(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.pending_yank = false;
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_lock = false;
        self.pending_time = false;
        self.pending_m = false;
        self.pending_leader = false;
        self.following = false;
        self.visual_anchor = Some(self.cursor);
    }

    /// Arm `c` operator-pending (clear other pendings; stay on LogList).
    pub fn begin_chip_from_cursor(&mut self) {
        self.clear_visual();
        self.pending_yank = false;
        self.pending_d = false;
        self.pending_lock = false;
        self.pending_time = false;
        self.pending_exclude = false;
        self.pending_leader = false;
        self.pending_chip = true;
    }

    /// Arm `C` operator-pending for exclude-from-cursor (H9).
    pub fn begin_exclude_from_cursor(&mut self) {
        self.clear_visual();
        self.pending_yank = false;
        self.pending_d = false;
        self.pending_lock = false;
        self.pending_time = false;
        self.pending_chip = false;
        self.pending_leader = false;
        self.pending_exclude = true;
    }

    /// Cancel `c`/`C` pending / msg picker without touching `following`.
    pub fn cancel_chip_from_cursor(&mut self) {
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_leader = false;
        if self
            .picker
            .as_ref()
            .is_some_and(|p| matches!(p.kind, crate::picker::PickerKind::MsgChip { .. }))
        {
            self.close_picker();
        }
    }

    /// Arm `f` operator-pending (H8 session lock).
    pub fn begin_lock_from_cursor(&mut self) {
        self.clear_visual();
        self.pending_yank = false;
        self.pending_d = false;
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_time = false;
        self.pending_leader = false;
        self.pending_lock = true;
    }

    /// Cancel `f` pending without clearing lock or touching `following`.
    pub fn cancel_lock_pending(&mut self) {
        self.pending_lock = false;
        self.pending_leader = false;
    }

    /// Arm `t` operator-pending (global time window; file mode only).
    pub fn begin_time_op(&mut self) {
        self.clear_visual();
        self.pending_yank = false;
        self.pending_d = false;
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_lock = false;
        self.pending_m = false;
        self.pending_leader = false;
        self.pending_time = true;
    }

    /// Cancel `t` pending without touching the bound or `following`.
    pub fn cancel_time_pending(&mut self) {
        self.pending_time = false;
        self.pending_leader = false;
    }

    /// `ts`: open time panel from current rows / file sample. Returns false
    /// when refused (empty date candidates). Sets `following=false` on success.
    pub fn open_time_panel(&mut self) -> bool {
        self.pending_time = false;
        let catalog_rows: Vec<EntryRow> = match &self.store {
            RowStore::Stream(s) => s.rows.iter().cloned().collect(),
            RowStore::File(f) => {
                // Sample up to 4000 lines for date catalog (full scan is ok for
                // medium files; large files still get a useful date set).
                let n = f.line_count();
                let step = (n / 4000).max(1);
                let mut rows = Vec::new();
                let mut i = 0usize;
                while i < n {
                    if let Some(r) = f.row_at(i) {
                        rows.push(r);
                    }
                    i = i.saturating_add(step);
                }
                rows
            }
        };
        match TimePanel::open_from_iter(catalog_rows.iter(), self.time_bound.as_ref()) {
            Some(panel) => {
                self.following = false;
                self.time_panel = Some(panel);
                true
            }
            None => {
                self.set_flash("NO DATES");
                false
            }
        }
    }

    /// Close time panel without applying (Esc). Does not resume following.
    pub fn close_time_panel(&mut self) {
        self.time_panel = None;
        self.pending_time = false;
    }

    /// Apply a submitted bound from the panel.
    pub fn apply_time_bound(&mut self, bound: TimeBound) {
        self.time_panel = None;
        self.pending_time = false;
        self.following = false;
        self.time_bound = Some(bound);
        self.rebuild_visible();
        self.clear_flash();
    }

    /// `tu`: clear global time window and rebuild.
    pub fn clear_time_bound(&mut self) {
        self.pending_time = false;
        self.time_panel = None;
        self.following = false;
        let had = self.time_bound.as_ref().is_some_and(|t| t.is_active());
        self.time_bound = None;
        if had {
            self.rebuild_visible();
            self.set_flash("TIME CLEARED");
        } else {
            self.set_flash("NO TIME WINDOW");
        }
    }

    /// Status-bar label when a global time window is active.
    pub fn time_badge_label(&self) -> Option<String> {
        let bound = self.time_bound.as_ref()?;
        if !bound.is_active() {
            return None;
        }
        match (&bound.since, &bound.until) {
            (Some(s), Some(u)) => Some(format!("{s}…{u}")),
            (Some(s), None) => Some(format!("≥{s}")),
            (None, Some(u)) => Some(format!("≤{u}")),
            (None, None) => None,
        }
    }

    /// `f` `u`: clear session lock and rebuild.
    pub fn clear_session_lock(&mut self) {
        self.pending_lock = false;
        self.pending_leader = false;
        let had = self.lock_pid.is_some() || self.lock_tid.is_some();
        self.lock_pid = None;
        self.lock_tid = None;
        if had {
            self.rebuild_visible();
            self.set_flash("UNLOCK");
        } else {
            self.set_flash("NO LOCK");
        }
    }

    /// `f` `p` / `f` `t`: set, toggle-clear, or switch lock target.
    pub fn apply_session_lock(&mut self, kind: LockKind) {
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return;
        };
        let value = match kind {
            LockKind::Pid => row.pid.clone(),
            LockKind::Tid => row.tid.clone(),
        };
        if value.is_empty() {
            self.set_flash(match kind {
                LockKind::Pid => "EMPTY pid",
                LockKind::Tid => "EMPTY tid",
            });
            return;
        }
        let same = match kind {
            LockKind::Pid => self.lock_pid.as_deref() == Some(value.as_str()),
            LockKind::Tid => self.lock_tid.as_deref() == Some(value.as_str()),
        };
        if same {
            self.lock_pid = None;
            self.lock_tid = None;
            self.rebuild_visible();
            self.set_flash("UNLOCK");
            return;
        }
        match kind {
            LockKind::Pid => {
                self.lock_pid = Some(value);
                self.lock_tid = None;
            }
            LockKind::Tid => {
                self.lock_tid = Some(value);
                self.lock_pid = None;
            }
        }
        self.rebuild_visible();
        // Persistent LOCK badge is drawn from lock_pid/tid; clear any stale toast.
        self.clear_flash();
    }

    /// Status-bar label when a session lock is active.
    /// Short value for the lock icon (no "LOCK" word — glyph carries the meaning).
    pub fn lock_badge_label(&self) -> Option<String> {
        if let Some(pid) = &self.lock_pid {
            return Some(format!("pid={pid}"));
        }
        if let Some(tid) = &self.lock_tid {
            return Some(format!("tid={tid}"));
        }
        None
    }

    /// Push a single-field filter group from the current row (H7, non-msg path).
    /// Returns `true` when a new group was pushed.
    pub fn push_chip_from_field(&mut self, field: crate::input::ChipField) -> bool {
        use crate::input::{Chip, ChipField};
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return false;
        };
        let yank = match field {
            ChipField::Tag => YankField::Tag,
            ChipField::Msg => YankField::Msg,
            ChipField::Pkg => YankField::Pkg,
            ChipField::Pid => YankField::Pid,
            ChipField::Tid => YankField::Tid,
            ChipField::Level => YankField::Level,
        };
        let value = Self::field_text(&row, yank);
        if value.is_empty() {
            self.set_flash(format!("EMPTY {}", field.keyword()));
            return false;
        }
        self.push_single_chip_filter(Chip { field, value })
    }

    /// Open msg token picker for the current row (`c`/`C`+`m`).
    pub fn begin_msg_chip_picker(&mut self, as_exclude: bool) {
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return;
        };
        let tokens = crate::input::msg_token_candidates(&row.msg);
        if tokens.is_empty() {
            self.pending_chip = false;
            self.pending_exclude = false;
            self.pending_leader = false;
            self.set_flash("NO TOKENS");
        } else {
            self.pending_chip = false;
            self.pending_exclude = false;
            self.pending_leader = false;
            self.open_picker(crate::picker::PickerKind::MsgChip {
                exclude: as_exclude,
            });
            let picker = self.picker.as_mut().expect("picker just opened");
            picker.enter_new();
            picker.choices = tokens;
        }
    }

    /// Confirm msg picker selection / draft fallback → push msg include or exclude.
    pub fn confirm_msg_chip_picker(&mut self) -> bool {
        use crate::input::{Chip, ChipField};
        let Some((as_exclude, value)) = self.picker.as_ref().and_then(|picker| {
            let crate::picker::PickerKind::MsgChip { exclude } = picker.kind else {
                return None;
            };
            let visible = crate::picker::PickerSession::filtered_indices(
                &picker.choices,
                picker.draft.as_str(),
            );
            let value = visible
                .get(picker.selected)
                .and_then(|&index| picker.choices.get(index))
                .cloned()
                .or_else(|| (!picker.draft.is_empty()).then(|| picker.draft.to_string()))?;
            Some((exclude, value))
        }) else {
            self.set_flash("NO TOKENS");
            return false;
        };
        self.close_picker();
        let chip = Chip {
            field: ChipField::Msg,
            value,
        };
        if as_exclude {
            self.push_exclude_chip(chip)
        } else {
            self.push_single_chip_filter(chip)
        }
    }

    /// Push a single-field exclude from the current row (H9, non-msg path).
    pub fn push_exclude_from_field(&mut self, field: crate::input::ChipField) -> bool {
        use crate::input::{Chip, ChipField};
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return false;
        };
        let yank = match field {
            ChipField::Tag => YankField::Tag,
            ChipField::Msg => YankField::Msg,
            ChipField::Pkg => YankField::Pkg,
            ChipField::Pid => YankField::Pid,
            ChipField::Tid => YankField::Tid,
            ChipField::Level => YankField::Level,
        };
        let value = Self::field_text(&row, yank);
        if value.is_empty() {
            self.set_flash(format!("EMPTY {}", field.keyword()));
            return false;
        }
        self.push_exclude_chip(Chip { field, value })
    }

    pub fn push_exclude_chip(&mut self, chip: crate::input::Chip) -> bool {
        match self.groups.push_exclude(chip) {
            Ok(true) => {
                self.following = false;
                self.rebuild_visible();
                self.set_flash("EXCLUDE");
                true
            }
            Ok(false) => {
                self.set_flash("EXISTS");
                false
            }
            Err(e) => {
                self.set_flash(e);
                false
            }
        }
    }

    fn push_single_chip_filter(&mut self, chip: crate::input::Chip) -> bool {
        use crate::input::build_group_from_chips;
        let group = match build_group_from_chips(vec![chip], true) {
            Ok(Some(g)) => g,
            Ok(None) => return false,
            Err(e) => {
                self.set_flash(e);
                return false;
            }
        };
        if !self.push_filter_group(group) {
            self.set_flash("EXISTS");
            return false;
        }
        self.following = false;
        self.rebuild_visible();
        self.set_flash("FILTER");
        true
    }

    pub fn clear_visual(&mut self) {
        self.visual_anchor = None;
    }

    pub fn field_text(row: &EntryRow, field: YankField) -> String {
        match field {
            YankField::Raw => row.raw.clone(),
            YankField::Tag => row.tag.clone(),
            YankField::Msg => row.msg.clone(),
            YankField::Pid => row.pid.clone(),
            YankField::Tid => row.tid.clone(),
            YankField::Level => row.level.as_char().to_string(),
            YankField::Pkg => row.pkg.clone(),
            YankField::Timestamp => row.timestamp.clone(),
        }
    }

    pub fn yank_field(&self, field: YankField) -> Option<String> {
        self.current_row().map(|row| Self::field_text(&row, field))
    }

    /// Join `field` values for visible indices `lo..=hi` with newlines.
    pub fn yank_range(&self, lo: usize, hi: usize, field: YankField) -> Option<String> {
        if self.visible.is_empty() || lo > hi || hi >= self.visible.len() {
            return None;
        }
        let mut parts = Vec::with_capacity(hi - lo + 1);
        for vi in lo..=hi {
            let row = self.row_at(vi)?;
            parts.push(Self::field_text(&row, field));
        }
        Some(parts.join("\n"))
    }

    pub fn record_yank(&mut self, text: String) {
        self.last_yanked = Some(text);
    }

    /// Jump to the next (`dir > 0`) or previous (`dir < 0`) severe visible row
    /// (level E/F or crash). Wraps like vim `wrapscan`. Independent of search.
    ///
    /// File mode prefers the severe cache / prefetch; synchronous `row_at` is
    /// budget-capped so a keypress never full-scans a huge visible set.
    pub fn find_severe(&mut self, dir: i8) -> bool {
        let n = self.visible.len();
        if n == 0 {
            return false;
        }
        let step: isize = if dir >= 0 { 1 } else { -1 };
        let start = self.cursor as isize;
        let file_mode = self.store.is_file();
        let mut sync_parses = 0usize;
        for offset in 1..=n as isize {
            let idx = (start + offset * step).rem_euclid(n as isize) as usize;
            let is_severe = if file_mode {
                let Some(src) = self.source_idx_for_visible(idx) else {
                    continue;
                };
                match self.store.as_file().and_then(|f| f.severe_cached(src)) {
                    Some(v) => v,
                    None => {
                        if sync_parses >= SEVERE_SYNC_PARSE_BUDGET {
                            self.set_flash("SEVERE…");
                            return false;
                        }
                        sync_parses += 1;
                        self.row_at(idx).is_some_and(|r| r.severe)
                    }
                }
            } else {
                self.row_at(idx).is_some_and(|r| r.severe)
            };
            if is_severe {
                self.following = false;
                self.cursor = idx;
                self.match_stats_stale = true;
                return true;
            }
        }
        false
    }

    /// Jump to the next (`dir > 0`) or previous (`dir < 0`) visible row whose
    /// tag or msg matches the globally active search group. Wraps like vim `wrapscan`.
    pub fn find_match(&mut self, dir: i8) -> bool {
        let Some(active_idx) = self.active_highlight else {
            return false;
        };
        if !self
            .highlight_groups
            .groups
            .get(active_idx)
            .is_some_and(|g| g.enabled)
        {
            return false;
        }
        if self.store.is_file() {
            let Some(idx) = self.highlight_scan.find_next(self.cursor, dir) else {
                return false;
            };
            self.following = false;
            self.cursor = idx;
            self.match_stats_stale = true;
            return true;
        }
        let n = self.visible.len();
        if n == 0 {
            return false;
        }
        let step: isize = if dir >= 0 { 1 } else { -1 };
        let start = self.cursor as isize;
        for offset in 1..=n as isize {
            let idx = (start + offset * step).rem_euclid(n as isize) as usize;
            let Some(row) = self.row_at(idx) else {
                continue;
            };
            let hit = self.highlight_groups.groups[active_idx].matches_row(&row.tag, &row.msg);
            if hit {
                self.following = false;
                self.cursor = idx;
                self.match_stats_stale = true;
                return true;
            }
        }
        false
    }

    /// Jump to the first visible row matching search group `group_idx`.
    /// Used after committing a search (or re-submitting a duplicate).
    pub fn jump_first_match_of(&mut self, group_idx: usize) -> bool {
        let Some(group) = self.highlight_groups.groups.get(group_idx) else {
            return false;
        };
        if !group.enabled {
            return false;
        }
        if self.store.is_file() {
            if let Some(h) = self.highlight_scan.first_hit() {
                self.following = false;
                self.cursor = h;
                self.match_stats_stale = true;
                self.pending_jump_first = None;
                return true;
            }
            if self.highlight_scan.done {
                return false;
            }
            // Scan still running — jump when the first hit arrives.
            self.following = false;
            self.pending_jump_first = Some(group_idx);
            return true;
        }
        for idx in 0..self.visible.len() {
            let Some(row) = self.row_at(idx) else {
                continue;
            };
            if group.matches_row(&row.tag, &row.msg) {
                self.following = false;
                self.cursor = idx;
                self.match_stats_stale = true;
                return true;
            }
        }
        false
    }

    /// Jump to the first visible row matching the newest search group.
    pub fn jump_first_match(&mut self) -> bool {
        let Some(group_idx) = self.highlight_groups.groups.len().checked_sub(1) else {
            return false;
        };
        self.jump_first_match_of(group_idx)
    }

    /// Push a filter group unless an equivalent already exists. Returns whether pushed.
    pub fn push_filter_group(&mut self, group: crate::filter_model::Group) -> bool {
        if self.groups.groups.iter().any(|g| g.same_as(&group)) {
            return false;
        }
        self.groups.groups.push(group);
        true
    }

    pub fn update_filter_group(&mut self, index: usize, mut group: Group) -> bool {
        if index >= self.groups.groups.len() {
            return false;
        }
        if self
            .groups
            .groups
            .iter()
            .enumerate()
            .any(|(i, g)| i != index && g.same_as(&group))
        {
            return false;
        }
        group.enabled = self.groups.groups[index].enabled;
        self.groups.groups[index] = group;
        self.rebuild_visible();
        true
    }

    pub fn clear_filter_groups(&mut self) {
        self.groups.groups.clear();
        self.group_cursor = 0;
        self.rebuild_visible();
    }

    pub fn delete_filter_group_at(&mut self, index: usize) -> bool {
        if index >= self.groups.groups.len() {
            return false;
        }
        self.groups.groups.remove(index);
        if self.group_cursor >= self.groups.groups.len() {
            self.group_cursor = self.groups.groups.len().saturating_sub(1);
        }
        self.rebuild_visible();
        true
    }

    pub fn update_exclude_group(&mut self, index: usize, group: Group) -> bool {
        let Some(entry) = group_to_exclude_entry(group) else {
            return false;
        };
        if index >= self.groups.excludes.len() {
            return false;
        }
        if self
            .groups
            .excludes
            .iter()
            .enumerate()
            .any(|(i, e)| i != index && e.same_chip_as(&entry.chip))
        {
            return false;
        }
        let mut entry = entry;
        entry.enabled = self.groups.excludes[index].enabled;
        self.groups.excludes[index] = entry;
        self.rebuild_visible();
        true
    }

    pub fn clear_exclude_groups(&mut self) {
        self.groups.excludes.clear();
        self.exclude_cursor = 0;
        self.rebuild_visible();
    }

    pub fn delete_exclude_group_at(&mut self, index: usize) -> bool {
        if index >= self.groups.excludes.len() {
            return false;
        }
        self.groups.excludes.remove(index);
        if self.exclude_cursor >= self.groups.excludes.len() {
            self.exclude_cursor = self.groups.excludes.len().saturating_sub(1);
        }
        self.rebuild_visible();
        true
    }

    pub fn update_search_group(&mut self, index: usize, pattern: &str) -> bool {
        if index >= self.highlight_groups.groups.len() {
            return false;
        }
        let Some(mut group) = HighlightGroup::from_pattern(pattern) else {
            return false;
        };
        if self
            .highlight_groups
            .groups
            .iter()
            .enumerate()
            .any(|(i, g)| i != index && g.same_pattern_as(pattern))
        {
            return false;
        }
        group.enabled = self.highlight_groups.groups[index].enabled;
        self.highlight_groups.groups[index] = group;
        self.match_stats_stale = true;
        if self.store.is_file() && self.active_highlight == Some(index) {
            self.restart_highlight_scan();
        }
        true
    }

    pub fn clear_highlight_groups(&mut self) {
        self.highlight_groups.groups.clear();
        self.active_highlight = None;
        self.highlight_cursor = 0;
        self.match_stats_stale = true;
        if self.store.is_file() {
            self.restart_highlight_scan();
        }
    }

    pub fn delete_highlight_group_at(&mut self, index: usize) -> bool {
        if index >= self.highlight_groups.groups.len() {
            return false;
        }
        self.highlight_groups.groups.remove(index);
        if self.highlight_cursor >= self.highlight_groups.groups.len() {
            self.highlight_cursor = self.highlight_groups.groups.len().saturating_sub(1);
        }
        self.fix_active_highlight_after_delete(index);
        if self.store.is_file() {
            self.restart_highlight_scan();
        }
        true
    }

    pub fn update_bookmark_label(&mut self, row_id: u64, label: String) -> bool {
        self.bookmarks.update_label(row_id, label)
    }

    pub fn clear_bookmarks(&mut self) {
        self.bookmark_row_ids.clear();
        self.bookmarks.clear();
    }

    /// Delete a bookmark by index into `bookmarks.items`; keeps
    /// `bookmark_row_ids` in sync (F1). Returns false if index out of range.
    pub fn delete_bookmark_at_index(&mut self, index: usize) -> bool {
        let row_id = self.bookmarks.items.get(index).map(|b| b.row_id);
        if !self.bookmarks.delete_at(index) {
            return false;
        }
        if let Some(rid) = row_id {
            self.bookmark_row_ids.remove(&rid);
        }
        true
    }

    /// O(1) bookmark-row check for LogList bg (F1).
    pub fn is_bookmark_row(&self, row_id: u64) -> bool {
        self.bookmark_row_ids.contains(&row_id)
    }

    /// Toggle `enabled` on a unified Manage item. Returns whether state changed.
    pub fn toggle_unified_enabled(
        &mut self,
        kind: crate::picker::UnifiedKind,
        index: usize,
    ) -> bool {
        use crate::picker::UnifiedKind;
        match kind {
            UnifiedKind::Filter => {
                let Some(g) = self.groups.groups.get_mut(index) else {
                    return false;
                };
                g.enabled = !g.enabled;
                self.rebuild_visible();
                true
            }
            UnifiedKind::Highlight => {
                let Some(g) = self.highlight_groups.groups.get_mut(index) else {
                    return false;
                };
                g.enabled = !g.enabled;
                self.match_stats_stale = true;
                if self.store.is_file() {
                    self.restart_highlight_scan();
                }
                true
            }
            UnifiedKind::Exclude => {
                let Some(e) = self.groups.excludes.get_mut(index) else {
                    return false;
                };
                e.enabled = !e.enabled;
                self.rebuild_visible();
                true
            }
        }
    }

    /// Delete a unified Manage item by kind + source index.
    pub fn delete_unified_at(&mut self, kind: crate::picker::UnifiedKind, index: usize) -> bool {
        use crate::picker::UnifiedKind;
        match kind {
            UnifiedKind::Filter => self.delete_filter_group_at(index),
            UnifiedKind::Highlight => self.delete_highlight_group_at(index),
            UnifiedKind::Exclude => self.delete_exclude_group_at(index),
        }
    }

    /// Push a search group, or return the index of an existing equivalent.
    /// Always marks the returned index as the globally active search.
    /// Caller always jumps to that group's first match.
    pub fn push_or_find_highlight_group(
        &mut self,
        group: crate::highlight_model::HighlightGroup,
    ) -> usize {
        let idx = if let Some(idx) = self.highlight_groups.find_equivalent(&group.pattern) {
            idx
        } else {
            self.highlight_groups.groups.push(group);
            self.highlight_groups.groups.len() - 1
        };
        self.active_highlight = Some(idx);
        self.match_stats_stale = true;
        if self.store.is_file() {
            self.restart_highlight_scan();
        }
        idx
    }

    /// Search hit position among visible rows for the globally active group.
    /// Recomputes lazily when the stale flag is set OR the cursor moved since
    /// the last computation (handles direct `cursor` field writes in tests).
    pub fn highlight_match_stats(&mut self) -> Option<(Option<usize>, usize)> {
        if self.match_stats_stale || self.cursor != self.match_stats_cursor {
            self.match_stats_stale = false;
            self.match_stats_cursor = self.cursor;
            self.cached_match_stats = self.compute_match_stats_inner();
        }
        self.cached_match_stats
    }

    /// Eagerly recompute and cache highlight match stats if the stale flag is set.
    /// File mode reads the async hit index (O(log n)); Stream may O(visible) parse.
    pub fn recompute_match_stats_if_stale(&mut self) {
        if self.match_stats_stale || self.cursor != self.match_stats_cursor {
            self.match_stats_stale = false;
            self.match_stats_cursor = self.cursor;
            self.cached_match_stats = self.compute_match_stats_inner();
        }
    }

    fn compute_match_stats_inner(&self) -> Option<(Option<usize>, usize)> {
        let Some(_group) = self.active_highlight_group() else {
            return None;
        };
        if self.store.is_file() {
            return Some(self.highlight_scan.match_stats(self.cursor));
        }
        let group = self.active_highlight_group()?;
        let mut total = 0usize;
        let mut current = None;
        for idx in 0..self.visible.len() {
            let Some(row) = self.row_at(idx) else {
                continue;
            };
            if group.matches_row(&row.tag, &row.msg) {
                total += 1;
                if idx == self.cursor {
                    current = Some(total);
                }
            }
        }
        Some((current, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter_model::Group;
    use aloggrep::expr::Expr;
    use std::sync::mpsc;

    fn row(tag: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {tag}   : m")).unwrap()
    }

    fn filter_group(label: &str, expr: Option<Expr>) -> Group {
        Group {
            label: label.into(),
            chips: Vec::new(),
            expr,
            enabled: true,
        }
    }

    #[test]
    fn test_drain_appends_visible_rows() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn test_new_app_has_zero_list_offset() {
        let app = App::new(100);
        assert_eq!(app.list_offset, 0);
    }

    #[test]
    fn test_time_bound_alone_activates_filter_and_matches() {
        let mut app = App::new(100);
        assert!(!app.filter_active());
        app.time_bound = Some(TimeBound {
            since: Some("10:00:00".into()),
            until: Some("10:00:00".into()),
        });
        assert!(app.filter_active());
        let keep = row("A");
        let late = EntryRow::from_line("04-02 11:00:00.000  1  1 I B   : m").unwrap();
        assert!(app.row_passes_filters(&keep));
        assert!(!app.row_passes_filters(&late));
        let (tx, rx) = mpsc::channel();
        tx.send(keep).unwrap();
        tx.send(late).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.view_source()[app.source_idx_for_visible(0).unwrap()].tag,
            "A"
        );
        assert_eq!(app.time_badge_label().as_deref(), Some("10:00:00…10:00:00"));
        assert!(app.export_cli_command().contains("--since '10:00:00'"));
    }

    #[test]
    fn test_clear_time_bound_restores_visibility() {
        let mut app = App::new(100);
        app.time_bound = Some(TimeBound {
            since: Some("11:00:00".into()),
            until: None,
        });
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert!(app.visible.is_empty());
        app.clear_time_bound();
        assert!(app.time_bound.is_none());
        assert!(!app.filter_active());
        assert_eq!(app.visible.len(), 1);
    }

    #[test]
    fn test_time_badge_label() {
        let mut app = App::new(100);
        assert!(app.time_badge_label().is_none());
        app.time_bound = Some(TimeBound {
            since: Some("04-02 10:00:00".into()),
            until: None,
        });
        assert_eq!(app.time_badge_label().as_deref(), Some("≥04-02 10:00:00"));
    }

    #[test]
    fn test_di_group_zero_does_not_clear_time_bound() {
        use crate::input::{Chip, ChipField};
        use aloggrep::expr::Expr;

        let mut app = App::new(100);
        app.time_bound = Some(TimeBound {
            since: Some("10:30:00".into()),
            until: None,
        });
        app.groups.groups.push(Group {
            label: "tag:A".into(),
            chips: vec![Chip {
                field: ChipField::Tag,
                value: "A".into(),
            }],
            expr: Some(Expr::parse("tag~A", true).unwrap()),
            enabled: true,
        });
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap(); // 10:00:00 — excluded by since
        tx.send(EntryRow::from_line("04-02 11:00:00.000  1  1 I A   : late").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.view_source()[app.source_idx_for_visible(0).unwrap()]
                .as_log_entry()
                .time_hms(),
            Some("11:00:00")
        );

        app.groups.groups[0].enabled = false;
        app.rebuild_visible();
        // Group disabled → include vacuous, but global time window still AND.
        assert!(app.time_bound.is_some());
        assert!(app.filter_active());
        assert_eq!(app.visible.len(), 1);
        assert_eq!(
            app.view_source()[app.source_idx_for_visible(0).unwrap()]
                .as_log_entry()
                .time_hms(),
            Some("11:00:00")
        );
    }

    #[test]
    fn test_ring_buffer_evicts_oldest_and_shifts_indices() {
        let mut app = App::new(2);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        tx.send(row("C")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.rows().len(), 2);
        assert_eq!(app.rows()[0].tag, "B");
        assert_eq!(app.rows()[1].tag, "C");
        assert_eq!(app.visible, Visible::All { len: 2 });
    }

    #[test]
    fn test_visible_all_identity_under_filter_active() {
        let mut app = App::new(100);
        app.groups = GroupList {
            groups: vec![filter_group(
                "keep-A",
                Some(Expr::parse("tag~A", false).unwrap()),
            )],
            excludes: Vec::new(),
        };
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        tx.send(row("A")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.matched().len(), 2);
        assert_eq!(app.visible, Visible::All { len: 2 });
        assert_eq!(app.source_idx_for_visible(0), Some(0));
        assert_eq!(app.source_idx_for_visible(1), Some(1));
        assert_eq!(app.source_idx_for_visible(2), None);
    }

    #[test]
    fn test_drain_ring_applies_push_row() {
        use crate::ingest::DropOldestRing;
        let mut app = App::new(100);
        let ring = DropOldestRing::new(8);
        ring.push(row("A"));
        ring.push(row("B"));
        ring.mark_disconnected();
        app.drain(&ring);
        assert_eq!(app.visible_len(), 2);
        assert!(app.ingest_done);
        assert_eq!(app.rows()[0].tag, "A");
        assert_eq!(app.rows()[1].tag, "B");
    }

    #[test]
    fn test_drain_budget_leaves_remainder_and_defers_ingest_done() {
        use crate::ingest::DropOldestRing;
        let mut app = App::new(DRAIN_BUDGET_PER_FRAME + 8);
        let ring = DropOldestRing::new(DRAIN_BUDGET_PER_FRAME + 4);
        for i in 0..(DRAIN_BUDGET_PER_FRAME + 3) {
            ring.push(row(&format!("T{i}")));
        }
        ring.mark_disconnected();
        app.drain(&ring);
        assert_eq!(app.rows().len(), DRAIN_BUDGET_PER_FRAME);
        assert!(
            !app.ingest_done,
            "budget stop must not mark done while rows remain"
        );
        assert_eq!(ring.len(), 3);
        app.drain(&ring);
        assert_eq!(app.rows().len(), DRAIN_BUDGET_PER_FRAME + 3);
        assert!(app.ingest_done);
        assert!(ring.is_empty());
    }

    #[test]
    fn test_inactive_front_evict_decrements_list_offset_o1() {
        let mut app = App::new(2);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.list_offset = 1;
        app.cursor = 1;
        assert_eq!(app.visible, Visible::All { len: 2 });

        let (tx2, rx2) = mpsc::channel();
        tx2.send(row("C")).unwrap(); // rows at cap → pop A, O(1) adjust
        drop(tx2);
        app.drain(&rx2);

        assert_eq!(app.rows()[0].tag, "B");
        assert_eq!(app.rows()[1].tag, "C");
        assert_eq!(app.visible, Visible::All { len: 2 });
        assert_eq!(app.list_offset, 0);
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_evicting_visible_front_decrements_list_offset() {
        // With the matched buffer, a filter-matching row is NOT evicted by
        // `rows` overflow — only by reaching `matched_cap`. Simulate that:
        // two matching rows fill `matched`; a third match evicts the oldest.
        let mut app = App::new(100);
        app.matched_cap = 2;
        app.groups = GroupList {
            groups: vec![filter_group(
                "keep-A",
                Some(Expr::parse("tag~A", false).unwrap()),
            )],
            excludes: Vec::new(),
        };
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap(); // matched=[A0], visible=[0]
        tx.send(row("A")).unwrap(); // matched=[A0,A1], visible=[0,1]
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible.len(), 2);
        app.following = false;
        app.list_offset = 1;
        app.cursor = 1;

        let (tx2, rx2) = mpsc::channel();
        tx2.send(row("A")).unwrap(); // matched at cap → evict A0 → visible front drops
        drop(tx2);
        app.drain(&rx2);

        assert_eq!(app.visible.len(), 2);
        assert_eq!(
            app.list_offset, 0,
            "viewport must shift with front eviction"
        );
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn test_move_cursor_clamps() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor(-5);
        assert_eq!(app.cursor, 0);
        app.move_cursor(5);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn test_move_cursor_manual_large_delta_clamps_like_paging() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-10); // simulates Ctrl-u paging past the top
        assert_eq!(app.cursor, 0);
        assert!(!app.following, "negative delta should pause following");
        app.move_cursor_manual(10); // simulates Ctrl-d paging past the bottom
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn clear_buffered_logs_drops_buffers_keeps_filters_and_resumes() {
        use crate::bookmark::Bookmark;
        use crate::highlight_model::HighlightGroup;

        let mut app = App::new(100);
        app.groups = GroupList {
            groups: vec![filter_group(
                "keep-A",
                Some(Expr::parse("tag~A", false).unwrap()),
            )],
            excludes: Vec::new(),
        };
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("err").unwrap());
        app.lock_pid = Some("1".into());

        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert!(!app.rows().is_empty());
        assert!(!app.matched().is_empty());
        assert!(!app.visible.is_empty());

        let row_id = app.matched()[0].row_id;
        app.bookmarks
            .try_add(Bookmark {
                row_id,
                label: "bm".into(),
            })
            .unwrap();
        app.bookmark_row_ids.insert(row_id);

        app.following = false;
        app.cursor = 0;
        app.list_offset = 0;
        app.pending_yank = true;
        app.pending_chip = true;
        app.visual_anchor = Some(0);

        app.clear_buffered_logs();

        assert!(app.rows().is_empty());
        assert!(app.matched().is_empty());
        assert!(app.visible.is_empty());
        assert!(app.bookmarks.is_empty());
        assert!(app.bookmark_row_ids.is_empty());
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.highlight_groups.groups.len(), 1);
        assert_eq!(app.lock_pid.as_deref(), Some("1"));
        assert!(app.following);
        assert_eq!(app.cursor, 0);
        assert_eq!(app.list_offset, 0);
        assert!(!app.pending_yank);
        assert!(!app.pending_chip);
        assert!(app.visual_anchor.is_none());
        assert_eq!(app.status_msg.as_deref(), Some("CLEARED"));
    }

    #[test]
    fn test_cursor_unaffected_when_evicted_row_was_already_filtered_out() {
        let mut app = App::new(3);
        app.groups = GroupList {
            groups: vec![filter_group(
                "x",
                Some(Expr::parse("tag~X", false).unwrap()),
            )],
            excludes: Vec::new(),
        };
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(row("N1")).unwrap(); // filtered out, not in `matched`/`visible`
        tx.send(row("X1")).unwrap();
        tx.send(row("X2")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible, Visible::All { len: 2 });
        app.cursor = 1; // pointing at X2
        app.following = false;
        let selected_tag_before = app.current_row().unwrap().tag.clone();

        let (tx2, rx2) = std::sync::mpsc::channel();
        tx2.send(row("X3")).unwrap(); // triggers `rows` eviction of N1 (non-matching)
        drop(tx2);
        app.drain(&rx2);

        let selected_tag_after = app.current_row().unwrap().tag.clone();
        assert_eq!(
            selected_tag_before, selected_tag_after,
            "cursor should still point at the same logical row"
        );
        assert_eq!(selected_tag_after, "X2");
    }

    #[test]
    fn test_matched_rows_survive_rows_overflow() {
        // The core fix: with a filter active, matching rows are retained in
        // `matched` even after `rows` rolls over. Non-matching churn must not
        // wash out previously matched rows.
        let mut app = App::new(2);
        app.groups = GroupList {
            groups: vec![filter_group(
                "keep-A",
                Some(Expr::parse("tag~A", false).unwrap()),
            )],
            excludes: Vec::new(),
        };
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap(); // A0: matched=[A0], rows=[A0]
        tx.send(row("X")).unwrap(); // rows=[A0,X], matched=[A0]
        tx.send(row("Y")).unwrap(); // rows rolls: [X,Y], A0 evicted from rows
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.rows().len(), 2);
        assert_eq!(app.rows()[0].tag, "X");
        assert_eq!(app.rows()[1].tag, "Y");
        assert_eq!(app.matched().len(), 1);
        assert_eq!(app.matched()[0].tag, "A");
        assert_eq!(app.visible, Visible::All { len: 1 });
        assert_eq!(app.current_row().unwrap().tag, "A");
    }

    #[test]
    fn test_rebuild_visible_after_filter_change_loses_matched_evicted_from_rows() {
        // Rebuild re-scans current `rows`; rows already evicted from `rows`
        // (even if previously retained in `matched`) are unrecoverable.
        let mut app = App::new(2);
        // Start with NO filter: everything goes to `rows`, `matched` stays empty.
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        tx.send(row("A")).unwrap(); // rows rolls: [B,A]
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.rows()[0].tag, "B");
        assert_eq!(app.rows()[1].tag, "A");
        // Now activate a filter matching `A`; rebuild scans only current rows
        // ([B,A]) — the first `A` (already evicted from `rows`) is gone.
        app.groups = GroupList {
            groups: vec![filter_group(
                "keep-A",
                Some(Expr::parse("tag~A", false).unwrap()),
            )],
            excludes: Vec::new(),
        };
        app.rebuild_visible();
        assert_eq!(app.matched().len(), 1);
        assert_eq!(app.matched()[0].tag, "A");
        assert_eq!(app.visible, Visible::All { len: 1 });
    }
}

#[cfg(test)]
mod focus_tests {
    use super::*;
    use crate::filter_model::Group;
    use aloggrep::expr::Expr;

    fn g(label: &str) -> Group {
        Group {
            label: label.into(),
            chips: Vec::new(),
            expr: None,
            enabled: true,
        }
    }

    #[test]
    fn test_cycle_focus_forward_wraps() {
        let mut app = App::new(100);
        assert_eq!(app.focus, Focus::LogList);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::Input);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::ChipStrip);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::ExcludeStrip);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::HighlightStrip);
        app.cycle_focus_forward();
        assert_eq!(app.focus, Focus::LogList);
    }

    #[test]
    fn test_delete_focused_group_removes_and_rescans() {
        let mut app = App::new(100);
        app.groups.groups.push(g("g0"));
        app.groups.groups.push(g("g1"));
        app.group_cursor = 0;
        app.delete_focused_group();
        assert_eq!(app.groups.groups.len(), 1);
        assert_eq!(app.groups.groups[0].label, "g1");
    }

    #[test]
    fn test_delete_focused_group_returns_focus_to_loglist_when_list_becomes_empty() {
        let mut app = App::new(100);
        app.groups.groups.push(g("g0"));
        app.focus = Focus::ChipStrip;
        app.delete_focused_group();
        assert!(app.groups.groups.is_empty());
        assert_eq!(app.focus, Focus::LogList);
    }

    #[test]
    fn test_delete_focused_group_keeps_focus_when_groups_remain() {
        let mut app = App::new(100);
        app.groups.groups.push(g("g0"));
        app.groups.groups.push(g("g1"));
        app.focus = Focus::ChipStrip;
        app.delete_focused_group();
        assert!(!app.groups.groups.is_empty());
        assert_eq!(
            app.focus,
            Focus::ChipStrip,
            "focus should stay put while groups remain"
        );
    }

    #[test]
    fn test_move_group_cursor_clamps() {
        let mut app = App::new(100);
        app.groups.groups.push(g("g0"));
        app.move_group_cursor(-5);
        assert_eq!(app.group_cursor, 0);
        app.move_group_cursor(5);
        assert_eq!(app.group_cursor, 0);
    }

    #[test]
    fn test_toggle_disable_filter_rebuilds_visible() {
        let mut app = App::new(100);
        app.groups.groups.push(Group {
            label: "a".into(),
            chips: Vec::new(),
            expr: Some(Expr::parse("tag~A", false).unwrap()),
            enabled: true,
        });
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I A   : m").unwrap())
            .unwrap();
        tx.send(EntryRow::from_line("04-02 10:00:00.000  1  1 I B   : m").unwrap())
            .unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible.len(), 1);
        app.toggle_disable_focused(StripKind::Filter);
        assert!(!app.groups.groups[0].enabled);
        assert_eq!(app.visible.len(), 2, "disabled-only list ≡ empty filter");
    }
}

#[cfg(test)]
mod follow_tests {
    use super::*;
    use crate::filter_model::Group;
    use aloggrep::expr::Expr;
    use std::sync::mpsc;

    fn row(tag: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {tag}   : m")).unwrap()
    }

    #[test]
    fn test_follow_pins_cursor_to_latest() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.cursor, 1); // pinned to last row
    }

    #[test]
    fn test_manual_up_navigation_pauses_follow() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);

        app.move_cursor_manual(-1);
        assert!(!app.following);

        let (tx2, rx2) = mpsc::channel();
        tx2.send(row("C")).unwrap();
        drop(tx2);
        app.drain(&rx2);
        assert_eq!(app.cursor, 0); // did not jump to the new bottom
    }

    #[test]
    fn test_resume_following_pins_bottom() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.move_cursor_manual(-1);
        assert!(!app.following);
        app.resume_following();
        assert!(app.following);
        assert_eq!(app.cursor, 1);
    }

    #[test]
    fn test_manual_down_also_pauses_follow() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert!(app.following);
        app.move_cursor_manual(0); // still counts as manual
                                   // delta 0 doesn't move but we always clear following in move_cursor_manual
        app.following = true;
        app.move_cursor_manual(1);
        assert!(!app.following);
    }

    #[test]
    fn test_rebuild_visible_follows_when_following_and_visible_set_grows() {
        let mut app = App::new(100);
        app.groups = GroupList {
            groups: vec![Group {
                label: "a".into(),
                chips: Vec::new(),
                expr: Some(Expr::parse("tag~A", false).unwrap()),
                enabled: true,
            }],
            excludes: Vec::new(),
        };
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap(); // doesn't match "a" group, filtered out
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible, Visible::All { len: 1 }); // only A is visible
        assert!(app.following);

        app.groups.groups.clear(); // simulates deleting the last filter group -> empty GroupList matches everything
        app.rebuild_visible();
        assert_eq!(app.visible, Visible::All { len: 2 }); // both now visible, set grew
        assert_eq!(app.cursor, 1); // still following: cursor pinned to new bottom (B), not stuck at old position
    }
}

#[cfg(test)]
mod highlight_tests {
    use super::*;
    use crate::highlight_model::HighlightGroup;
    use std::sync::mpsc;

    fn row_with_msg(tag: &str, msg: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1234  5678 I {tag}   : {msg}")).unwrap()
    }

    #[test]
    fn test_find_match_next_prev_and_wrap() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "aaa")).unwrap();
        tx.send(row_with_msg("T", "hit one")).unwrap();
        tx.send(row_with_msg("T", "bbb")).unwrap();
        tx.send(row_with_msg("T", "hit two")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());

        assert!(app.find_match(1));
        assert_eq!(app.cursor, 1);
        assert!(app.find_match(1));
        assert_eq!(app.cursor, 3);
        assert!(app.find_match(1)); // wrap
        assert_eq!(app.cursor, 1);
        assert!(app.find_match(-1)); // wrap backward to last
        assert_eq!(app.cursor, 3);
    }

    #[test]
    fn test_find_match_noop_without_search() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "x")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert!(!app.find_match(1));
    }

    #[test]
    fn test_jump_first_match_and_highlight_match_stats() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "aaa")).unwrap();
        tx.send(row_with_msg("T", "hit one")).unwrap();
        tx.send(row_with_msg("T", "bbb")).unwrap();
        tx.send(row_with_msg("T", "hit two")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = true;
        app.cursor = 3;
        assert!(app.highlight_match_stats().is_none());

        app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
        app.cursor = 0; // non-hit row
        assert_eq!(app.highlight_match_stats(), Some((None, 2)));

        assert!(app.jump_first_match());
        assert_eq!(app.cursor, 1);
        assert!(!app.following);
        assert_eq!(app.highlight_match_stats(), Some((Some(1), 2)));

        app.cursor = 3;
        assert_eq!(app.highlight_match_stats(), Some((Some(2), 2)));

        app.cursor = 2;
        assert_eq!(app.highlight_match_stats(), Some((None, 2)));
    }

    #[test]
    fn test_jump_first_match_noop_when_no_hits() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "aaa")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 0;
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("zzz").unwrap());
        assert!(!app.jump_first_match());
        assert_eq!(app.cursor, 0);
        assert_eq!(app.highlight_match_stats(), Some((None, 0)));
    }

    #[test]
    fn test_jump_first_match_targets_newest_group_only() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "foo early")).unwrap();
        tx.send(row_with_msg("T", "bar later")).unwrap();
        tx.send(row_with_msg("T", "foo late")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;

        app.push_or_find_highlight_group(HighlightGroup::from_pattern("foo").unwrap());
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("bar").unwrap());
        assert_eq!(app.active_highlight, Some(1));

        assert!(app.jump_first_match());
        // Must land on newest group ("bar"), not the earlier "foo" at index 0.
        assert_eq!(app.cursor, 1);
        assert_eq!(app.current_row().unwrap().msg, "bar later");
    }

    #[test]
    fn test_find_match_ignore_case_by_default() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "an error occurred")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("ERROR").unwrap());
        assert!(app.highlight_groups.any_match("", "an error occurred"));
    }

    #[test]
    fn test_find_match_hits_tag_only() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("Other", "aaa")).unwrap();
        tx.send(row_with_msg("MyTag", "bbb")).unwrap();
        tx.send(row_with_msg("Other", "ccc")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("MyTag").unwrap());
        assert_eq!(app.highlight_match_stats(), Some((None, 1)));
        assert!(app.jump_first_match());
        assert_eq!(app.cursor, 1);
        assert_eq!(app.current_row().unwrap().tag, "MyTag");
        assert!(app.find_match(1));
        assert_eq!(app.cursor, 1, "only one tag hit; wrap lands on same row");
    }

    #[test]
    fn test_disabled_search_group_excluded_from_find() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "hit")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
        app.highlight_groups.groups[0].enabled = false;
        assert!(!app.find_match(1));
        assert!(app.highlight_match_stats().is_none());
    }

    #[test]
    fn test_push_or_find_highlight_group_dedups() {
        let mut app = App::new(100);
        let idx0 = app.push_or_find_highlight_group(HighlightGroup::from_pattern("foo").unwrap());
        assert_eq!(idx0, 0);
        assert_eq!(app.active_highlight, Some(0));
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("bar").unwrap());
        assert_eq!(app.active_highlight, Some(1));
        let idx1 = app.push_or_find_highlight_group(HighlightGroup::from_pattern("FOO").unwrap());
        assert_eq!(idx1, 0);
        assert_eq!(app.active_highlight, Some(0));
        assert_eq!(app.highlight_groups.groups.len(), 2);
    }

    #[test]
    fn test_find_match_only_uses_active_highlight_group() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("T", "foo early")).unwrap();
        tx.send(row_with_msg("T", "bar mid")).unwrap();
        tx.send(row_with_msg("T", "foo late")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("foo").unwrap());
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("bar").unwrap());
        // active is "bar" — n must not land on "foo"
        assert!(app.find_match(1));
        assert_eq!(app.current_row().unwrap().msg, "bar mid");
        assert!(app.find_match(1)); // wrap to same hit
        assert_eq!(app.current_row().unwrap().msg, "bar mid");
    }

    #[test]
    fn test_delete_active_highlight_falls_back_to_newest() {
        let mut app = App::new(100);
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("a").unwrap());
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("b").unwrap());
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("c").unwrap());
        assert_eq!(app.active_highlight, Some(2));
        app.highlight_cursor = 2;
        app.focus = Focus::HighlightStrip;
        app.delete_focused_strip_group(StripKind::Highlight);
        assert_eq!(app.highlight_groups.groups.len(), 2);
        assert_eq!(app.active_highlight, Some(1)); // newest remaining ("b")
        app.highlight_cursor = 0;
        app.delete_focused_strip_group(StripKind::Highlight); // remove "a", active was 1 -> shifts to 0
        assert_eq!(app.active_highlight, Some(0));
        assert_eq!(app.highlight_groups.groups[0].pattern, "b");
    }
}

#[cfg(test)]
mod yank_and_search_tests {
    use super::*;
    use std::sync::mpsc;

    fn row_with_msg(tag: &str, msg: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1234  5678 I {tag}   : {msg}")).unwrap()
    }

    #[test]
    fn test_yank_field_extracts_tag_and_msg() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_with_msg("MyTag", "hello")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.yank_field(YankField::Tag).as_deref(), Some("MyTag"));
        assert_eq!(app.yank_field(YankField::Msg).as_deref(), Some("hello"));
        assert_eq!(app.yank_field(YankField::Pid).as_deref(), Some("1234"));
        assert_eq!(app.yank_field(YankField::Tid).as_deref(), Some("5678"));
        assert_eq!(app.yank_field(YankField::Level).as_deref(), Some("I"));
        assert_eq!(
            app.yank_field(YankField::Timestamp).as_deref(),
            Some("04-02 10:00:00.000")
        );
    }

    #[test]
    fn test_yank_range_joins_raw_with_newlines() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        let a = row_with_msg("A", "one");
        let b = row_with_msg("B", "two");
        let raw_a = a.raw.clone();
        let raw_b = b.raw.clone();
        tx.send(a).unwrap();
        tx.send(b).unwrap();
        drop(tx);
        app.drain(&rx);
        let text = app.yank_range(0, 1, YankField::Raw).unwrap();
        assert_eq!(text, format!("{raw_a}\n{raw_b}"));
    }

    #[test]
    fn test_selection_range_orders_anchor_and_cursor() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        for i in 0..5 {
            tx.send(row_with_msg("T", &format!("m{i}"))).unwrap();
        }
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 3;
        app.visual_anchor = Some(1);
        assert_eq!(app.selection_range(), Some((1, 3)));
        app.cursor = 0;
        assert_eq!(app.selection_range(), Some((0, 1)));
    }

    #[test]
    fn test_yank_field_from_char_mapping() {
        assert_eq!(YankField::from_char('y'), Some(YankField::Raw));
        assert_eq!(YankField::from_char('t'), Some(YankField::Tag));
        assert_eq!(YankField::from_char('m'), Some(YankField::Msg));
        assert_eq!(YankField::from_char('T'), Some(YankField::Tid));
        assert_eq!(YankField::from_char('x'), None);
    }
}

#[cfg(test)]
mod flash_tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn set_flash_stores_msg_and_deadline() {
        let mut app = App::new(100);
        app.set_flash("YANKED");
        assert_eq!(app.status_msg.as_deref(), Some("YANKED"));
        assert!(app.status_flash_until.is_some());
    }

    #[test]
    fn tick_flash_clears_expired() {
        let mut app = App::new(100);
        app.set_flash("NO ERROR");
        app.status_flash_until = Some(Instant::now() - Duration::from_millis(1));
        app.tick_flash();
        assert!(app.status_msg.is_none());
        assert!(app.status_flash_until.is_none());
    }

    #[test]
    fn tick_flash_keeps_unexpired() {
        let mut app = App::new(100);
        app.set_flash("FILTER");
        app.tick_flash();
        assert_eq!(app.status_msg.as_deref(), Some("FILTER"));
    }

    #[test]
    fn cancel_pending_does_not_clear_flash() {
        let mut app = App::new(100);
        app.set_flash("YANKED");
        app.begin_bookmark_op();
        app.cancel_bookmark_op();
        assert_eq!(app.status_msg.as_deref(), Some("YANKED"));
        assert!(!app.pending_m);
    }

    fn sample_tag_group(tag: &str) -> Group {
        use crate::input::{build_group_from_chips, Chip, ChipField};
        build_group_from_chips(
            vec![Chip {
                field: ChipField::Tag,
                value: tag.to_string(),
            }],
            true,
        )
        .unwrap()
        .unwrap()
    }

    #[test]
    fn update_and_clear_highlight_groups() {
        let mut app = App::new(100);
        let g = HighlightGroup::from_pattern("foo").unwrap();
        app.push_or_find_highlight_group(g);
        assert!(app.update_search_group(0, "bar"));
        assert!(app.highlight_groups.groups[0].same_pattern_as("bar"));
        app.clear_highlight_groups();
        assert!(app.highlight_groups.groups.is_empty());
        assert!(app.active_highlight.is_none());
    }

    #[test]
    fn update_search_group_dedups_other_indices() {
        let mut app = App::new(100);
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("foo").unwrap());
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("bar").unwrap());
        assert!(!app.update_search_group(0, "BAR"));
        assert!(app.highlight_groups.groups[0].same_pattern_as("foo"));
    }

    #[test]
    fn update_and_clear_filter_groups() {
        let mut app = App::new(100);
        assert!(app.push_filter_group(sample_tag_group("A")));
        let g2 = sample_tag_group("B");
        assert!(app.update_filter_group(0, g2));
        assert!(app.groups.groups[0].same_as(&sample_tag_group("B")));
        app.clear_filter_groups();
        assert!(app.groups.groups.is_empty());
    }

    #[test]
    fn delete_filter_group_at_out_of_bounds() {
        let mut app = App::new(100);
        assert!(!app.delete_filter_group_at(0));
    }

    #[test]
    fn clear_bookmarks() {
        let mut app = App::new(100);
        app.bookmarks
            .try_add(Bookmark {
                row_id: 1,
                label: "test".into(),
            })
            .unwrap();
        app.clear_bookmarks();
        assert!(app.bookmarks.is_empty());
    }

    #[test]
    fn update_bookmark_label_by_row_id() {
        let mut app = App::new(100);
        app.bookmarks
            .try_add(Bookmark {
                row_id: 42,
                label: "old".into(),
            })
            .unwrap();
        assert!(app.update_bookmark_label(42, "new".into()));
        assert_eq!(app.bookmarks.items[0].label, "new");
        assert!(!app.update_bookmark_label(99, "x".into()));
    }

    #[test]
    fn delete_highlight_group_at_fixes_active_highlight() {
        let mut app = App::new(100);
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("a").unwrap());
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("b").unwrap());
        assert_eq!(app.active_highlight, Some(1));
        assert!(app.delete_highlight_group_at(1));
        assert_eq!(app.active_highlight, Some(0));
        assert!(app.delete_highlight_group_at(0));
        assert!(app.active_highlight.is_none());
    }
}

#[cfg(test)]
mod severe_tests {
    use super::*;
    use crate::filter_model::Group;
    use aloggrep::expr::{Expr, SameFieldOp};
    use std::sync::mpsc;

    fn row_level(level: char, tag: &str, msg: &str) -> EntryRow {
        EntryRow::from_line(&format!(
            "04-02 10:00:00.000  1234  5678 {level} {tag}   : {msg}"
        ))
        .unwrap()
    }

    fn filter_group(label: &str, expr: Option<Expr>) -> Group {
        Group {
            label: label.into(),
            chips: Vec::new(),
            expr,
            enabled: true,
        }
    }

    #[test]
    fn test_find_severe_next_prev_and_wrap() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_level('I', "T", "ok")).unwrap();
        tx.send(row_level('E', "T", "err one")).unwrap();
        tx.send(row_level('I', "T", "ok2")).unwrap();
        tx.send(row_level('F', "T", "fatal")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;

        assert!(app.find_severe(1));
        assert_eq!(app.cursor, 1);
        assert!(!app.following);
        assert!(app.find_severe(1));
        assert_eq!(app.cursor, 3);
        assert!(app.find_severe(1)); // wrap
        assert_eq!(app.cursor, 1);
        assert!(app.find_severe(-1)); // wrap backward to last
        assert_eq!(app.cursor, 3);
    }

    #[test]
    fn test_find_severe_noop_when_none() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_level('I', "T", "ok")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = true;
        app.cursor = 0;
        assert!(!app.find_severe(1));
        assert_eq!(app.cursor, 0);
        assert!(app.following, "no jump must not clear following");
    }

    #[test]
    fn test_find_severe_respects_visible_filter() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_level('E', "Keep", "err keep")).unwrap();
        tx.send(row_level('E', "Drop", "err drop")).unwrap();
        tx.send(row_level('I', "Keep", "ok")).unwrap();
        drop(tx);
        app.drain(&rx);

        let expr = Expr::from_filters(
            &[String::from("Keep")],
            &[],
            &[],
            &[],
            &[],
            None,
            true,
            SameFieldOp::And,
        )
        .unwrap()
        .unwrap();
        assert!(app.push_filter_group(filter_group("tag=Keep", Some(expr))));
        app.rebuild_visible();
        assert_eq!(app.visible.len(), 2);
        app.following = false;
        app.cursor = 0; // on Keep E

        assert!(app.find_severe(1));
        // Only one severe in visible; wrap lands on same Keep E (index 0), not Drop.
        assert_eq!(app.cursor, 0);
        assert_eq!(app.current_row().unwrap().tag, "Keep");
    }

    #[test]
    fn test_find_severe_hits_crash_message_even_if_info_level() {
        let mut app = App::new(100);
        let (tx, rx) = mpsc::channel();
        tx.send(row_level('I', "T", "normal")).unwrap();
        tx.send(row_level('I', "AndroidRuntime", "FATAL EXCEPTION: main"))
            .unwrap();
        tx.send(row_level('I', "T", "after")).unwrap();
        drop(tx);
        app.drain(&rx);
        app.following = false;
        app.cursor = 0;
        assert!(is_severe_row(
            &app.view_source()[app.source_idx_for_visible(1).unwrap()]
        ));
        assert!(app.find_severe(1));
        assert_eq!(app.cursor, 1);
    }
}

#[cfg(test)]
mod vocab_tests {
    use super::*;

    #[test]
    fn push_row_feeds_vocab() {
        let mut app = App::new(100);
        let row = crate::model::EntryRow::from_line(
            "01-01 00:00:00.000  1234  1234 I VocabTag: hello world test123",
        );
        app.push_row(row.unwrap());
        let cands = app.vocab.tag_candidates("Vocab");
        assert!(
            !cands.is_empty(),
            "VocabTag should appear in tag candidates"
        );
        assert_eq!(cands[0], "VocabTag");
    }
}

#[cfg(test)]
mod file_store_tests {
    use super::*;
    use crate::filter_model::Group;
    use aloggrep::expr::Expr;
    use std::io::Write;

    fn write_temp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn file_store_full_browse_no_max_lines_cap() {
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!(
                "04-02 10:00:{i:02}.000  1  1 I Tag{i}   : line{i}\n"
            ));
        }
        let f = write_temp(&body);
        let mut app = App::new(10); // max_lines would cap stream; ignored for file
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        assert_eq!(app.visible_len(), 50);
        app.jump_bottom();
        assert_eq!(app.cursor, 49);
        assert_eq!(app.row_at(0).unwrap().tag, "Tag0");
        assert_eq!(app.row_at(49).unwrap().tag, "Tag49");
    }

    #[test]
    fn file_filter_uses_subset_not_matched_buffer() {
        let f = write_temp(
            "04-02 10:00:00.000  1  1 I Keep   : a\n\
             04-02 10:00:01.000  1  1 I Drop   : b\n\
             04-02 10:00:02.000  1  1 I Keep   : c\n",
        );
        let mut app = App::new(100);
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        app.groups.groups.push(Group {
            label: "tag~Keep".into(),
            chips: Vec::new(),
            expr: Some(Expr::parse("tag ~ Keep", true).unwrap()),
            enabled: true,
        });
        app.rebuild_visible();
        // Drain bg filter to completion.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !app.ingest_done {
            if Instant::now() > deadline {
                panic!("filter timed out");
            }
            app.poll_file_store();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(app.visible, Visible::Subset(_)));
        assert_eq!(app.visible_len(), 2);
        assert!(app.matched().is_empty());
        assert_eq!(app.row_at(0).unwrap().tag, "Keep");
        assert_eq!(app.row_at(1).unwrap().tag, "Keep");
    }

    #[test]
    fn file_unparseable_kept_as_raw() {
        let f = write_temp("not a log\n04-02 10:00:00.000  1  1 I TagA   : ok\n");
        let mut app = App::new(100);
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        assert_eq!(app.visible_len(), 2);
        assert_eq!(app.row_at(0).unwrap().raw, "not a log");
        assert!(!app.row_at(0).unwrap().is_parsed());
        assert_eq!(app.row_at(1).unwrap().tag, "TagA");
    }

    #[test]
    fn file_unparseable_rejected_under_any_filter() {
        let f = write_temp(
            "================= QQXlog open =================\n\
             04-02 10:00:00.000  1  1 I TagA   : ok\n\
             04-02 10:00:01.000  1  1 W TagB   : warn\n",
        );
        let mut app = App::new(100);
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        assert_eq!(app.visible_len(), 3);

        // msg filter (would substring-match the header if unparsed were allowed)
        app.groups.groups.push(Group {
            label: "msg~QQXlog".into(),
            chips: Vec::new(),
            expr: Some(Expr::parse("msg ~ QQXlog", true).unwrap()),
            enabled: true,
        });
        app.rebuild_visible();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !app.ingest_done {
            if Instant::now() > deadline {
                panic!("filter timed out");
            }
            app.poll_file_store();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.visible_len(), 0, "unparsed must not pass any active filter");

        // Clear include; time window alone must also drop unparsed
        app.groups.groups.clear();
        app.time_bound = Some(crate::filter_model::TimeBound {
            since: Some("04-02 10:00:00".into()),
            until: None,
        });
        app.rebuild_visible();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !app.ingest_done {
            if Instant::now() > deadline {
                panic!("time filter timed out");
            }
            app.poll_file_store();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.visible_len(), 2);
        assert!(app.row_at(0).unwrap().is_parsed());
        assert_eq!(app.row_at(0).unwrap().tag, "TagA");
    }

    #[test]
    fn row_passes_filters_rejects_unparsed_when_active() {
        let mut app = App::new(100);
        let raw = EntryRow::from_line_or_raw("QQXlog header");
        assert!(!raw.is_parsed());
        assert!(
            app.row_passes_filters(&raw),
            "no filter → unparsed still eligible for vacuous pass"
        );
        app.groups.groups.push(Group {
            label: "level".into(),
            chips: Vec::new(),
            expr: Some(Expr::parse("level >= I", true).unwrap()),
            enabled: true,
        });
        assert!(app.filter_active());
        assert!(!app.row_passes_filters(&raw));
    }

    #[test]
    fn file_visible_idx_for_row_id_uses_line_mapping() {
        let f = write_temp(
            "04-02 10:00:00.000  1  1 I Keep   : a\n\
             04-02 10:00:01.000  1  1 I Drop   : b\n\
             04-02 10:00:02.000  1  1 I Keep   : c\n",
        );
        let mut app = App::new(100);
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        // Unfiltered: row_id 3 → visible 2
        assert_eq!(app.visible_idx_for_row_id(3), Some(2));
        app.groups.groups.push(Group {
            label: "tag~Keep".into(),
            chips: Vec::new(),
            expr: Some(Expr::parse("tag ~ Keep", true).unwrap()),
            enabled: true,
        });
        app.rebuild_visible();
        let deadline = Instant::now() + Duration::from_secs(2);
        while !app.ingest_done {
            if Instant::now() > deadline {
                panic!("filter timed out");
            }
            app.poll_file_store();
            std::thread::sleep(Duration::from_millis(5));
        }
        // Filtered Subset [0, 2]: row_id 3 (line 2) → visible 1
        assert_eq!(app.visible_idx_for_row_id(3), Some(1));
        assert_eq!(app.visible_idx_for_row_id(2), None); // Drop filtered out
        assert_eq!(app.jump_to_bookmark(3), JumpResult::Ok);
        assert_eq!(app.cursor, 1);
    }

    fn wait_highlight_done(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !app.highlight_scan.done {
            if Instant::now() > deadline {
                panic!("highlight scan timed out");
            }
            app.poll_file_store();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn file_highlight_scan_stats_and_n_without_ui_full_parse() {
        use crate::highlight_model::HighlightGroup;

        let f = write_temp(
            "04-02 10:00:00.000  1  1 I T   : aaa\n\
             04-02 10:00:01.000  1  1 I T   : hit one\n\
             04-02 10:00:02.000  1  1 I T   : bbb\n\
             04-02 10:00:03.000  1  1 I T   : hit two\n",
        );
        let mut app = App::new(100);
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        app.following = false;
        app.cursor = 0;
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
        wait_highlight_done(&mut app);
        assert_eq!(app.highlight_scan.hits, vec![1, 3]);
        assert_eq!(app.highlight_match_stats(), Some((None, 2)));
        assert!(app.find_match(1));
        assert_eq!(app.cursor, 1);
        assert_eq!(app.highlight_match_stats(), Some((Some(1), 2)));
        assert!(app.find_match(1));
        assert_eq!(app.cursor, 3);
        assert!(app.find_match(1)); // wrap
        assert_eq!(app.cursor, 1);
        assert!(app
            .log_loading_label()
            .is_none_or(|s| !s.contains("Highlight")));
    }

    #[test]
    fn file_highlight_scan_cancels_on_filter_change() {
        use crate::highlight_model::HighlightGroup;

        let f = write_temp(
            "04-02 10:00:00.000  1  1 I Keep   : hit a\n\
             04-02 10:00:01.000  1  1 I Drop   : hit b\n\
             04-02 10:00:02.000  1  1 I Keep   : other\n",
        );
        let mut app = App::new(100);
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
        wait_highlight_done(&mut app);
        assert_eq!(app.highlight_scan.hits, vec![0, 1]);
        let gen_before = app.highlight_scan.gen;

        app.groups.groups.push(Group {
            label: "tag~Keep".into(),
            chips: Vec::new(),
            expr: Some(Expr::parse("tag ~ Keep", true).unwrap()),
            enabled: true,
        });
        app.rebuild_visible();
        assert_ne!(app.highlight_scan.gen, gen_before);
        let deadline = Instant::now() + Duration::from_secs(2);
        while !app.ingest_done {
            if Instant::now() > deadline {
                panic!("filter timed out");
            }
            app.poll_file_store();
            std::thread::sleep(Duration::from_millis(5));
        }
        wait_highlight_done(&mut app);
        // Vis domain is Keep rows only: line 0 (hit) and line 2 (no hit) → one hit at vis 0.
        assert_eq!(app.highlight_scan.hits, vec![0]);
        assert_eq!(app.visible_len(), 2);
    }

    #[test]
    fn file_find_severe_uses_cache_not_full_visible_parse() {
        let f = write_temp(
            "04-02 10:00:00.000  1  1 I T   : ok\n\
             04-02 10:00:01.000  1  1 E T   : err\n\
             04-02 10:00:02.000  1  1 I T   : ok2\n",
        );
        let mut app = App::new(100);
        app.set_file_store(FileStore::open_sync(f.path()).unwrap());
        // Wait for severe prefetch to fill cache for all lines.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let done = app
                .store
                .as_file()
                .map(|f| f.progress().severe_done)
                .unwrap_or(true);
            if done {
                break;
            }
            if Instant::now() > deadline {
                panic!("severe prefetch timed out");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.store.as_file().unwrap().severe_cached(1), Some(true));
        app.following = false;
        app.cursor = 0;
        assert!(app.find_severe(1));
        assert_eq!(app.cursor, 1);
        assert!(app.log_loading_label().is_none());
    }

    #[test]
    fn file_log_loading_label_during_filter() {
        let mut body = String::new();
        for i in 0..200 {
            body.push_str(&format!("04-02 10:00:00.000  1  1 I Tag{i}   : line{i}\n"));
        }
        let f = write_temp(&body);
        let mut app = App::new(100);
        app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
        // Async open so filter can still be in-flight when we sample the label.
        app.set_file_store(FileStore::open(f.path()).unwrap());
        app.groups.groups.push(Group {
            label: "tag~Tag1".into(),
            chips: Vec::new(),
            expr: Some(Expr::parse("tag ~ Tag1", true).unwrap()),
            enabled: true,
        });
        app.rebuild_visible();
        // Either indexing or filtering should produce a loading label.
        let mut saw_loading = false;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            app.poll_file_store();
            if let Some(label) = app.log_loading_label() {
                assert!(
                    label.contains("Indexing")
                        || label.contains("Filtering")
                        || label.contains("Highlight"),
                    "unexpected label {label}"
                );
                saw_loading = true;
                break;
            }
            if app.ingest_done {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            saw_loading || app.ingest_done,
            "expected loading or quick finish"
        );
    }
}
