use super::*;
use crate::bookmark::ComparePanel;

/// A key must reach the surface that is painted on top. Detail can stay open
/// while Summary/Hist open over it; the panel that owns keys must win.
#[test]
fn summary_outranks_open_detail_overlay() {
    let mut app = App::new(100);
    app.toggle_detail_fields();
    assert_eq!(app.surface(), Surface::Detail);
    app.open_summary_panel();
    assert_eq!(app.surface(), Surface::Summary);
    app.close_summary_panel();
    assert_eq!(app.surface(), Surface::Detail);
}

#[test]
fn source_panel_outranks_dashboard_and_picker() {
    let mut app = App::new(100);
    app.dashboard = Some(crate::dashboard::DashboardState::new(
        crate::recent::RecentFiles::default(),
    ));
    assert_eq!(app.surface(), Surface::Dashboard);
    app.open_stream_source_panel();
    assert_eq!(app.surface(), Surface::StreamSource);
    app.close_source_panels();
    app.dashboard = None;
    app.open_unified_picker();
    assert_eq!(app.surface(), Surface::Picker);
}

#[test]
fn dialog_layers_above_any_surface_and_blocks_palette_and_help() {
    let mut app = App::new(100);
    app.compare = Some(ComparePanel::new());
    assert_eq!(app.surface(), Surface::Compare);
    assert!(!app.command_palette_available());
    app.compare = None;
    assert!(app.command_palette_available());
    assert!(crate::help::help_available(&app));

    app.confirm = Some(crate::picker::ConfirmKind::ClearAll);
    assert_eq!(app.dialog(), Some(Dialog::Confirm));
    assert_eq!(app.surface(), Surface::Main);
    assert!(!app.command_palette_available());
    assert!(!crate::help::help_available(&app));
}

#[test]
fn armed_chord_blocks_help_but_not_palette() {
    let mut app = App::new(100);
    app.begin_lock_from_cursor();
    assert_eq!(app.pending, Pending::Lock);
    assert!(!crate::help::help_available(&app));
    assert!(app.command_palette_available());
    app.open_command_palette();
    assert_eq!(app.pending, Pending::None);
    assert_eq!(app.surface(), Surface::CommandPalette);
}
