//! File-backed store tests.

use super::*;
use crate::filter_model::Group;
use std::io::Write;

fn write_temp(contents: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(contents.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

#[test]
fn file_store_full_browse_no_max_lines_cap() {
    let mut body = String::new();
    for i in 0..50 {
        body.push_str(&format!(
            "04-02 10:00:{i:02}.000  1  1 I Tag{i}   : line{i}\n"
        ));
    }
    let f = write_temp(&body);
    let mut app = App::new(10); // max_lines would cap stream; ignored for file
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    assert_eq!(app.index.visible.len(), 50);
    app.jump_bottom();
    assert_eq!(app.cursor, 49);
    assert_eq!(app.row_at(0).unwrap().tag, "Tag0");
    assert_eq!(app.row_at(49).unwrap().tag, "Tag49");
}

/// Regression: `-f` filter scan must honor `fe` (ViewFocus.severe).
#[test]
fn test_view_focus_fe_file_mode_keeps_severe() {
    let f = write_temp(
        "04-02 10:00:00.000  1  1 I Tag     : hit one\n\
         04-02 10:00:01.000  1  1 E Tag     : err\n\
         04-02 10:00:02.000  1  1 I Tag     : other\n",
    );
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    assert_eq!(app.index.visible.len(), 3);

    app.toggle_view_focus(ViewFocusKind::Severe);
    assert!(app.view_focus.severe);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.ingest_done {
        if Instant::now() > deadline {
            panic!("filter timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        app.index.visible.len(),
        1,
        "fe in file mode should keep the E/F (severe) row"
    );
    assert_eq!(app.row_at(0).unwrap().msg, "err");
}

#[test]
fn file_filter_uses_subset_not_matched_buffer() {
    let f = write_temp(
        "04-02 10:00:00.000  1  1 I Keep   : a\n\
         04-02 10:00:01.000  1  1 I Drop   : b\n\
         04-02 10:00:02.000  1  1 I Keep   : c\n",
    );
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    app.groups.groups.push(Group {
        label: "tag~Keep".into(),
        chips: vec![crate::input::Chip {
            field: crate::input::ChipField::Tag,
            value: "Keep".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.rebuild_visible();
    // Drain bg filter to completion.
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.ingest_done {
        if Instant::now() > deadline {
            panic!("filter timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(app.index.visible, Visible::Subset(_)));
    assert_eq!(app.index.visible.len(), 2);
    assert!(app.matched().is_empty());
    assert_eq!(app.row_at(0).unwrap().tag, "Keep");
    assert_eq!(app.row_at(1).unwrap().tag, "Keep");
}

fn wait_file_filter(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.ingest_done {
        if Instant::now() > deadline {
            panic!("filter timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn file_pin_survives_filter_in_line_order() {
    let f = write_temp(
        "04-02 10:00:00.000  1  1 I Keep   : a\n\
         04-02 10:00:01.000  1  1 I Drop   : b\n\
         04-02 10:00:02.000  1  1 I Keep   : c\n",
    );
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    app.cursor = 1;
    app.bookmark_add_current();
    app.groups.groups.push(Group {
        label: "tag~Keep".into(),
        chips: vec![crate::input::Chip {
            field: crate::input::ChipField::Tag,
            value: "Keep".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.rebuild_visible();
    wait_file_filter(&mut app);
    assert!(matches!(app.index.visible, Visible::Subset(_)));
    assert_eq!(app.index.visible.len(), 3);
    assert_eq!(app.row_at(0).unwrap().tag, "Keep");
    assert_eq!(app.row_at(1).unwrap().tag, "Drop");
    assert_eq!(app.row_at(2).unwrap().tag, "Keep");
    let pin_id = app.bookmarks.items[0].row_id();
    app.cursor = app.visible_idx_for_row_id(pin_id).unwrap();
    app.bookmark_remove_current();
    assert_eq!(app.index.visible.len(), 2);
    assert_eq!(app.row_at(0).unwrap().tag, "Keep");
    assert_eq!(app.row_at(1).unwrap().tag, "Keep");
}

#[test]
fn file_unparseable_kept_as_raw() {
    let f = write_temp("not a log\n04-02 10:00:00.000  1  1 I TagA   : ok\n");
    let mut app = App::new(100);
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    assert_eq!(app.index.visible.len(), 2);
    assert_eq!(app.row_at(0).unwrap().raw, "not a log");
    assert!(!app.row_at(0).unwrap().is_parsed());
    assert_eq!(app.row_at(1).unwrap().tag, "TagA");
}

#[test]
fn file_unparseable_rejected_under_any_filter() {
    let f = write_temp(
        "================= QQXlog open =================\n\
         04-02 10:00:00.000  1  1 I TagA   : ok\n\
         04-02 10:00:01.000  1  1 W TagB   : warn\n",
    );
    let mut app = App::new(100);
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    assert_eq!(app.index.visible.len(), 3);

    // msg filter (would substring-match the header if unparsed were allowed)
    app.groups.groups.push(Group {
        label: "msg~QQXlog".into(),
        chips: vec![crate::input::Chip {
            field: crate::input::ChipField::Msg,
            value: "QQXlog".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.rebuild_visible();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.ingest_done {
        if Instant::now() > deadline {
            panic!("filter timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        app.index.visible.len(),
        0,
        "unparsed must not pass any active filter"
    );

    // Clear include; time window alone must also drop unparsed
    app.groups.groups.clear();
    app.time_bound = Some(crate::filter_model::TimeBound {
        since: Some("04-02 10:00:00".into()),
        until: None,
    });
    app.rebuild_visible();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.ingest_done {
        if Instant::now() > deadline {
            panic!("time filter timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.index.visible.len(), 2);
    assert!(app.row_at(0).unwrap().is_parsed());
    assert_eq!(app.row_at(0).unwrap().tag, "TagA");
}

#[test]
fn row_passes_filters_rejects_unparsed_when_active() {
    let mut app = App::new(100);
    let raw = EntryRow::from_line_or_raw("QQXlog header");
    assert!(!raw.is_parsed());
    assert!(
        app.row_passes_filters(&raw),
        "no filter → unparsed still eligible for vacuous pass"
    );
    app.groups.groups.push(Group {
        label: "level".into(),
        chips: vec![crate::input::Chip {
            field: crate::input::ChipField::Level,
            value: "I".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.rebuild_visible();
    assert!(app.filter_active());
    assert!(!app.row_passes_filters(&raw));
}

#[test]
fn file_visible_idx_for_row_id_uses_line_mapping() {
    let f = write_temp(
        "04-02 10:00:00.000  1  1 I Keep   : a\n\
         04-02 10:00:01.000  1  1 I Drop   : b\n\
         04-02 10:00:02.000  1  1 I Keep   : c\n",
    );
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    // Unfiltered: row_id 3 → visible 2
    assert_eq!(app.visible_idx_for_row_id(3), Some(2));
    app.groups.groups.push(Group {
        label: "tag~Keep".into(),
        chips: vec![crate::input::Chip {
            field: crate::input::ChipField::Tag,
            value: "Keep".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.rebuild_visible();
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.ingest_done {
        if Instant::now() > deadline {
            panic!("filter timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(5));
    }
    // Filtered Subset [0, 2]: row_id 3 (line 2) → visible 1
    assert_eq!(app.visible_idx_for_row_id(3), Some(1));
    assert_eq!(app.visible_idx_for_row_id(2), None); // Drop filtered out
    assert_eq!(app.jump_to_bookmark(3), JumpResult::Ok);
    assert_eq!(app.cursor, 1);
}

fn wait_highlight_done(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.highlight_scan.done {
        if Instant::now() > deadline {
            panic!("highlight scan timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn file_highlight_scan_stats_and_n_without_ui_full_parse() {
    use crate::highlight_model::HighlightGroup;

    let f = write_temp(
        "04-02 10:00:00.000  1  1 I T   : aaa\n\
         04-02 10:00:01.000  1  1 I T   : hit one\n\
         04-02 10:00:02.000  1  1 I T   : bbb\n\
         04-02 10:00:03.000  1  1 I T   : hit two\n",
    );
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    app.following = false;
    app.cursor = 0;
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
    wait_highlight_done(&mut app);
    assert_eq!(app.highlight_scan.hits, vec![1, 3]);
    assert_eq!(app.highlight_match_stats(), Some((None, 2)));
    assert_eq!(app.find_match(1), FindJumpResult::Moved);
    assert_eq!(app.cursor, 1);
    assert_eq!(app.highlight_match_stats(), Some((Some(1), 2)));
    assert_eq!(app.find_match(1), FindJumpResult::Moved);
    assert_eq!(app.cursor, 3);
    assert_eq!(app.find_match(1), FindJumpResult::NoMore); // no wrap
    assert_eq!(app.cursor, 3);
    assert!(app
        .log_loading_label()
        .is_none_or(|s| !s.contains("Highlight")));
}

#[test]
fn file_highlight_scan_cancels_on_filter_change() {
    use crate::highlight_model::HighlightGroup;

    let f = write_temp(
        "04-02 10:00:00.000  1  1 I Keep   : hit a\n\
         04-02 10:00:01.000  1  1 I Drop   : hit b\n\
         04-02 10:00:02.000  1  1 I Keep   : other\n",
    );
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    app.push_or_find_highlight_group(HighlightGroup::from_pattern("hit").unwrap());
    wait_highlight_done(&mut app);
    assert_eq!(app.highlight_scan.hits, vec![0, 1]);
    let gen_before = app.highlight_scan.gen;

    app.groups.groups.push(Group {
        label: "tag~Keep".into(),
        chips: vec![crate::input::Chip {
            field: crate::input::ChipField::Tag,
            value: "Keep".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.rebuild_visible();
    assert_ne!(app.highlight_scan.gen, gen_before);
    let deadline = Instant::now() + Duration::from_secs(2);
    while !app.ingest_done {
        if Instant::now() > deadline {
            panic!("filter timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(5));
    }
    wait_highlight_done(&mut app);
    // Vis domain is Keep rows only: line 0 (hit) and line 2 (no hit) → one hit at vis 0.
    assert_eq!(app.highlight_scan.hits, vec![0]);
    assert_eq!(app.index.visible.len(), 2);
}

#[test]
fn file_find_severe_uses_cache_not_full_visible_parse() {
    let f = write_temp(
        "04-02 10:00:00.000  1  1 I T   : ok\n\
         04-02 10:00:01.000  1  1 E T   : err\n\
         04-02 10:00:02.000  1  1 I T   : ok2\n",
    );
    let mut app = App::new(100);
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    // Wait for severe prefetch to fill cache for all lines.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let done = app
            .index
            .store
            .as_file()
            .map(|f| f.progress().severe_done)
            .unwrap_or(true);
        if done {
            break;
        }
        if Instant::now() > deadline {
            panic!("severe prefetch timed out");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        app.index.store.as_file().unwrap().severe_cached(1),
        Some(true)
    );
    app.following = false;
    app.cursor = 0;
    assert_eq!(app.find_severe(1), FindJumpResult::Moved);
    assert_eq!(app.cursor, 1);
    assert!(app.log_loading_label().is_none());
}

#[test]
fn file_log_loading_label_during_filter() {
    let mut body = String::new();
    for i in 0..200 {
        body.push_str(&format!("04-02 10:00:00.000  1  1 I Tag{i}   : line{i}\n"));
    }
    let f = write_temp(&body);
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    // Async open so filter can still be in-flight when we sample the label.
    app.set_file_store(FileStore::open(f.path()).unwrap());
    app.groups.groups.push(Group {
        label: "tag~Tag1".into(),
        chips: vec![crate::input::Chip {
            field: crate::input::ChipField::Tag,
            value: "Tag1".into(),
        }],
        enabled: true,
        same_field_op: crate::fuzzy::SameFieldOp::And,
    });
    app.rebuild_visible();
    // Either indexing or filtering should produce a loading label.
    let mut saw_loading = false;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        app.poll_file_store();
        if let Some(label) = app.log_loading_label() {
            assert!(
                label.contains("Indexing")
                    || label.contains("Filtering")
                    || label.contains("Highlight"),
                "unexpected label {label}"
            );
            saw_loading = true;
            break;
        }
        if app.ingest_done {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        saw_loading || app.ingest_done,
        "expected loading or quick finish"
    );
}

#[test]
fn summary_panel_file_async_loading_then_ready() {
    let mut body = String::new();
    for i in 0..20_000 {
        let level = if i % 500 == 0 { 'E' } else { 'I' };
        body.push_str(&format!(
            "04-02 10:00:00.000  1  1 {level} Tag{}   : line{i}\n",
            i % 5
        ));
    }
    let f = write_temp(&body);
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    // Async open: index still growing when the panel opens (async path).
    app.set_file_store(FileStore::open(f.path()).unwrap());
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.ingest_done {
        if Instant::now() > deadline {
            panic!("index timed out");
        }
        app.poll_file_store();
        std::thread::sleep(Duration::from_millis(2));
    }

    app.open_summary_panel();
    assert!(matches!(app.summary_view, SummaryView::Loading));
    app.flush_summary_job(Duration::from_secs(10));
    let SummaryView::Ready(report) = &app.summary_view else {
        panic!("expected Ready after flush");
    };
    assert_eq!(report.total, 20_000);
    assert_eq!(report.matched, 20_000);
    assert_eq!(report.crashes, 0);
    assert_eq!(report.top_tags.len(), 5);
    assert!(report.levels.get(&'I').copied().unwrap_or(0) > 0);
    assert!(report.levels.get(&'E').copied().unwrap_or(0) > 0);
}

// ---- File session restore (09-15-file-session-restore) ----

fn session_file_app(body: &str) -> (App, tempfile::NamedTempFile, tempfile::TempDir) {
    let f = write_temp(body);
    let cfg = tempfile::tempdir().unwrap();
    let mut app = App::new(100);
    app.config_dir = cfg.path().to_path_buf();
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    (app, f, cfg)
}

fn five_line_body() -> String {
    let mut body = String::new();
    for i in 0..5 {
        body.push_str(&format!(
            "04-02 10:00:0{i}.000  100  200 I Tag{i}   : line{i}\n"
        ));
    }
    body
}

fn chip(field: crate::input::ChipField, value: &str) -> crate::input::Chip {
    crate::input::Chip {
        field,
        value: value.to_string(),
    }
}

#[test]
fn capture_apply_roundtrip_preserves_every_persisted_field() {
    let (mut app, f, _cfg) = session_file_app(&five_line_body());
    let mut disabled = crate::input::build_group_from_chips(
        vec![chip(crate::input::ChipField::Msg, "line9")],
        true,
    )
    .unwrap()
    .unwrap();
    disabled.enabled = false;
    app.groups.groups.push(
        crate::input::build_group_from_chips(
            vec![chip(crate::input::ChipField::Tag, "Tag1")],
            true,
        )
        .unwrap()
        .unwrap(),
    );
    app.groups.groups.push(disabled);
    app.groups
        .push_exclude(chip(crate::input::ChipField::Tag, "Spam"))
        .unwrap();
    app.groups.excludes[0].enabled = false;
    app.highlight_groups
        .groups
        .push(HighlightGroup::from_pattern("boom").unwrap());
    app.highlight_groups.groups[0].enabled = false;
    app.active_highlight = Some(0);
    app.lock_pid = Some("100".into());
    app.time_bound = Some(TimeBound {
        since: Some("10:00:01".into()),
        until: None,
    });
    app.view_focus = ViewFocus {
        highlight: false,
        severe: true,
    };
    app.cursor = 3;
    app.list_offset = 2;
    let pinned_id = app.current_row().unwrap().row_id;
    app.bookmark_add_current();

    let snap = app.capture_session(f.path()).expect("active session");
    assert_eq!(snap.filters.len(), 2);
    assert!(snap.filters[0].enabled);
    assert!(!snap.filters[1].enabled, "disabled group must round-trip");
    assert_eq!(snap.excludes.len(), 1);
    assert!(!snap.excludes[0].enabled);
    assert_eq!(snap.highlights.len(), 1);
    assert!(!snap.highlights[0].enabled);
    assert_eq!(snap.active_highlight, Some(0));
    assert_eq!(snap.lock_pid.as_deref(), Some("100"));
    assert_eq!(snap.time_since.as_deref(), Some("10:00:01"));
    assert!(snap.view_focus_severe);
    assert_eq!(snap.cursor_row_id, pinned_id);
    assert_eq!(snap.list_offset, 2);
    assert_eq!(snap.bookmarks.len(), 1);

    let (mut fresh, _f2, _cfg2) = session_file_app(&five_line_body());
    fresh.apply_session(&snap).unwrap();
    assert_eq!(fresh.groups.groups.len(), 2);
    assert!(fresh.groups.groups[0].enabled);
    assert!(!fresh.groups.groups[1].enabled);
    assert_eq!(fresh.groups.excludes.len(), 1);
    assert!(!fresh.groups.excludes[0].enabled);
    assert_eq!(fresh.highlight_groups.groups.len(), 1);
    assert!(!fresh.highlight_groups.groups[0].enabled);
    assert_eq!(fresh.active_highlight, Some(0));
    assert_eq!(fresh.lock_pid.as_deref(), Some("100"));
    assert!(fresh.time_bound.as_ref().unwrap().is_active());
    assert!(fresh.view_focus.severe);
    assert_eq!(fresh.bookmarks.len(), 1);
    assert!(fresh.bookmark_row_ids.contains(&pinned_id));
    assert_eq!(fresh.pending_restore_cursor, Some(pinned_id));
    assert!(!fresh.following, "restore must not leave following on");
}

#[test]
fn capture_returns_none_for_degenerate_and_disabled() {
    let (mut app, f, _cfg) = session_file_app(&five_line_body());
    app.cursor = 0;
    assert!(
        app.capture_session(f.path()).is_none(),
        "no rules / no pins / top cursor is degenerate"
    );

    app.groups.groups.push(
        crate::input::build_group_from_chips(
            vec![chip(crate::input::ChipField::Tag, "Tag1")],
            true,
        )
        .unwrap()
        .unwrap(),
    );
    assert!(app.capture_session(f.path()).is_some());

    app.session_restore_disabled = true;
    assert!(
        app.capture_session(f.path()).is_none(),
        "R5 suppression blocks writes too"
    );
}

#[test]
fn apply_session_drops_lock_tid_when_pid_also_set() {
    let (mut app, _f, _cfg) = session_file_app(&five_line_body());
    let mut snap = crate::session::SessionFile::new("/tmp/a.log".into());
    snap.lock_pid = Some("100".into());
    snap.lock_tid = Some("200".into());
    snap.cursor_row_id = 2;
    app.apply_session(&snap).unwrap();
    assert_eq!(app.lock_pid.as_deref(), Some("100"));
    assert_eq!(app.lock_tid, None, "lock pid/tid are mutually exclusive");
}

#[test]
fn apply_session_clamps_out_of_range_active_highlight() {
    let (mut app, _f, _cfg) = session_file_app(&five_line_body());
    let mut snap = crate::session::SessionFile::new("/tmp/a.log".into());
    snap.highlights = vec![crate::session::SessionHighlight {
        pattern: "boom".into(),
        enabled: true,
    }];
    snap.active_highlight = Some(7);
    app.apply_session(&snap).unwrap();
    assert_eq!(app.active_highlight, None);
}

#[test]
fn apply_session_caps_bookmarks_and_syncs_row_id_cache() {
    let (mut app, _f, _cfg) = session_file_app(&five_line_body());
    let mut snap = crate::session::SessionFile::new("/tmp/a.log".into());
    snap.bookmarks = (1..=(crate::bookmark::BOOKMARK_SOFT_CAP as u64 + 4))
        .map(|i| crate::session::SessionBookmark {
            row_id: i,
            raw: format!("04-02 10:00:00.000  1  1 I Tag   : line{i}"),
        })
        .collect();
    app.apply_session(&snap).unwrap();
    assert_eq!(app.bookmarks.len(), crate::bookmark::BOOKMARK_SOFT_CAP);
    assert_eq!(
        app.bookmark_row_ids.len(),
        app.bookmarks.len(),
        "row_id cache must stay in lockstep"
    );
}

#[test]
fn deferred_cursor_lands_on_restored_row() {
    let (mut app, _f, _cfg) = session_file_app(&five_line_body());
    let mut snap = crate::session::SessionFile::new("/tmp/a.log".into());
    snap.cursor_row_id = 4; // file mode: row_id == line_index + 1
    snap.list_offset = 2;
    app.apply_session(&snap).unwrap();
    assert_eq!(app.pending_restore_cursor, Some(4));
    app.poll_file_store();
    assert_eq!(app.pending_restore_cursor, None);
    assert_eq!(app.cursor, 3);
    assert_eq!(app.list_offset, 2);
}

#[test]
fn deferred_cursor_degrades_to_top_when_row_is_gone() {
    let (mut app, _f, _cfg) = session_file_app(&five_line_body());
    let mut snap = crate::session::SessionFile::new("/tmp/a.log".into());
    snap.cursor_row_id = 9_999; // beyond the 5-line file (R8)
    snap.list_offset = 3;
    app.apply_session(&snap).unwrap();
    assert!(app.file_index_done(), "open_sync completes the index");
    app.poll_file_store();
    assert_eq!(app.pending_restore_cursor, None);
    assert_eq!(app.cursor, 0);
    assert_eq!(app.list_offset, 0);
}

#[test]
fn deferred_cursor_waits_for_an_in_flight_filter_scan() {
    let (mut app, _f, _cfg) = session_file_app(&five_line_body());
    app.groups.groups.push(
        crate::input::build_group_from_chips(
            vec![chip(crate::input::ChipField::Tag, "Tag3")],
            true,
        )
        .unwrap()
        .unwrap(),
    );
    assert!(app.filter_active());
    // Index done (open_sync) but the filter scan has not reported hits yet:
    // `visible` is legitimately an empty Subset here.
    app.pending_restore_cursor = Some(4);
    app.index.visible = Visible::Subset(Vec::new());
    app.ingest_done = false;
    app.poll_file_store();
    assert_eq!(
        app.pending_restore_cursor,
        Some(4),
        "must not give up while the filter scan is still settling"
    );

    // Scan settled without the row → only now degrade to the top (R8).
    app.ingest_done = true;
    app.poll_file_store();
    assert_eq!(app.pending_restore_cursor, None);
    assert_eq!(app.cursor, 0);
}

#[test]
fn clear_session_flashes_and_removes_archive() {
    let (mut app, f, _cfg) = session_file_app(&five_line_body());
    assert!(!app.has_session_worth_clearing());
    app.clear_session_for_current_file();
    assert_eq!(app.status_msg.as_deref(), Some("NO SESSION"));

    let mut snap =
        crate::session::SessionFile::new(crate::session::normalize_source(f.path()).unwrap());
    snap.cursor_row_id = 3;
    crate::session::save(&app.config_dir, f.path(), &snap).unwrap();
    assert!(app.has_session_worth_clearing());

    app.clear_session_for_current_file();
    assert_eq!(app.status_msg.as_deref(), Some("SESSION CLEARED"));
    assert!(!app.has_session_worth_clearing());
}

#[test]
fn has_session_worth_clearing_is_false_when_suppressed() {
    let (mut app, f, _cfg) = session_file_app(&five_line_body());
    let mut snap =
        crate::session::SessionFile::new(crate::session::normalize_source(f.path()).unwrap());
    snap.cursor_row_id = 3;
    crate::session::save(&app.config_dir, f.path(), &snap).unwrap();
    assert!(app.has_session_worth_clearing());
    app.session_restore_disabled = true;
    assert!(!app.has_session_worth_clearing());
}

#[test]
fn reset_for_source_switch_keeps_suppression_clears_transients() {
    let (mut app, _f, _cfg) = session_file_app(&five_line_body());
    app.session_restore_disabled = true;
    app.session_restored = true;
    app.pending_restore_cursor = Some(3);
    app.reset_for_source_switch();
    assert!(
        app.session_restore_disabled,
        "suppression is process-scoped, not per-source"
    );
    assert!(!app.session_restored);
    assert_eq!(app.pending_restore_cursor, None);
}

#[test]
fn stream_store_never_reports_file_index_done() {
    let app = App::new(100);
    assert!(!app.file_index_done());
}
