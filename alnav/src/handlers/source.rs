//! Source surfaces: Dashboard, open-file panel, stream-source panel keys.

use crate::app::App;
use crate::dashboard;
use crate::handlers::dispatch_global_chords;
use crate::handlers::picker::submit::apply_text_field_key;
use crate::live_ctl::{LiveBackend, LiveIngestCtl};
use crate::source_bind::{bind_file_source, bind_live_source};
use crossterm::event::{self, KeyCode};

pub(crate) fn handle_dashboard_key(
    app: &mut App,
    live: &mut Option<LiveIngestCtl>,
    key: event::KeyEvent,
) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL) {
        app.should_quit = true;
        return;
    }
    if dispatch_global_chords(app, key) {
        return;
    }
    let Some(dash) = app.dashboard.as_mut() else {
        return;
    };
    let action = dashboard::handle_key(dash, key.code);
    match action {
        Some(dashboard::DashboardAction::Quit) => app.should_quit = true,
        Some(dashboard::DashboardAction::BindHdc) => {
            if let Err(e) = bind_live_source(app, live, LiveBackend::Hdc, false) {
                app.set_flash(e);
            }
        }
        Some(dashboard::DashboardAction::BindAdb) => {
            if let Err(e) = bind_live_source(app, live, LiveBackend::Adb, false) {
                app.set_flash(e);
            }
        }
        Some(dashboard::DashboardAction::OpenFilePicker) => {
            app.open_file_source_panel();
        }
        Some(dashboard::DashboardAction::OpenRecent(path)) => {
            *live = None;
            if let Err(e) = bind_file_source(app, &path, false) {
                app.set_flash(e);
                // Stay on dashboard; drop missing recent entry.
                app.recent.remove(&path);
                let _ = app.recent.save(&app.config_dir);
                if let Some(d) = app.dashboard.as_mut() {
                    d.recent = app.recent.clone();
                    d.clamp_cursor();
                }
            }
        }
        None => {}
    }
}

pub(crate) fn handle_open_file_panel_key(
    app: &mut App,
    live: &mut Option<LiveIngestCtl>,
    key: event::KeyEvent,
) {
    use crossterm::event::KeyModifiers;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if (key.code == KeyCode::Char('c') && ctrl) || key.code == KeyCode::Esc {
        app.close_open_file_panel();
        return;
    }
    if key.code == KeyCode::Char('r') && ctrl {
        app.log_corpus.refresh();
        app.refresh_open_file_choices();
        app.set_flash("corpus refresh");
        return;
    }

    let switching = app.dashboard.is_none();
    let mut confirm_path: Option<String> = None;
    let mut flash: Option<&'static str> = None;
    let mut need_refresh = false;
    {
        let Some(panel) = app.open_file_panel.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Enter => {
                if let Some(path) = panel
                    .choices
                    .get(panel.selected)
                    .and_then(|c| c.path_to_open())
                {
                    confirm_path = Some(path.display().to_string());
                } else {
                    flash = Some("NO FILE");
                }
            }
            // Arrows only — `j`/`k` must type into the draft.
            KeyCode::Up => panel.move_sel(-1),
            KeyCode::Down => panel.move_sel(1),
            code => {
                if apply_text_field_key(&mut panel.draft, code, ctrl) {
                    need_refresh = true;
                }
            }
        }
    }
    if need_refresh {
        app.refresh_open_file_choices();
    }
    if let Some(msg) = flash {
        app.set_flash(msg);
    }
    if let Some(path) = confirm_path {
        *live = None;
        if let Err(e) = bind_file_source(app, &path, switching) {
            app.set_flash(e);
        }
    }
}

pub(crate) fn handle_stream_source_panel_key(
    app: &mut App,
    live: &mut Option<LiveIngestCtl>,
    key: event::KeyEvent,
) {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(event::KeyModifiers::CONTROL)
        || key.code == KeyCode::Esc
    {
        app.stream_source_panel = None;
        return;
    }
    let switching = app.dashboard.is_none();
    let bind = {
        let Some(panel) = app.stream_source_panel.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                panel.move_by(1);
                None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                panel.move_by(-1);
                None
            }
            KeyCode::Char('h') => Some(LiveBackend::Hdc),
            KeyCode::Char('a') => Some(LiveBackend::Adb),
            KeyCode::Enter => {
                if panel.is_hdc() {
                    Some(LiveBackend::Hdc)
                } else {
                    Some(LiveBackend::Adb)
                }
            }
            _ => None,
        }
    };
    if let Some(backend) = bind {
        if let Err(e) = bind_live_source(app, live, backend, switching) {
            app.set_flash(e);
        }
    }
}
