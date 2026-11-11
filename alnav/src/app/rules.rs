//! Filter/exclude/highlight group CRUD, strip focus/cursor cycling, and unified enable/delete.

use super::{App, Focus, StripKind};
use crate::filter_model::{ExcludeEntry, Group};
use crate::highlight_model::HighlightGroup;

fn group_to_exclude_entry(group: Group) -> Option<ExcludeEntry> {
    if group.chips.len() != 1 {
        return None;
    }
    let chip = group.chips.first()?.clone();
    Some(ExcludeEntry {
        chip,
        enabled: group.enabled,
    })
}

impl App {
    /// Filter/Exclude/Highlight strip is focused and has a selected group.
    pub fn focused_strip_has_selection(&self) -> bool {
        let kind = match self.focus {
            Focus::ChipStrip => StripKind::Filter,
            Focus::ExcludeStrip => StripKind::Exclude,
            Focus::HighlightStrip => StripKind::Highlight,
            _ => return false,
        };
        self.strip_len(kind) > 0
    }

    /// Enable the group if needed, make it the n/N target, jump to the first hit.
    /// Other enabled highlights stay painted. Does not set view-focus.
    pub fn activate_highlight_group(&mut self, index: usize) -> bool {
        let Some(group) = self.highlight_groups.groups.get_mut(index) else {
            return false;
        };
        if !group.enabled {
            group.enabled = true;
        }
        self.active_highlight = Some(index);
        self.match_stats_stale = true;
        if self.store.is_file() {
            self.restart_highlight_scan();
        }
        self.jump_first_match_of(index)
    }

    /// Whether any Filter / Highlight / Exclude group exists (enabled or not).
    pub fn has_rules(&self) -> bool {
        !self.groups.groups.is_empty()
            || !self.groups.excludes.is_empty()
            || !self.highlight_groups.groups.is_empty()
    }

    /// Delete every Filter / Highlight / Exclude group. Does not touch lock,
    /// time, search, or bookmarks. Closes the picker and returns to LogList.
    pub fn clear_all_rules(&mut self) {
        let keep_id = self.current_row().map(|r| r.row_id);
        self.following = false;
        self.groups.groups.clear();
        self.groups.excludes.clear();
        self.highlight_groups.groups.clear();
        self.group_cursor = 0;
        self.exclude_cursor = 0;
        self.highlight_cursor = 0;
        self.active_highlight = None;
        self.rebuild_visible();
        if let Some(id) = keep_id {
            if let Some(vis) = self.visible_idx_for_row_id(id) {
                self.cursor = vis;
            }
        }
        self.match_stats_stale = true;
        if self.store.is_file() {
            self.restart_highlight_scan();
        }
        self.confirm = None;
        self.close_picker();
        self.set_flash("ALL CLEARED");
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

    /// Read `enabled` of a unified Manage item; `None` when the index is stale.
    pub fn unified_enabled(&self, id: crate::picker::UnifiedId) -> Option<bool> {
        use crate::picker::UnifiedKind;
        match id.kind {
            UnifiedKind::Filter => self.groups.groups.get(id.source_index).map(|g| g.enabled),
            UnifiedKind::Highlight => self
                .highlight_groups
                .groups
                .get(id.source_index)
                .map(|g| g.enabled),
            UnifiedKind::Exclude => self.groups.excludes.get(id.source_index).map(|e| e.enabled),
        }
    }

    /// Force `enabled` on many unified items, rebuilding once at the end.
    /// Returns how many items actually changed state.
    pub fn set_unified_enabled_bulk(
        &mut self,
        ids: &[crate::picker::UnifiedId],
        enabled: bool,
    ) -> usize {
        use crate::picker::UnifiedKind;
        let mut changed = 0usize;
        let mut rows_dirty = false;
        let mut highlight_dirty = false;
        for id in ids {
            let slot = match id.kind {
                UnifiedKind::Filter => self
                    .groups
                    .groups
                    .get_mut(id.source_index)
                    .map(|g| &mut g.enabled),
                UnifiedKind::Highlight => self
                    .highlight_groups
                    .groups
                    .get_mut(id.source_index)
                    .map(|g| &mut g.enabled),
                UnifiedKind::Exclude => self
                    .groups
                    .excludes
                    .get_mut(id.source_index)
                    .map(|e| &mut e.enabled),
            };
            let Some(slot) = slot else { continue };
            if *slot == enabled {
                continue;
            }
            *slot = enabled;
            changed += 1;
            match id.kind {
                UnifiedKind::Filter | UnifiedKind::Exclude => rows_dirty = true,
                UnifiedKind::Highlight => highlight_dirty = true,
            }
        }
        if highlight_dirty {
            self.match_stats_stale = true;
            if self.store.is_file() {
                self.restart_highlight_scan();
            }
        }
        if rows_dirty {
            self.rebuild_visible();
        }
        changed
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
}
