//! Source binding (file / live) and per-file session persist & restore.

use crate::app::App;
use crate::live_ctl::{spawn_live_ctl, LiveBackend, LiveIngestCtl};
use crate::{export, session, store};

/// Persist the currently bound file's session. Silent: callers on the teardown
/// path have no visible status bar.
pub(crate) fn persist_current_file(app: &App) {
    if app.session_restore_disabled {
        return;
    }
    let export::ExportSource::File(path) = &app.export_source else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let path = std::path::PathBuf::from(path);
    if let Some(file) = app.capture_session(&path) {
        let _ = session::save(&app.config_dir, &path, &file);
    }
}

/// Like [`persist_current_file`], but surfaces write failures (source-switch path,
/// where the user is still looking at the status bar).
fn save_session_or_flash(app: &mut App) {
    if app.session_restore_disabled {
        return;
    }
    let export::ExportSource::File(path) = app.export_source.clone() else {
        return;
    };
    if path.is_empty() {
        return;
    }
    let path = std::path::PathBuf::from(path);
    if let Some(file) = app.capture_session(&path) {
        if let Err(e) = session::save(&app.config_dir, &path, &file) {
            app.set_flash(format!("SESSION SAVE: {e}"));
        }
    }
}

/// Load and apply `path`'s session. Returns whether a non-degenerate session
/// was restored (gates the `RESTORED` flash).
fn restore_session(app: &mut App, path: &std::path::Path) -> bool {
    if app.session_restore_disabled {
        return false;
    }
    match session::load(&app.config_dir, path) {
        session::SessionLoad::Missing => false,
        session::SessionLoad::Invalid(reason) => {
            app.set_flash(format!("SESSION fallback: {reason}"));
            false
        }
        session::SessionLoad::Loaded(s) => match app.apply_session(&s) {
            Ok(()) => {
                app.session_restored = s.is_active();
                app.session_restored
            }
            Err(e) => {
                app.set_flash(format!("SESSION fallback: {e}"));
                false
            }
        },
    }
}

/// Short `RESTORED …` summary for the Dashboard-path flash (R10).
fn restored_label(app: &App) -> String {
    let mut parts = vec!["RESTORED".to_string()];
    let filters = app.groups.groups.len();
    if filters > 0 {
        parts.push(format!("{filters} FILTER"));
    }
    let excludes = app.groups.excludes.len();
    if excludes > 0 {
        parts.push(format!("{excludes} EXCLUDE"));
    }
    let highlights = app.highlight_groups.groups.len();
    if highlights > 0 {
        parts.push(format!("{highlights} HL"));
    }
    if app.time_bound.as_ref().is_some_and(|t| t.is_active()) {
        parts.push("TIME".into());
    }
    if !app.bookmarks.is_empty() {
        parts.push(format!("\u{2605}{}", app.bookmarks.len()));
    }
    parts.join(" ")
}

/// Bind a file source. When `switching`, resets non-F/E/H state after open succeeds.
pub(crate) fn bind_file_source(app: &mut App, path: &str, switching: bool) -> Result<(), String> {
    let path_buf = std::path::PathBuf::from(path);
    if !path_buf.exists() {
        return Err(format!("file not found: {path}"));
    }
    if path_buf.is_dir() {
        return Err(format!("not a file: {path}"));
    }
    let file_store = store::FileStore::open(&path_buf).map_err(|e| e.to_string())?;
    let from_dashboard = app.dashboard.is_some();
    if switching {
        // The old file is still bound here — capture it before the reset.
        save_session_or_flash(app);
        app.reset_for_source_switch();
    }
    app.set_file_store(file_store);
    app.export_source = export::ExportSource::File(path_buf.display().to_string());
    app.dashboard = None;
    app.close_source_panels();
    // R2: only the first bind of this session restores; `C-f` mid-session does not.
    let restored = !switching && restore_session(app, &path_buf);
    app.record_recent_file(&path_buf);
    if app.filter_active() {
        app.rebuild_visible();
    }
    if restored && from_dashboard {
        app.set_flash(restored_label(app));
    }
    Ok(())
}

/// Bind HDC/ADB. Clears `time_bound` (live has no interactive time window).
pub(crate) fn bind_live_source(
    app: &mut App,
    live: &mut Option<LiveIngestCtl>,
    backend: LiveBackend,
    switching: bool,
) -> Result<(), String> {
    let ctl = spawn_live_ctl(backend, None)?;
    if switching {
        // Leaving a file for a live source still persists that file's session.
        save_session_or_flash(app);
        *live = None;
        app.reset_for_source_switch();
    }
    app.time_bound = None;
    app.export_source = match backend {
        LiveBackend::Hdc => export::ExportSource::Hdc { device: None },
        LiveBackend::Adb => export::ExportSource::Adb { device: None },
    };
    *live = Some(ctl);
    app.dashboard = None;
    app.close_source_panels();
    app.ingest_done = false;
    if app.filter_active() {
        app.rebuild_visible();
    }
    Ok(())
}

#[cfg(test)]
mod tests;
