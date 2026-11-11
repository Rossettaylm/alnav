//! Picker, command palette, source panels, confirm dialog, detail overlay, and flash toasts.

use super::{App, DetailView, Focus, Mode};
use std::time::{Duration, Instant};

impl App {
    /// Open the unified Manage picker (aggregated Filter/Highlight/Exclude/Bookmark).
    pub fn open_unified_picker(&mut self) {
        self.open_picker(crate::picker::PickerKind::Unified);
    }

    /// Open the requested fzf-style picker in Manage mode. Clears operator-pending.
    /// Does not auto-switch to New (use [`Self::open_picker_new`]).
    pub fn open_picker(&mut self, kind: crate::picker::PickerKind) {
        self.open_picker_with(kind, false);
    }

    /// Whether `C-p` may open the command palette (R3: Normal LogList/strips,
    /// no other modal). Pending chords are allowed — opening clears them.
    pub fn command_palette_available(&self) -> bool {
        if self.command_palette.is_some()
            || self.confirm.is_some()
            || self.picker.is_some()
            || self.time_panel.is_some()
            || self.detail_open()
            || self.highlight_box.editing
            || self.help_open
            || self.compare.is_some()
            || self.summary_open()
            || self.hist_open()
            || self.dashboard.is_some()
            || self.open_file_panel.is_some()
            || self.stream_source_panel.is_some()
            || self.preset_name.is_some()
            || self.mode != Mode::Normal
        {
            return false;
        }
        matches!(
            self.focus,
            Focus::LogList | Focus::ChipStrip | Focus::ExcludeStrip | Focus::HighlightStrip
        )
    }

    /// Open the command palette from a Normal LogList/strip surface.
    /// Clears pending chords and stops following. Does not change focus.
    /// No-op when [`Self::command_palette_available`] is false.
    pub fn open_command_palette(&mut self) {
        if !self.command_palette_available() {
            return;
        }
        self.clear_pending_all();
        self.clear_visual();
        self.following = false;
        self.command_palette = Some(crate::command_palette::CommandPalette::new());
    }

    /// Close the command palette without resuming follow.
    pub fn close_command_palette(&mut self) {
        self.command_palette = None;
    }

    /// Open the picker forced into New mode (`;` `` ` ``, palette Add Highlight).
    pub fn open_picker_new(&mut self, kind: crate::picker::PickerKind) {
        self.open_picker_with(kind, true);
    }

    /// LogList `/`: empty groups → Highlight New; otherwise Highlight Manage.
    pub fn open_highlight_finder(&mut self) {
        if self.highlight_groups.groups.is_empty() {
            self.open_picker_new(crate::picker::PickerKind::Highlight);
        } else {
            self.open_picker(crate::picker::PickerKind::Highlight);
        }
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

    pub fn open_file_source_panel(&mut self, from_dashboard: bool) {
        self.clear_pending_all();
        self.stream_source_panel = None;
        self.picker = None;
        self.log_corpus.configure(
            self.config.log_dirs.clone(),
            self.config.log_extensions.clone(),
        );
        self.log_corpus.ensure_started();
        if self.recent.paths.is_empty() && self.log_corpus.roots_empty() {
            self.set_flash("configure log_dirs in config.toml");
        }
        self.open_file_panel = Some(crate::source_panel::OpenFilePanel::open(
            &self.recent,
            &self.log_corpus,
            from_dashboard,
        ));
    }

    pub fn open_stream_source_panel(&mut self, from_dashboard: bool) {
        self.clear_pending_all();
        if self.open_file_panel.is_some() {
            self.log_corpus.cancel_inflight();
        }
        self.open_file_panel = None;
        self.picker = None;
        self.stream_source_panel =
            Some(crate::source_panel::StreamSourcePanel::new(from_dashboard));
    }

    pub fn close_open_file_panel(&mut self) {
        self.log_corpus.cancel_inflight();
        self.open_file_panel = None;
    }

    /// Refresh Open-file candidate list from current recent + corpus (split borrow).
    pub fn refresh_open_file_choices(&mut self) {
        let recent = self.recent.clone();
        let Some(mut panel) = self.open_file_panel.take() else {
            return;
        };
        panel.refresh_choices(&recent, &self.log_corpus);
        self.open_file_panel = Some(panel);
    }

    pub fn close_source_panels(&mut self) {
        self.close_open_file_panel();
        self.stream_source_panel = None;
    }

    /// Close the fzf-style picker and return focus to LogList.
    /// Does not change live-follow state.
    pub fn close_picker(&mut self) {
        self.picker = None;
        self.confirm = None;
        self.pending_leader = false;
        self.focus = Focus::LogList;
        self.vocab_match.clear();
    }

    pub fn request_confirm(&mut self, kind: crate::picker::ConfirmKind) {
        self.confirm = Some(kind);
    }

    pub fn cancel_confirm(&mut self) {
        self.confirm = None;
    }

    pub fn request_delete_many(&mut self, items: Vec<crate::picker::UnifiedId>) {
        if items.is_empty() {
            return;
        }
        self.confirm = Some(crate::picker::ConfirmKind::DeleteMany { items });
    }

    /// Ask to wipe every rule, or flash `NO RULES` when the lists are empty.
    pub fn request_clear_all_rules(&mut self) {
        if !self.has_rules() {
            self.set_flash("NO RULES");
            return;
        }
        self.confirm = Some(crate::picker::ConfirmKind::ClearAll);
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
}
