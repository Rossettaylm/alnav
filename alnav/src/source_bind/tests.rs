//! Tests for session restore / persist across source binds.

use super::*;
use crate::app::App;
use crate::{export, input, session};

fn session_test_app(cfg: &std::path::Path, log: &std::path::Path) -> App {
    let mut app = App::new(100);
    app.config_dir = cfg.to_path_buf();
    app.export_source = export::ExportSource::File(log.display().to_string());
    app
}

fn filtered_snapshot(log: &std::path::Path) -> session::SessionFile {
    let mut snap = session::SessionFile::new(session::normalize_source(log).unwrap());
    snap.filters = vec![session::SessionFilterGroup {
        chips: vec![session::SessionChip {
            field: "tag".into(),
            value: "Restored".into(),
        }],
        enabled: true,
        same_field_op: "and".into(),
    }];
    snap.cursor_row_id = 3;
    snap
}

#[test]
fn restore_session_applies_saved_rules() {
    let cfg = tempfile::tempdir().unwrap();
    let log = cfg.path().join("a.log");
    std::fs::write(&log, "04-02 10:00:00.000 1 1 I T : x\n").unwrap();
    let mut app = session_test_app(cfg.path(), &log);
    session::save(&app.config_dir, &log, &filtered_snapshot(&log)).unwrap();

    assert!(restore_session(&mut app, &log));
    assert_eq!(app.groups.groups.len(), 1);
    assert_eq!(app.groups.groups[0].chips[0].value, "Restored");
    assert!(app.session_restored);
}

#[test]
fn restore_and_persist_are_both_skipped_when_disabled() {
    let cfg = tempfile::tempdir().unwrap();
    let log = cfg.path().join("a.log");
    std::fs::write(&log, "04-02 10:00:00.000 1 1 I T : x\n").unwrap();
    let mut app = session_test_app(cfg.path(), &log);
    let original = filtered_snapshot(&log);
    session::save(&app.config_dir, &log, &original).unwrap();
    app.session_restore_disabled = true;

    assert!(!restore_session(&mut app, &log));
    assert!(app.groups.groups.is_empty(), "must not read the archive");

    // A write attempt with different rules must leave the archive untouched.
    app.groups
        .push_exclude(input::Chip {
            field: input::ChipField::Tag,
            value: "Other".into(),
        })
        .unwrap();
    persist_current_file(&app);
    match session::load(&app.config_dir, &log) {
        session::SessionLoad::Loaded(back) => assert_eq!(back, original),
        other => panic!("archive must survive untouched, got {other:?}"),
    }
}

#[test]
fn degenerate_session_does_not_overwrite_existing_archive() {
    let cfg = tempfile::tempdir().unwrap();
    let log = cfg.path().join("a.log");
    std::fs::write(&log, "04-02 10:00:00.000 1 1 I T : x\n").unwrap();
    let app = session_test_app(cfg.path(), &log);
    let original = filtered_snapshot(&log);
    session::save(&app.config_dir, &log, &original).unwrap();

    // Empty rules, no pins, cursor at top → R6 skips the write.
    persist_current_file(&app);
    match session::load(&app.config_dir, &log) {
        session::SessionLoad::Loaded(back) => assert_eq!(back, original),
        other => panic!("expected the original archive, got {other:?}"),
    }
}

#[test]
fn corrupt_archive_flashes_fallback_and_keeps_file() {
    let cfg = tempfile::tempdir().unwrap();
    let log = cfg.path().join("a.log");
    std::fs::write(&log, "04-02 10:00:00.000 1 1 I T : x\n").unwrap();
    let mut app = session_test_app(cfg.path(), &log);
    let path = session::session_path(cfg.path(), &log);
    std::fs::create_dir_all(session::sessions_dir(cfg.path())).unwrap();
    std::fs::write(&path, "version = {{{").unwrap();

    assert!(!restore_session(&mut app, &log));
    let flash = app.status_msg.clone().unwrap_or_default();
    assert!(flash.starts_with("SESSION fallback:"), "{flash}");
    assert!(path.is_file(), "a corrupt archive is kept, not deleted");
}

#[test]
fn live_source_never_writes_a_session() {
    let cfg = tempfile::tempdir().unwrap();
    let mut app = App::new(100);
    app.config_dir = cfg.path().to_path_buf();
    app.export_source = export::ExportSource::Adb { device: None };
    app.groups
        .push_exclude(input::Chip {
            field: input::ChipField::Tag,
            value: "Spam".into(),
        })
        .unwrap();
    persist_current_file(&app);
    assert!(
        !session::sessions_dir(cfg.path()).exists(),
        "live sessions must not create the sessions dir"
    );
}

#[test]
fn restored_label_summarizes_what_came_back() {
    let cfg = tempfile::tempdir().unwrap();
    let log = cfg.path().join("a.log");
    std::fs::write(&log, "04-02 10:00:00.000 1 1 I T : x\n").unwrap();
    let mut app = session_test_app(cfg.path(), &log);
    let mut snap = filtered_snapshot(&log);
    snap.time_since = Some("10:00:00".into());
    snap.highlights = vec![session::SessionHighlight {
        pattern: "boom".into(),
        enabled: true,
    }];
    app.apply_session(&snap).unwrap();

    let label = restored_label(&app);
    assert!(label.starts_with("RESTORED"), "{label}");
    assert!(label.contains("1 FILTER"), "{label}");
    assert!(label.contains("1 HL"), "{label}");
    assert!(label.contains("TIME"), "{label}");
}

#[test]
fn first_bind_restores_and_switch_bind_does_not() {
    let cfg = tempfile::tempdir().unwrap();
    let a = cfg.path().join("a.log");
    let b = cfg.path().join("b.log");
    std::fs::write(&a, "04-02 10:00:00.000 1 1 I T : x\n").unwrap();
    std::fs::write(&b, "04-02 10:00:00.000 1 1 I T : y\n").unwrap();
    let mut app = App::new(100);
    app.config_dir = cfg.path().to_path_buf();
    session::save(&app.config_dir, &a, &filtered_snapshot(&a)).unwrap();
    // b.log gets a *distinct* archive so applying it would be observable.
    let mut b_snap = session::SessionFile::new(session::normalize_source(&b).unwrap());
    b_snap.lock_pid = Some("999".into());
    b_snap.cursor_row_id = 2;
    session::save(&app.config_dir, &b, &b_snap).unwrap();

    bind_file_source(&mut app, &a.display().to_string(), false).unwrap();
    assert_eq!(
        app.groups.groups.len(),
        1,
        "first bind of the session restores"
    );
    assert!(app.session_restored);

    // `C-f` mid-session must not apply b.log's archive. Note that
    // `reset_for_source_switch` intentionally carries Filter/Exclude/Highlight
    // across a switch, so the lock is the signal to assert on here.
    bind_file_source(&mut app, &b.display().to_string(), true).unwrap();
    assert_eq!(
        app.lock_pid, None,
        "switching binds must not restore the new file's session"
    );
    assert!(!app.session_restored);
    assert_eq!(app.pending_restore_cursor, None);
}

#[test]
fn switching_away_persists_the_leaving_file() {
    let cfg = tempfile::tempdir().unwrap();
    let a = cfg.path().join("a.log");
    let b = cfg.path().join("b.log");
    std::fs::write(&a, "04-02 10:00:00.000 1 1 I T : x\n").unwrap();
    std::fs::write(&b, "04-02 10:00:00.000 1 1 I T : y\n").unwrap();
    let mut app = App::new(100);
    app.config_dir = cfg.path().to_path_buf();

    bind_file_source(&mut app, &a.display().to_string(), false).unwrap();
    app.groups
        .push_exclude(input::Chip {
            field: input::ChipField::Tag,
            value: "Spam".into(),
        })
        .unwrap();

    bind_file_source(&mut app, &b.display().to_string(), true).unwrap();
    match session::load(&app.config_dir, &a) {
        session::SessionLoad::Loaded(back) => {
            assert_eq!(back.excludes.len(), 1);
            assert_eq!(back.excludes[0].chips[0].value, "Spam");
        }
        other => panic!("a.log session should have been saved, got {other:?}"),
    }
    assert!(
        matches!(
            session::load(&app.config_dir, &b),
            session::SessionLoad::Missing
        ),
        "b.log has no archive yet"
    );
}

#[test]
fn switching_from_file_to_live_persists_the_file() {
    let cfg = tempfile::tempdir().unwrap();
    let a = cfg.path().join("a.log");
    std::fs::write(&a, "04-02 10:00:00.000 1 1 I T : x\n").unwrap();
    let mut app = App::new(100);
    app.config_dir = cfg.path().to_path_buf();
    bind_file_source(&mut app, &a.display().to_string(), false).unwrap();
    app.groups
        .push_exclude(input::Chip {
            field: input::ChipField::Tag,
            value: "Spam".into(),
        })
        .unwrap();

    // `bind_live_source` spawns a device child, so exercise the persist hook
    // it runs before `reset_for_source_switch` directly.
    save_session_or_flash(&mut app);
    app.reset_for_source_switch();
    app.export_source = export::ExportSource::Adb { device: None };

    match session::load(&app.config_dir, &a) {
        session::SessionLoad::Loaded(back) => assert_eq!(back.excludes.len(), 1),
        other => panic!("file session should persist on live switch, got {other:?}"),
    }
}
