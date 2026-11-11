//! Crash context detection and crash detail rendering tests.

use super::*;
use crate::app::App;
use crate::store::FileStore;
use alnav::crash::CrashType;
use ratatui::text::Line;
use std::io::Write;

fn write_temp(contents: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(contents.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

fn joined(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(|l| {
            l.spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn push_stream_line(app: &mut App, line: &str) {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(crate::model::EntryRow::from_line(line).unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
}

#[test]
fn crash_context_file_mode_merges_stack_continuation() {
    let body = "04-02 10:00:00.000  1  1 E AndroidRuntime : FATAL EXCEPTION: main\n\
         java.lang.RuntimeException: boom\n\
         \tat com.example.Foo.bar(Foo.java:10)\n\
         \tat com.example.Foo.baz(Foo.java:20)\n\
         04-02 10:00:01.000  1  1 I Tag     : next entry\n";
    let f = write_temp(body);
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    app.cursor = 0;
    let row = app.current_row().unwrap();
    let (info, truncated) = crash_context_for_row(&app, &row).expect("crash detected");
    assert!(!truncated);
    assert!(matches!(info.crash_type, CrashType::FatalException));
    assert_eq!(info.headline, "FATAL EXCEPTION: main");
    assert_eq!(
        info.exception.as_deref(),
        Some("java.lang.RuntimeException")
    );
    assert_eq!(info.stack.len(), 2);
    assert!(info.stack[0].contains("Foo.bar"));
    assert!(info.stack[1].contains("Foo.baz"));

    let rendered = joined(&render_crash_detail_lines(&info, false, truncated, 60));
    assert!(rendered.contains("FATAL EXCEPTION"));
    assert!(rendered.contains("Foo.bar"));
    assert!(!rendered.contains("truncated"));
}

#[test]
fn crash_context_file_mode_truncates_at_scan_limit() {
    let mut body =
        String::from("04-02 10:00:00.000  1  1 E AndroidRuntime : FATAL EXCEPTION: main\n");
    for i in 0..600 {
        body.push_str(&format!("junk continuation line {i}\n"));
    }
    let f = write_temp(&body);
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    app.cursor = 0;
    let row = app.current_row().unwrap();
    let (info, truncated) = crash_context_for_row(&app, &row).expect("crash detected");
    assert!(truncated, "scan past 500 continuation lines must truncate");

    let rendered = joined(&render_crash_detail_lines(&info, false, truncated, 60));
    assert!(rendered.contains("truncated"));
}

#[test]
fn crash_context_stream_mode_single_line_has_no_stack() {
    let mut app = App::new(100);
    push_stream_line(
        &mut app,
        "04-02 10:00:00.000  1  1 E AndroidRuntime : FATAL EXCEPTION: main",
    );
    app.cursor = 0;
    let row = app.current_row().unwrap();
    let (info, truncated) = crash_context_for_row(&app, &row).expect("crash detected");
    assert!(!truncated);
    assert!(info.stack.is_empty());
    assert_eq!(info.headline, "FATAL EXCEPTION: main");

    let rendered = joined(&render_crash_detail_lines(&info, true, truncated, 60));
    assert!(rendered.contains("stream"));
}

#[test]
fn crash_context_none_for_non_crash_signature() {
    let mut app = App::new(100);
    push_stream_line(&mut app, r#"04-02 10:00:00.000  1  1 E Tag     : {"a":1}"#);
    app.cursor = 0;
    let row = app.current_row().unwrap();
    assert!(crash_context_for_row(&app, &row).is_none());
}

#[test]
fn crash_context_none_for_bare_continuation_line() {
    let body = "04-02 10:00:00.000  1  1 E AndroidRuntime : FATAL EXCEPTION: main\n\
         \tat com.example.Foo.bar(Foo.java:10)\n";
    let f = write_temp(body);
    let mut app = App::new(100);
    app.export_source = crate::export::ExportSource::File(f.path().display().to_string());
    app.set_file_store(FileStore::open_sync(f.path()).unwrap());
    app.cursor = 1;
    let row = app.current_row().unwrap();
    assert!(!row.is_parsed());
    assert!(crash_context_for_row(&app, &row).is_none());
}
