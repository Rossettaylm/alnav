//! LogList operators: chip/exclude from cursor, msg token picker, lock, time window, view focus, and yank.

use super::{App, LockKind, ViewFocusKind, YankField};
use crate::filter_model::TimeBound;
use crate::highlight_model::HighlightGroup;
use crate::model::EntryRow;
use crate::store::RowStore;
use crate::time_panel::TimePanel;

impl App {
    /// Clear every operator-pending flag (leader / yank / chip / lock / …).
    pub fn clear_pending_all(&mut self) {
        self.pending_d = false;
        self.pending_yank = false;
        self.pending_chip = false;
        self.pending_exclude = false;
        self.pending_lock = false;
        self.pending_time = false;
        self.pending_m = false;
        self.pending_leader = false;
    }

    pub(super) fn sample_rows_for_dates(&self) -> Vec<EntryRow> {
        match &self.store {
            RowStore::Stream(s) => s.rows.iter().cloned().collect(),
            RowStore::File(f) => {
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
        }
    }

    /// Whether `tt` would find at least one date candidate (file-mode catalog).
    /// Early-exits; does not clone the stream buffer (palette `when` hot path).
    pub fn has_time_date_candidates(&self) -> bool {
        match &self.store {
            RowStore::Stream(s) => crate::time_panel::DateCatalog::any_in_rows(s.rows.iter()),
            RowStore::File(f) => {
                let n = f.line_count();
                let step = (n / 4000).max(1);
                let mut i = 0usize;
                while i < n {
                    if let Some(r) = f.row_at(i) {
                        if crate::time_panel::DateCatalog::any_in_rows(std::iter::once(&r)) {
                            return true;
                        }
                    }
                    i = i.saturating_add(step);
                }
                false
            }
        }
    }

    /// Copy `text` to the clipboard and flash YANKED / YANK FAILED.
    pub fn apply_yank(&mut self, text: String) {
        self.record_yank(text.clone());
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text)) {
            Ok(()) => self.set_flash("YANKED (approx)"),
            Err(e) => self.set_flash(format!("YANK FAILED: {e}")),
        }
    }

    /// H10: one-line `alnav grep` command for the current filter state.
    pub fn export_cli_command(&self) -> String {
        crate::export::build_cli_command(
            &self.export_source,
            &self.groups,
            self.lock_pid.as_deref(),
            self.lock_tid.as_deref(),
            self.time_bound.as_ref(),
        )
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

    /// Toggle one session view-focus bit (`fh` / `fe`). Same key again clears that
    /// bit; the other bit is left alone. Enabling highlight with none enabled →
    /// flash `NO HIGHLIGHT` and leave state. Sets `following=false` and rebuilds
    /// on a successful change.
    pub fn toggle_view_focus(&mut self, kind: ViewFocusKind) {
        self.pending_lock = false;
        self.pending_leader = false;
        if kind == ViewFocusKind::Highlight
            && !self.view_focus.highlight
            && !self.highlight_groups.groups.iter().any(|g| g.enabled)
        {
            self.set_flash("NO HIGHLIGHT");
            return;
        }
        self.following = false;
        match kind {
            ViewFocusKind::Highlight => self.view_focus.highlight = !self.view_focus.highlight,
            ViewFocusKind::Severe => self.view_focus.severe = !self.view_focus.severe,
        }
        self.rebuild_visible();
        self.clear_flash();
    }

    /// Status-bar short value when view focus is active (`HL` / `ERR` / `HL+ERR`).
    pub fn view_focus_badge_label(&self) -> Option<&'static str> {
        match (self.view_focus.highlight, self.view_focus.severe) {
            (true, true) => Some("HL+ERR"),
            (true, false) => Some("HL"),
            (false, true) => Some("ERR"),
            (false, false) => None,
        }
    }

    /// `tt`: open time panel from current rows / file sample. Returns false
    /// when refused (empty date candidates). Sets `following=false` on success.
    pub fn open_time_panel(&mut self) -> bool {
        self.pending_time = false;
        let catalog_rows = self.sample_rows_for_dates();
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

    /// Open msg token picker for the current row (`c`/`C`/`y`+`m`).
    pub fn begin_msg_token_picker(&mut self, purpose: crate::picker::MsgChipPurpose) {
        let Some(row) = self.current_row() else {
            self.set_flash("NO ROW");
            return;
        };
        let tokens = crate::input::msg_token_candidates(&row.msg);
        if tokens.is_empty() {
            self.pending_chip = false;
            self.pending_exclude = false;
            self.pending_yank = false;
            self.pending_leader = false;
            self.set_flash("NO TOKENS");
        } else {
            self.pending_chip = false;
            self.pending_exclude = false;
            self.pending_yank = false;
            self.pending_leader = false;
            self.open_picker(crate::picker::PickerKind::MsgChip { purpose });
            let picker = self.picker.as_mut().expect("picker just opened");
            picker.enter_new();
            picker.choices = tokens;
        }
    }

    /// Confirm msg-token picker: yank / exclude / open Filter|Highlight ActionList.
    /// Returns [`Some`] text when the caller should yank to the clipboard.
    pub fn confirm_msg_token_picker(&mut self) -> Option<String> {
        use crate::input::{Chip, ChipField};
        use crate::picker::{MsgChipPurpose, PickerKind};
        let Some((purpose, value)) = self.picker.as_ref().and_then(|picker| {
            let PickerKind::MsgChip { purpose } = &picker.kind else {
                return None;
            };
            let purpose = *purpose;
            let visible = crate::picker::PickerSession::filtered_indices(
                &picker.choices,
                picker.draft.as_str(),
            );
            let value = visible
                .get(picker.selected)
                .and_then(|&index| picker.choices.get(index))
                .cloned()
                .or_else(|| (!picker.draft.is_empty()).then(|| picker.draft.to_string()))?;
            Some((purpose, value))
        }) else {
            self.set_flash("NO TOKENS");
            return None;
        };
        match purpose {
            MsgChipPurpose::Yank => {
                self.close_picker();
                Some(value)
            }
            MsgChipPurpose::Chip { exclude: true } => {
                self.close_picker();
                let _ = self.push_exclude_chip(Chip {
                    field: ChipField::Msg,
                    value,
                });
                None
            }
            MsgChipPurpose::Chip { exclude: false } => {
                self.open_msg_action_list(value);
                None
            }
        }
    }

    /// Open post-`cm` ActionList (Filter default, Highlight second).
    pub fn open_msg_action_list(&mut self, value: String) {
        self.open_picker(crate::picker::PickerKind::ActionList { value });
        let picker = self.picker.as_mut().expect("picker just opened");
        picker.choices = vec!["Filter".into(), "Highlight".into()];
        picker.selected = 0;
    }

    /// Confirm ActionList selection for a msg token → Filter or Highlight.
    pub fn confirm_msg_action_list(&mut self) -> bool {
        use crate::input::{Chip, ChipField};
        use crate::picker::{PickerKind, PickerSession};
        let Some((value, choice)) = self.picker.as_ref().and_then(|picker| {
            let PickerKind::ActionList { value } = &picker.kind else {
                return None;
            };
            let visible = PickerSession::contains_indices(&picker.choices, picker.query.as_str());
            let choice = visible
                .get(picker.selected)
                .and_then(|&i| picker.choices.get(i))
                .cloned()?;
            Some((value.clone(), choice))
        }) else {
            return false;
        };
        self.close_picker();
        match choice.as_str() {
            "Filter" => self.push_single_chip_filter(Chip {
                field: ChipField::Msg,
                value,
            }),
            "Highlight" => {
                let Some(group) = HighlightGroup::from_pattern(&value) else {
                    self.set_flash("BAD PATTERN");
                    return false;
                };
                let idx = self.push_or_find_highlight_group(group);
                self.jump_first_match_of(idx);
                self.following = false;
                self.set_flash("HIGHLIGHT");
                true
            }
            _ => false,
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
}
