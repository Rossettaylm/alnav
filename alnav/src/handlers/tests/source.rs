//! Dashboard / source-panel / Ctrl-L / source-chord dispatch tests.

use crate::app::App;
use crate::handlers::handle_normal_event;
use crate::handlers::normal::handle_normal_key;
use crate::handlers::source::handle_dashboard_key;
use crate::{app, dashboard, model, recent};
use crossterm::event::KeyCode;
use crossterm::event::{KeyEvent, KeyModifiers};

#[test]
fn dashboard_invalid_last_recent_is_removed_and_cursor_is_clamped() {
    let dir = tempfile::TempDir::new().unwrap();
    let valid = dir.path().join("valid.log");
    std::fs::write(&valid, "line\n").unwrap();
    let missing = dir.path().join("missing.log");
    let recent = recent::RecentFiles {
        paths: vec![valid.display().to_string(), missing.display().to_string()],
    };
    let mut app = App::new(100);
    app.config_dir = dir.path().to_path_buf();
    app.recent = recent.clone();
    app.dashboard = Some(dashboard::DashboardState::new(recent));
    app.dashboard.as_mut().unwrap().cursor = 4;
    let mut live = None;

    handle_dashboard_key(
        &mut app,
        &mut live,
        KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
    );

    let dash = app.dashboard.as_ref().unwrap();
    assert_eq!(dash.cursor, 3);
    assert_eq!(dash.recent.paths, vec![valid.display().to_string()]);
    assert!(matches!(
        dash.selected(),
        Some(dashboard::DashboardItem::Recent { path, .. })
            if path == valid.display().to_string()
    ));
    assert!(app.status_msg.is_some());
}

#[test]
fn dashboard_ctrl_c_still_quits() {
    let mut app = App::new(100);
    app.dashboard = Some(dashboard::DashboardState::new(
        recent::RecentFiles::default(),
    ));
    let mut live = None;

    handle_dashboard_key(
        &mut app,
        &mut live,
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
    );

    assert!(app.should_quit);
}

#[test]
fn ctrl_l_clears_buffers_in_hdc_loglist() {
    use crate::export::ExportSource;
    use std::sync::mpsc;

    let mut app = App::new(100);
    app.export_source = ExportSource::Hdc { device: None };
    app.focus = app::Focus::LogList;

    let (tx, rx) = mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.rows().len(), 1);

    crate::action::dispatch(&mut app, crate::keymap::ActionId::LogListClearLive);
    assert!(app.rows().is_empty());
    assert!(app.following);
    assert_eq!(app.status_msg.as_deref(), Some("CLEARED"));
}

#[test]
fn ctrl_l_clears_buffers_in_adb_loglist() {
    use crate::export::ExportSource;
    use std::sync::mpsc;

    let mut app = App::new(100);
    app.export_source = ExportSource::Adb { device: None };
    let (tx, rx) = mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);

    crate::action::dispatch(&mut app, crate::keymap::ActionId::LogListClearLive);
    assert!(app.rows().is_empty());
    assert_eq!(app.status_msg.as_deref(), Some("CLEARED"));
}

#[test]
fn ctrl_l_noop_in_file_mode() {
    use crate::export::ExportSource;
    use std::sync::mpsc;

    let mut app = App::new(100);
    app.export_source = ExportSource::File("app.log".into());
    let (tx, rx) = mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    crate::action::dispatch(&mut app, crate::keymap::ActionId::LogListClearLive);
    assert_eq!(app.rows().len(), 1);
    assert_ne!(app.status_msg.as_deref(), Some("CLEARED"));
}

#[test]
fn ctrl_l_noop_when_detail_open() {
    use crate::export::ExportSource;
    use std::sync::mpsc;

    let mut app = App::new(100);
    app.export_source = ExportSource::Hdc { device: None };
    let (tx, rx) = mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I T   : a").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    app.toggle_detail_fields();
    assert!(app.detail_open());

    crate::action::dispatch(&mut app, crate::keymap::ActionId::LogListClearLive);
    assert_eq!(app.rows().len(), 1);
    assert_ne!(app.status_msg.as_deref(), Some("CLEARED"));
}

#[test]
fn ctrl_f_opens_file_and_ctrl_g_opens_stream() {
    let mut app = App::new(100);

    handle_normal_event(
        &mut app,
        KeyEvent::new(
            KeyCode::Char('o'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ),
    );
    assert!(
        app.open_file_panel.is_none(),
        "C-S-o is no longer Open File"
    );
    handle_normal_event(
        &mut app,
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
    );
    assert!(app.open_file_panel.is_some());
    assert!(app.stream_source_panel.is_none());

    handle_normal_event(
        &mut app,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
    );
    assert!(app.open_file_panel.is_none());
    assert!(app.stream_source_panel.is_some());
}

#[test]
fn dashboard_ctrl_f_opens_file_and_ctrl_g_opens_stream() {
    let mut app = App::new(100);
    app.dashboard = Some(dashboard::DashboardState::new(
        recent::RecentFiles::default(),
    ));
    let mut live = None;
    handle_dashboard_key(
        &mut app,
        &mut live,
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
    );
    assert!(app.open_file_panel.as_ref().is_some());
    assert!(app.stream_source_panel.is_none());

    handle_dashboard_key(
        &mut app,
        &mut live,
        KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
    );
    assert!(app.open_file_panel.is_none());
    assert!(app.stream_source_panel.as_ref().is_some());
}

#[test]
fn legacy_of_os_and_space_wo_no_longer_open_source_or_preset() {
    let mut app = App::new(100);

    handle_normal_key(&mut app, KeyCode::Char('o'));
    handle_normal_key(&mut app, KeyCode::Char('f'));
    assert!(app.open_file_panel.is_none());
    handle_normal_key(&mut app, KeyCode::Char('o'));
    handle_normal_key(&mut app, KeyCode::Char('s'));
    assert!(app.stream_source_panel.is_none());
    handle_normal_key(&mut app, KeyCode::Char(' '));
    handle_normal_key(&mut app, KeyCode::Char('w'));
    assert!(app.preset_name.is_none());
    handle_normal_key(&mut app, KeyCode::Char(' '));
    handle_normal_key(&mut app, KeyCode::Char('o'));
    assert!(app.picker.is_none());
}
