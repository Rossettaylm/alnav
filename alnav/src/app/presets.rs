//! Preset save/open/apply/rename/delete flows.

use super::App;

impl App {
    /// Begin Ctrl-S save: open name dialog, or flash if nothing to capture.
    pub fn begin_preset_save(&mut self) {
        if !crate::preset::has_savable_rules(&self.groups, &self.highlight_groups) {
            self.set_flash("NO RULES TO SAVE");
            return;
        }
        self.clear_pending_all();
        self.close_picker();
        self.preset_name = Some(crate::preset::PresetNameDialog::save());
    }

    /// Begin Ctrl-O open: load catalog; empty → flash; else Manage picker.
    pub fn begin_preset_open(&mut self) {
        let (list, skipped) = crate::preset::list(&self.config_dir);
        if list.is_empty() {
            if skipped > 0 {
                self.set_flash(&format!("NO PRESETS ({skipped} INVALID)"));
            } else {
                self.set_flash("NO PRESETS");
            }
            return;
        }
        self.preset_catalog = list;
        if skipped > 0 {
            self.set_flash(&format!("SKIPPED {skipped} INVALID"));
        }
        self.open_picker(crate::picker::PickerKind::Preset);
    }

    /// Replace Filter/Exclude/Highlight from preset; keep time/lock/bookmarks/search.
    /// `following=false`; retain current row when still visible.
    pub fn apply_preset(&mut self, preset: &crate::preset::Preset) -> Result<(), String> {
        let (filters, excludes, highlights) = crate::preset::apply_lists(preset)?;
        let keep_id = self.current_row().map(|r| r.row_id);
        self.following = false;
        self.groups.groups = filters;
        self.groups.excludes = excludes;
        self.highlight_groups.groups = highlights;
        self.group_cursor = 0;
        self.exclude_cursor = 0;
        self.highlight_cursor = 0;
        self.active_highlight = self.highlight_groups.groups.iter().position(|g| g.enabled);
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
        Ok(())
    }

    pub fn refresh_preset_catalog(&mut self) {
        let (list, _) = crate::preset::list(&self.config_dir);
        self.preset_catalog = list;
    }

    /// Commit the open name dialog (save or rename). Returns true when dialog closed.
    pub fn submit_preset_name(&mut self, force_overwrite: bool) -> bool {
        let Some(dialog) = self.preset_name.as_ref() else {
            return false;
        };
        let name = dialog.field.as_str().to_string();
        if let Err(e) = crate::preset::validate_name(&name) {
            self.set_flash(&e);
            return false;
        }
        let purpose = dialog.purpose.clone();
        let exists = crate::preset::exists(&self.config_dir, &name);
        match &purpose {
            crate::preset::PresetNamePurpose::Rename { from } if from == &name => {
                self.preset_name = None;
                return true;
            }
            _ => {}
        }
        if exists && !force_overwrite {
            if let Some(d) = self.preset_name.as_mut() {
                d.confirm_overwrite = true;
            }
            return false;
        }
        let is_save = matches!(purpose, crate::preset::PresetNamePurpose::Save);
        let result = match &purpose {
            crate::preset::PresetNamePurpose::Save => {
                match crate::preset::capture(&self.groups, &self.highlight_groups, &name) {
                    Ok(None) => {
                        self.set_flash("NO RULES TO SAVE");
                        Err(())
                    }
                    Ok(Some(preset)) => {
                        crate::preset::save(&self.config_dir, &preset).map_err(|e| {
                            self.set_flash(&e);
                        })
                    }
                    Err(e) => {
                        self.set_flash(&e);
                        Err(())
                    }
                }
            }
            crate::preset::PresetNamePurpose::Rename { from } => {
                crate::preset::rename(&self.config_dir, from, &name).map_err(|e| {
                    self.set_flash(&e);
                })
            }
        };
        if result.is_ok() {
            self.preset_name = None;
            self.refresh_preset_catalog();
            if is_save {
                self.set_flash("PRESET SAVED");
            } else {
                self.set_flash("PRESET RENAMED");
                if let Some(session) = self.picker.as_mut() {
                    if matches!(session.kind, crate::picker::PickerKind::Preset) {
                        session.selected = self
                            .preset_catalog
                            .iter()
                            .position(|p| p.name == name)
                            .unwrap_or(0);
                    }
                }
            }
            true
        } else {
            false
        }
    }

    pub fn delete_preset_named(&mut self, name: &str) -> bool {
        match crate::preset::delete(&self.config_dir, name) {
            Ok(()) => {
                self.refresh_preset_catalog();
                true
            }
            Err(e) => {
                self.set_flash(&e);
                false
            }
        }
    }
}
