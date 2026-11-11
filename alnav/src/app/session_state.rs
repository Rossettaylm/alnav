//! Session capture/restore, source switching, and recent-file tracking.

use super::Pending;
use super::{App, DetailView, Focus, Mode, SummaryView, ViewFocus, Visible};
use crate::bookmark::Bookmark;
use crate::help::HelpView;
use crate::highlight_model::HighlightBox;
use crate::hist_panel::HistView;
use crate::model::{is_severe_row, EntryRow};
use crate::scan::HighlightScanState;
use crate::store::RowStore;
use crate::vocab::Vocab;

impl App {
    /// Reset session for a confirmed source switch. Keeps Filter / Exclude / Highlight only.
    pub fn reset_for_source_switch(&mut self) {
        self.store = RowStore::stream(self.max_lines, self.matched_cap);
        self.visible = Visible::default();
        self.file_filter_gen = 0;
        self.ingest_done = false;
        self.clear_bookmarks();
        self.clear_visual();
        self.lock_pid = None;
        self.lock_tid = None;
        self.time_bound = None;
        self.view_focus = ViewFocus::default();
        self.time_panel = None;
        self.command_palette = None;
        self.confirm = None;
        self.compare = None;
        self.detail = DetailView::Closed;
        self.help_open = false;
        self.help_view = HelpView::default();
        self.help_search = None;
        self.summary_view = SummaryView::Closed;
        self.summary_scroll = 0;
        self.hist_view = HistView::Closed;
        self.hist_cursor = 0;
        self.highlight_box = HighlightBox::default();
        self.active_highlight = None;
        self.pending_jump_first = None;
        self.highlight_scan = HighlightScanState::default();
        self.highlight_domain = None;
        self.vocab = Vocab::default();
        self.vocab_match.clear();
        self.pending = Pending::None;
        self.close_source_panels();
        self.picker = None;
        self.preset_name = None;
        self.cursor = 0;
        self.list_offset = 0;
        self.next_row_id = 1;
        self.match_stats_stale = true;
        self.cached_match_stats = None;
        self.focus = Focus::LogList;
        self.mode = Mode::Normal;
        self.following = true;
        self.status_msg = None;
        self.status_flash_until = None;
        // Session-scoped `session_restore_disabled` survives; only the transients reset.
        self.pending_restore_cursor = None;
        self.pending_restore_offset = 0;
        self.session_restored = false;
        // Keep: groups / excludes / highlight_groups
    }

    /// Record a successfully opened file into recent + persist.
    pub fn record_recent_file(&mut self, path: &std::path::Path) {
        let limit = self.config.recent_files_limit;
        self.recent.record(path, limit);
        if let Err(e) = self.recent.save(&self.config_dir) {
            self.set_flash(format!("RECENT SAVE: {e}"));
        }
    }

    /// Snapshot the analysis session for `source`.
    ///
    /// `None` when sessions are suppressed (R5) or the result is degenerate
    /// (R6) — a degenerate snapshot must not overwrite a real archive.
    /// `following` / `collapsed_view` / visual / modal state are never captured.
    pub fn capture_session(&self, source: &std::path::Path) -> Option<crate::session::SessionFile> {
        if self.session_restore_disabled {
            return None;
        }
        let key = crate::session::normalize_source(source)?;
        let mut file = crate::session::SessionFile::new(key);
        file.filters = self
            .groups
            .groups
            .iter()
            .filter(|g| !g.chips.is_empty())
            .map(|g| crate::session::SessionFilterGroup {
                chips: g.chips.iter().map(crate::session::chip_to_wire).collect(),
                enabled: g.enabled,
                same_field_op: crate::session::same_field_op_to_wire(g.same_field_op),
            })
            .collect();
        file.excludes = self
            .groups
            .excludes
            .iter()
            .map(|e| crate::session::SessionFilterGroup {
                chips: vec![crate::session::chip_to_wire(&e.chip)],
                enabled: e.enabled,
                same_field_op: crate::session::same_field_op_to_wire(
                    crate::fuzzy::SameFieldOp::And,
                ),
            })
            .collect();
        file.highlights = self
            .highlight_groups
            .groups
            .iter()
            .filter(|g| !g.pattern.is_empty())
            .map(|g| crate::session::SessionHighlight {
                pattern: g.pattern.clone(),
                enabled: g.enabled,
            })
            .collect();
        file.active_highlight = self.active_highlight.filter(|i| *i < file.highlights.len());
        file.lock_pid = self.lock_pid.clone();
        file.lock_tid = self.lock_tid.clone();
        if let Some(bound) = &self.time_bound {
            file.time_since = bound.since.clone();
            file.time_until = bound.until.clone();
        }
        file.view_focus_highlight = self.view_focus.highlight;
        file.view_focus_severe = self.view_focus.severe;
        // The stable id, not `self.cursor` (a visible-slot index that shifts
        // with the filter). File mode: `row_id == line_index + 1`.
        file.cursor_row_id = self.current_row().map(|r| r.row_id).unwrap_or(0);
        file.list_offset = self.list_offset;
        file.bookmarks = self
            .bookmarks
            .items
            .iter()
            .map(|b| crate::session::SessionBookmark {
                row_id: b.row_id(),
                raw: b.row.raw.clone(),
            })
            .collect();
        if file.is_degenerate() {
            return None;
        }
        Some(file)
    }

    /// Apply a loaded session. Rules / time / lock / focus / pins land now; the
    /// cursor is deferred to [`Self::poll_file_store`] (R7).
    pub fn apply_session(&mut self, s: &crate::session::SessionFile) -> Result<(), String> {
        let (groups, highlights, active) = crate::session::materialize_rules(s)?;
        self.groups = groups;
        self.highlight_groups = highlights;
        self.active_highlight = active;
        self.lock_pid = s.lock_pid.clone();
        // pid/tid lock is mutually exclusive; a hand-edited file keeps pid.
        self.lock_tid = if self.lock_pid.is_some() {
            None
        } else {
            s.lock_tid.clone()
        };
        self.time_bound = s.time_bound();
        self.view_focus = ViewFocus {
            highlight: s.view_focus_highlight,
            severe: s.view_focus_severe,
        };
        self.bookmarks.clear();
        self.bookmark_row_ids.clear();
        for bm in &s.bookmarks {
            let mut row = EntryRow::from_line_or_raw(&bm.raw);
            row.row_id = bm.row_id;
            row.severe = is_severe_row(&row);
            if self.bookmarks.try_add(Bookmark::from_row(row)).is_ok() {
                self.bookmark_row_ids.insert(bm.row_id);
            }
        }
        if s.cursor_row_id > 0 {
            self.pending_restore_cursor = Some(s.cursor_row_id);
            self.pending_restore_offset = s.list_offset;
        }
        self.following = false;
        self.match_stats_stale = true;
        Ok(())
    }

    /// Whether the current file has an archive that `Clear Session` could delete.
    pub fn has_session_worth_clearing(&self) -> bool {
        if self.session_restore_disabled {
            return false;
        }
        match &self.export_source {
            crate::export::ExportSource::File(p) if !p.is_empty() => {
                crate::session::exists(&self.config_dir, std::path::Path::new(p))
            }
            _ => false,
        }
    }

    /// Palette `Clear Session`: delete the current file's archive.
    pub fn clear_session_for_current_file(&mut self) {
        let crate::export::ExportSource::File(path) = self.export_source.clone() else {
            return;
        };
        if path.is_empty() {
            self.set_flash("NO SESSION");
            return;
        }
        let path = std::path::PathBuf::from(path);
        if !crate::session::exists(&self.config_dir, &path) {
            self.set_flash("NO SESSION");
            return;
        }
        match crate::session::clear(&self.config_dir, &path) {
            Ok(()) => self.set_flash("SESSION CLEARED"),
            Err(e) => self.set_flash(format!("SESSION CLEAR FAILED: {e}")),
        }
    }
}
