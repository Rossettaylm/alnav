//! Tests for live child guard and reconnect backoff / health check.

use super::*;
use crate::app::App;
use crate::{ingest, model};
use alnav::live::{LiveFilter, LiveSession};
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn sleep_child() -> std::process::Child {
    Command::new("sleep")
        .arg("30")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sleep")
}

fn live_session_from_sh(script: &str) -> LiveSession {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn sh live session");
    let stdout = child.stdout.take().expect("piped stdout");
    LiveSession {
        child,
        lines: LiveFilter {
            inner: std::io::BufReader::new(stdout).lines(),
            start_marker: None,
        },
        used_history_fallback: true,
    }
}

fn dummy_ctl() -> LiveIngestCtl {
    let session = live_session_from_sh("sleep 30");
    let (ring, child) = ingest::spawn_live_ingest(session);
    LiveIngestCtl::new(
        LiveBackend::Hdc,
        None,
        ingest::IngestHandle::Ring(ring),
        child,
    )
}

#[test]
fn live_child_guard_replace_kills_previous() {
    let mut guard = LiveChildGuard::new(Some(sleep_child()));
    let old_id = guard.0.as_ref().unwrap().id();
    guard.replace(Some(Command::new("true").spawn().unwrap()));
    // Old sleep should no longer be running (best-effort: waitpid already done).
    let still = Command::new("kill")
        .args(["-0", &old_id.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(!still, "replaced child should be killed");
    assert!(guard.0.is_some());
}

#[test]
fn try_reconnect_skips_when_not_disconnected() {
    let mut app = App::new(100);
    app.ingest_done = false;
    let mut ctl = dummy_ctl();
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_c = Arc::clone(&calls);
    let ok = ctl.try_reconnect(&mut app, Instant::now(), || {
        calls_c.fetch_add(1, Ordering::SeqCst);
        Err("should not spawn".into())
    });
    assert!(!ok);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn try_reconnect_respects_backoff() {
    let mut app = App::new(100);
    app.ingest_done = true;
    let mut ctl = dummy_ctl();
    let t0 = Instant::now();
    ctl.last_reconnect_at = Some(t0);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_c = Arc::clone(&calls);
    let ok = ctl.try_reconnect(&mut app, t0 + Duration::from_millis(500), || {
        calls_c.fetch_add(1, Ordering::SeqCst);
        Err("no".into())
    });
    assert!(!ok);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(app.ingest_done);
}

#[test]
fn try_reconnect_success_keeps_buffer_and_clears_done() {
    let mut app = App::new(100);
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(model::EntryRow::from_line("04-02 10:00:00.000  1  1 I Keep    : a").unwrap())
        .unwrap();
    drop(tx);
    app.drain(&rx);
    assert_eq!(app.rows().len(), 1);
    app.ingest_done = true;

    let mut ctl = dummy_ctl();
    let t0 = Instant::now();
    let ok = ctl.try_reconnect(&mut app, t0, || {
        // Stay alive past RECONNECT_HEALTH_WAIT so health check passes.
        Ok(live_session_from_sh(
            "printf '04-02 10:00:01.000  1  1 I New     : b\\n'; sleep 2",
        ))
    });
    assert!(ok);
    assert!(!app.ingest_done);
    assert_eq!(app.status_msg.as_deref(), Some("RECONNECTED"));
    assert_eq!(
        app.rows().len(),
        1,
        "reconnect must not clear prior buffer before drain"
    );
    // Backoff stamp retained so a dying session cannot immediately re-flash.
    assert_eq!(ctl.last_reconnect_at, Some(t0));

    // Drain new session lines into the same buffer.
    let deadline = Instant::now() + Duration::from_secs(2);
    while app.rows().len() < 2 && Instant::now() < deadline {
        if let Some(ingest) = ctl.ingest.as_ref() {
            app.drain(ingest);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.rows().len(), 2);
    assert_eq!(app.rows()[0].tag, "Keep");
    assert_eq!(app.rows()[1].tag, "New");
}

#[test]
fn try_reconnect_rejects_immediate_exit_without_flash() {
    let mut app = App::new(100);
    app.ingest_done = true;
    let mut ctl = dummy_ctl();
    let t0 = Instant::now();
    let ok = ctl.try_reconnect(&mut app, t0, || Ok(live_session_from_sh("true")));
    assert!(
        !ok,
        "child that exits during health wait must not count as reconnect"
    );
    assert!(app.ingest_done);
    assert_ne!(app.status_msg.as_deref(), Some("RECONNECTED"));
    assert_eq!(ctl.last_reconnect_at, Some(t0));
}

#[test]
fn try_reconnect_failure_stays_disconnected() {
    let mut app = App::new(100);
    app.ingest_done = true;
    let mut ctl = dummy_ctl();
    let t0 = Instant::now();
    let ok = ctl.try_reconnect(&mut app, t0, || Err("device gone".into()));
    assert!(!ok);
    assert!(app.ingest_done);
    assert_eq!(ctl.last_reconnect_at, Some(t0));
}
