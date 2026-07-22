use std::collections::VecDeque;
use std::sync::{mpsc::Receiver, OnceLock};
use std::time::{Duration, Instant};

use aloggrep::crash::CrashDetector;
use aloggrep::parser::Level;

use crate::bookmark::{bookmark_label, AddError, Bookmark, BookmarkList, JumpResult};
use crate::filter_model::{ExcludeEntry, Group, GroupList};
use crate::model::EntryRow;
use crate::highlight_model::{HighlightBox, HighlightGroup, HighlightGroupList};
use crate::vocab::Vocab;

/// Hard cap on the matched-rows buffer (OOM safety). When a filter is active,
/// matching rows are retained in `App::matched` independently of `rows`'
/// rolling eviction; only this cap reclaims them.
const MATCHED_HARD_CAP: usize = 1_000_000;

fn crash_detector() -> &'static CrashDetector {
    static DETECTOR: OnceLock<CrashDetector> = OnceLock::new();
    DETECTOR.get_or_init(CrashDetector::new)
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

/// Severe = level E/F, or a crash signature in the message (H2 jump target).
pub fn is_severe_row(row: &EntryRow) -> bool {
    matches!(row.level, Level::E | Level::F)
        || crash_detector().detect(&row.as_log_entry()).is_some()
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
    pub rows: VecDeque<EntryRow>,
    /// Matched-rows buffer: when a filter is active, rows passing the filter
    /// are retained here independently of `rows`' rolling eviction, so a tight
    /// filter isn't washed out by non-matching churn. Capped by `matched_cap`.
    pub matched: VecDeque<EntryRow>,
    /// Hard cap on `matched` (OOM safety). Matched rows are never evicted by
    /// `rows` overflow — only by reaching this cap. Defaults to
    /// [`MATCHED_HARD_CAP`]; tests override.
    pub matched_cap: usize,
    pub visible: Vec<usize>,
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
    /// Armed by `m` on LogList; second key is `a`/`d` (M2).
    pub pending_m: bool,
    /// Armed by leader key (`Space`); second key opens fzf-style picker (Task 5).
    pub pending_leader: bool,
    /// Open fzf-style picker session (Unified Manage / Filter / Highlight / Bookmark / Exclude).
    pub picker: Option<crate::picker::PickerSession>,
    /// Session bookmarks (M2).
    pub bookmarks: BookmarkList,
    /// Next ingest `row_id` (M2).
    pub next_row_id: u64,
    /// Session lock: at most one of pid/tid is set (H8; AND after chip filter).
    pub lock_pid: Option<String>,
    pub lock_tid: Option<String>,
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
    /// Set to true the first time `drain` finds the ingest channel disconnected
    /// (file fully read or --hdc session ended). Used by P4 draw throttle.
    pub ingest_done: bool,
}

impl App {
    pub fn new(max_lines: usize) -> Self {
        Self {
            rows: VecDeque::new(),
            matched: VecDeque::new(),
            matched_cap: MATCHED_HARD_CAP,
            visible: Vec::new(),
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
            pending_m: false,
            pending_leader: false,
            picker: None,
            bookmarks: BookmarkList::default(),
            next_row_id: 1,
            lock_pid: None,
            lock_tid: None,
            visual_anchor: None,
            following: true,
            list_offset: 0,
            status_msg: None,
            status_flash_until: None,
            last_yanked: None,
            detail: DetailView::Closed,
            export_source: crate::export::ExportSource::default(),
            config: crate::config::AppConfig::default_config(),
            vocab: Vocab::default(),
            match_stats_stale: true,
            match_stats_cursor: usize::MAX, // sentinel: force first computation
            cached_match_stats: None,
            ingest_done: false,
        }
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
        )
    }

    pub fn detail_open(&self) -> bool {
        !matches!(self.detail, DetailView::Closed)
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

    /// Chip filter then session lock (H8). Used by drain and rebuild.
    pub fn row_passes_filters(&self, row: &EntryRow) -> bool {
        if !self.groups.matches(row) {
            return false;
        }
        if let Some(pid) = &self.lock_pid {
            return row.pid == *pid;
        }
        if let Some(tid) = &self.lock_tid {
            return row.tid == *tid;
        }
        true
    }

    /// Whether any include/exclude/lock filter is currently active. When false,
    /// `visible` indexes `rows` directly (every row shown); when true, `visible`
    /// indexes `matched` (only filter-passing rows, retained across `rows` churn).
    pub fn filter_active(&self) -> bool {
        self.groups.has_any_enabled()
            || self.groups.excludes.iter().any(|e| e.enabled)
            || self.lock_pid.is_some()
            || self.lock_tid.is_some()
    }

    /// The buffer `visible` currently indexes: `matched` when a filter is
    /// active, `rows` otherwise. All read paths (render, cursor, yank, search)
    /// go through here so they stay correct across `rows` overflow.
    pub fn view_source(&self) -> &VecDeque<EntryRow> {
        if self.filter_active() {
            &self.matched
        } else {
            &self.rows
        }
    }

    /// Whether `row_id` is still present in either buffer (bookmark liveness).
    /// A row retained in `matched` after being evicted from `rows` is alive.
    fn row_alive(&self, row_id: u64) -> bool {
        self.matched.iter().any(|r| r.row_id == row_id)
            || self.rows.iter().any(|r| r.row_id == row_id)
    }

    /// Drain everything currently available on the channel without blocking.
    /// Each new row is checked against the current filter in O(1) and, if
    /// visible, appended — no full rescan (see design doc "增量过滤").
    /// Drain all pending rows from the ingest channel without blocking.
    /// Sets `self.ingest_done = true` when the sender has been dropped (P4).
    pub fn drain(&mut self, rx: &Receiver<EntryRow>) {
        use std::sync::mpsc::TryRecvError;
        loop {
            match rx.try_recv() {
                Ok(row) => self.push_row(row),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.ingest_done = true;
                    break;
                }
            }
        }
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

        // `rows` rolling buffer. When a filter is active, `visible` tracks
        // `matched`, so a `rows` eviction must NOT shift `visible`.
        if self.rows.len() >= self.max_lines {
            self.rows.pop_front();
            if !active {
                self.shift_visible_after_front_evict(self.visible.first() == Some(&0));
            }
        }

        if matches {
            // Retain in `matched` (clone into `rows`; original goes to `matched`).
            if self.matched.len() >= self.matched_cap {
                self.matched.pop_front();
                self.shift_visible_after_front_evict(self.visible.first() == Some(&0));
            }
            self.rows.push_back(row.clone());
            self.matched.push_back(row);
            self.visible.push(self.matched.len() - 1);
        } else {
            self.rows.push_back(row);
            if !active {
                self.visible.push(self.rows.len() - 1);
            }
        }
        self.follow_tick();
        // P1: any visible change (row added / evicted) invalidates cached stats.
        self.match_stats_stale = true;
    }

    /// Update `visible`/`cursor`/`list_offset`/`visual_anchor` after the front
    /// row of the active source buffer was evicted. `evicted_was_visible`
    /// indicates that row was `visible[0]` (its source index was 0).
    fn shift_visible_after_front_evict(&mut self, evicted_was_visible: bool) {
        if evicted_was_visible {
            self.visible.remove(0);
            // `list_offset` is an index into `visible`; dropping the front item
            // must shift the viewport with the content, otherwise the list
            // appears to scroll up even when no new visible rows arrive.
            if self.list_offset > 0 {
                self.list_offset -= 1;
            }
        }
        for i in self.visible.iter_mut() {
            *i -= 1;
        }
        if evicted_was_visible && self.cursor > 0 {
            self.cursor -= 1;
        }
        // `visual_anchor` shares `cursor`'s coordinate space (index into
        // `visible`). Evicting the oldest visible row shifts that space.
        if evicted_was_visible {
            match self.visual_anchor {
                Some(0) => self.visual_anchor = None,
                Some(a) => self.visual_anchor = Some(a - 1),
                None => {}
            }
        }
    }

    /// Full rescan, used when the filter groups or session lock change.
    /// Rebuilds `matched` from the current `rows`: rows already evicted from
    /// `rows` (even if previously retained in `matched`) cannot be recovered.
    pub fn rebuild_visible(&mut self) {
        let active = self.filter_active();
        if active {
            self.matched.clear();
            for row in &self.rows {
                if self.row_passes_filters(row) {
                    self.matched.push_back(row.clone());
                }
            }
            self.visible = (0..self.matched.len()).collect();
        } else {
            self.matched.clear();
            self.visible = (0..self.rows.len()).collect();
        }
        if self.following {
            self.jump_bottom();
        } else if self.cursor >= self.visible.len() {
            self.cursor = self.visible.len().saturating_sub(1);
        }
        self.match_stats_stale = true;
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

    pub fn visible_rows(&self) -> impl Iterator<Item = &EntryRow> {
        let source = self.view_source();
        self.visible.iter().map(move |&i| &source[i])
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
            }
        }
    }

    pub fn current_row(&self) -> Option<&EntryRow> {
        let &i = self.visible.get(self.cursor)?;
        Some(&self.view_source()[i])
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
            self.set_flash("无选中行");
            return;
        };
        let bm = Bookmark {
            row_id: row.row_id,
            label: bookmark_label(&row.tag, &row.msg),
            enabled: true,
        };
        match self.bookmarks.try_add(bm) {
            Ok(()) => self.set_flash("已收藏"),
            Err(AddError::Duplicate) => self.set_flash("已存在"),
            Err(AddError::Full) => self.set_flash("书签已满"),
        }
    }

    /// `md`: remove bookmark for current row.
    pub fn bookmark_remove_current(&mut self) {
        self.pending_m = false;
        self.pending_leader = false;
        let Some(row) = self.current_row() else {
            self.set_flash("无选中行");
            return;
        };
        if self.bookmarks.remove_id(row.row_id) {
            self.set_flash("已删除");
        } else {
            self.set_flash("未收藏");
        }
    }

    /// Jump to a bookmarked row_id; sets `following=false` on success.
    /// Disabled bookmarks (`enabled == false`) are treated as filtered-out.
    /// A row retained in `matched` (evicted from `rows`) is still jumpable.
    pub fn jump_to_bookmark(&mut self, row_id: u64) -> JumpResult {
        if self
            .bookmarks
            .items
            .iter()
            .find(|b| b.row_id == row_id)
            .is_some_and(|b| !b.enabled)
        {
            return JumpResult::Filtered;
        }
        // `view_source()` is the active buffer `visible` indexes; by
        // construction every entry there is in `visible` (front-eviction
        // shifts both in tandem), so a hit here is always jumpable.
        let row_idx = self.view_source().iter().position(|r| r.row_id == row_id);
        let Some(row_idx) = row_idx else {
            return if self.row_alive(row_id) {
                JumpResult::Filtered
            } else {
                JumpResult::Evicted
            };
        };
        let Some(vis) = self.visible.iter().position(|&i| i == row_idx) else {
            return JumpResult::Filtered;
        };
        self.following = false;
        self.cursor = vis;
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
        self.pending_leader = false;
        self.pending_lock = true;
    }

    /// Cancel `f` pending without clearing lock or touching `following`.
    pub fn cancel_lock_pending(&mut self) {
        self.pending_lock = false;
        self.pending_leader = false;
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
            self.set_flash("无锁定");
        }
    }

    /// `f` `p` / `f` `t`: set, toggle-clear, or switch lock target.
    pub fn apply_session_lock(&mut self, kind: LockKind) {
        let Some(row) = self.current_row() else {
            self.set_flash("无选中行");
            return;
        };
        let value = match kind {
            LockKind::Pid => row.pid.clone(),
            LockKind::Tid => row.tid.clone(),
        };
        if value.is_empty() {
            self.set_flash(match kind {
                LockKind::Pid => "空 pid",
                LockKind::Tid => "空 tid",
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
    pub fn lock_badge_label(&self) -> Option<String> {
        if let Some(pid) = &self.lock_pid {
            return Some(format!("LOCK pid={pid}"));
        }
        if let Some(tid) = &self.lock_tid {
            return Some(format!("LOCK tid={tid}"));
        }
        None
    }

    /// Push a single-field filter group from the current row (H7, non-msg path).
    /// Returns `true` when a new group was pushed.
    pub fn push_chip_from_field(&mut self, field: crate::input::ChipField) -> bool {
        use crate::input::{Chip, ChipField};
        let Some(row) = self.current_row() else {
            self.set_flash("无选中行");
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
        let value = Self::field_text(row, yank);
        if value.is_empty() {
            self.set_flash(format!("空 {}", field.keyword()));
            return false;
        }
        self.push_single_chip_filter(Chip { field, value })
    }

    /// Open msg token picker for the current row (`c`/`C`+`m`).
    pub fn begin_msg_chip_picker(&mut self, as_exclude: bool) {
        let Some(row) = self.current_row() else {
            self.set_flash("无选中行");
            return;
        };
        let tokens = crate::input::msg_token_candidates(&row.msg);
        if tokens.is_empty() {
            self.pending_chip = false;
            self.pending_exclude = false;
            self.pending_leader = false;
            self.set_flash("无可选片段");
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
                &picker.draft,
            );
            let value = visible
                .get(picker.selected)
                .and_then(|&index| picker.choices.get(index))
                .cloned()
                .or_else(|| (!picker.draft.is_empty()).then(|| picker.draft.clone()))?;
            Some((exclude, value))
        }) else {
            self.set_flash("无可选片段");
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
            self.set_flash("无选中行");
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
        let value = Self::field_text(row, yank);
        if value.is_empty() {
            self.set_flash(format!("空 {}", field.keyword()));
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
                self.set_flash("已存在");
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
            self.set_flash("已存在");
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
        self.current_row().map(|row| Self::field_text(row, field))
    }

    /// Join `field` values for visible indices `lo..=hi` with newlines.
    pub fn yank_range(&self, lo: usize, hi: usize, field: YankField) -> Option<String> {
        if self.visible.is_empty() || lo > hi || hi >= self.visible.len() {
            return None;
        }
        let source = self.view_source();
        let mut parts = Vec::with_capacity(hi - lo + 1);
        for vi in lo..=hi {
            let row = &source[self.visible[vi]];
            parts.push(Self::field_text(row, field));
        }
        Some(parts.join("\n"))
    }

    pub fn record_yank(&mut self, text: String) {
        self.last_yanked = Some(text);
    }

    /// Jump to the next (`dir > 0`) or previous (`dir < 0`) severe visible row
    /// (level E/F or crash). Wraps like vim `wrapscan`. Independent of search.
    pub fn find_severe(&mut self, dir: i8) -> bool {
        let n = self.visible.len();
        if n == 0 {
            return false;
        }
        let step: isize = if dir >= 0 { 1 } else { -1 };
        let start = self.cursor as isize;
        for offset in 1..=n as isize {
            let idx = (start + offset * step).rem_euclid(n as isize) as usize;
            let row_idx = self.visible[idx];
            if self.view_source()[row_idx].severe {
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
        let n = self.visible.len();
        if n == 0 {
            return false;
        }
        let step: isize = if dir >= 0 { 1 } else { -1 };
        let start = self.cursor as isize;
        for offset in 1..=n as isize {
            let idx = (start + offset * step).rem_euclid(n as isize) as usize;
            let row_idx = self.visible[idx];
            let row = &self.view_source()[row_idx];
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
        for idx in 0..self.visible.len() {
            let row_idx = self.visible[idx];
            let row = &self.view_source()[row_idx];
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
        true
    }

    pub fn clear_highlight_groups(&mut self) {
        self.highlight_groups.groups.clear();
        self.active_highlight = None;
        self.highlight_cursor = 0;
        self.match_stats_stale = true;
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
        true
    }

    pub fn update_bookmark_label(&mut self, row_id: u64, label: String) -> bool {
        self.bookmarks.update_label(row_id, label)
    }

    pub fn clear_bookmarks(&mut self) {
        self.bookmarks.clear();
    }

    pub fn delete_bookmark_at(&mut self, index: usize) -> bool {
        self.bookmarks.delete_at(index)
    }

    /// Toggle `enabled` on a unified Manage item. Returns whether state changed.
    pub fn toggle_unified_enabled(&mut self, kind: crate::picker::UnifiedKind, index: usize) -> bool {
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
            UnifiedKind::Bookmark => {
                let Some(b) = self.bookmarks.items.get_mut(index) else {
                    return false;
                };
                b.enabled = !b.enabled;
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
            UnifiedKind::Bookmark => self.delete_bookmark_at(index),
        }
    }

    /// Push a search group, or return the index of an existing equivalent.
    /// Always marks the returned index as the globally active search.
    /// Caller always jumps to that group's first match.
    pub fn push_or_find_highlight_group(&mut self, group: crate::highlight_model::HighlightGroup) -> usize {
        let idx = if let Some(idx) = self.highlight_groups.find_equivalent(&group.pattern) {
            idx
        } else {
            self.highlight_groups.groups.push(group);
            self.highlight_groups.groups.len() - 1
        };
        self.active_highlight = Some(idx);
        self.match_stats_stale = true;
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
    /// Called once per draw cycle in `run()` so that the O(n) scan is always done
    /// BEFORE any render work, keeping the render phase O(viewport).
    pub fn recompute_match_stats_if_stale(&mut self) {
        if self.match_stats_stale || self.cursor != self.match_stats_cursor {
            self.match_stats_stale = false;
            self.match_stats_cursor = self.cursor;
            self.cached_match_stats = self.compute_match_stats_inner();
        }
    }

    fn compute_match_stats_inner(&self) -> Option<(Option<usize>, usize)> {
        let Some(group) = self.active_highlight_group() else {
            return None;
        };
        let mut total = 0usize;
        let mut current = None;
        let source = self.view_source();
        for (idx, &row_idx) in self.visible.iter().enumerate() {
            let row = &source[row_idx];
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
            time: None,
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
    fn test_ring_buffer_evicts_oldest_and_shifts_indices() {
        let mut app = App::new(2);
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap();
        tx.send(row("C")).unwrap();
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.rows.len(), 2);
        assert_eq!(app.rows[0].tag, "B");
        assert_eq!(app.rows[1].tag, "C");
        assert_eq!(app.visible, vec![0, 1]);
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
        assert_eq!(app.visible, vec![0, 1]);
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
        assert_eq!(app.rows.len(), 2);
        assert_eq!(app.rows[0].tag, "X");
        assert_eq!(app.rows[1].tag, "Y");
        assert_eq!(app.matched.len(), 1);
        assert_eq!(app.matched[0].tag, "A");
        assert_eq!(app.visible, vec![0]);
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
        assert_eq!(app.rows[0].tag, "B");
        assert_eq!(app.rows[1].tag, "A");
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
        assert_eq!(app.matched.len(), 1);
        assert_eq!(app.matched[0].tag, "A");
        assert_eq!(app.visible, vec![0]);
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
            time: None,
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
            time: None,
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
                time: None,
                enabled: true,
            }],
            excludes: Vec::new(),
        };
        let (tx, rx) = mpsc::channel();
        tx.send(row("A")).unwrap();
        tx.send(row("B")).unwrap(); // doesn't match "a" group, filtered out
        drop(tx);
        app.drain(&rx);
        assert_eq!(app.visible, vec![0]); // only A is visible
        assert!(app.following);

        app.groups.groups.clear(); // simulates deleting the last filter group -> empty GroupList matches everything
        app.rebuild_visible();
        assert_eq!(app.visible, vec![0, 1]); // both now visible, set grew
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
        assert_eq!(app.rows[app.visible[app.cursor]].msg, "bar later");
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
        assert_eq!(app.rows[app.visible[app.cursor]].tag, "MyTag");
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
        assert_eq!(app.rows[app.visible[app.cursor]].msg, "bar mid");
        assert!(app.find_match(1)); // wrap to same hit
        assert_eq!(app.rows[app.visible[app.cursor]].msg, "bar mid");
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
                enabled: true,
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
                enabled: true,
            })
            .unwrap();
        assert!(app.update_bookmark_label(42, "new".into()));
        assert_eq!(app.bookmarks.items[0].label, "new");
        assert!(!app.update_bookmark_label(99, "x".into()));
    }

    #[test]
    fn toggle_unified_enabled_bookmark() {
        use crate::picker::UnifiedKind;

        let mut app = App::new(100);
        app.bookmarks
            .try_add(Bookmark {
                row_id: 1,
                label: "b".into(),
                enabled: true,
            })
            .unwrap();
        assert!(app.toggle_unified_enabled(UnifiedKind::Bookmark, 0));
        assert!(!app.bookmarks.items[0].enabled);
        assert!(app.toggle_unified_enabled(UnifiedKind::Bookmark, 0));
        assert!(app.bookmarks.items[0].enabled);
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
            time: None,
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
        assert!(is_severe_row(&app.rows[app.visible[1]]));
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
        assert!(!cands.is_empty(), "VocabTag should appear in tag candidates");
        assert_eq!(cands[0], "VocabTag");
    }
}
