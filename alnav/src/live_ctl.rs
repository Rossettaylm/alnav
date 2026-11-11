//! Live `--hdc`/`--adb` capture control: child guard, reconnect with backoff, health check.

use crate::app::App;
use crate::ingest;
use std::time::{Duration, Instant};

/// Fixed backoff between live reconnect spawn attempts.
const RECONNECT_BACKOFF: Duration = Duration::from_secs(2);

/// After spawn, wait briefly and reject sessions whose child already exited
/// (common when `hdc`/`adb` starts then immediately fails without a device).
const RECONNECT_HEALTH_WAIT: Duration = Duration::from_millis(150);

/// Ensures the live child process is killed no matter which exit path
/// `main()` takes, including an early `?` bail-out between binding this
/// guard and the end of `main()`. Manual kill/wait calls threaded into every
/// fallible line are easy to miss when new fallible calls are added later;
/// `Drop` closes that gap structurally instead.
struct LiveChildGuard(Option<std::process::Child>);

impl LiveChildGuard {
    fn new(child: Option<std::process::Child>) -> Self {
        Self(child)
    }

    /// Kill/wait the previous child (if any), then take ownership of `child`.
    fn replace(&mut self, child: Option<std::process::Child>) {
        if let Some(mut old) = self.0.take() {
            let _ = old.kill();
            let _ = old.wait();
        }
        self.0 = child;
    }
}

impl Drop for LiveChildGuard {
    fn drop(&mut self) {
        self.replace(None);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LiveBackend {
    Hdc,
    Adb,
}

/// Replaceable live ingest session for `--hdc` / `--adb` TUI mode.
pub(crate) struct LiveIngestCtl {
    backend: LiveBackend,
    device: Option<String>,
    pub(crate) ingest: Option<ingest::IngestHandle>,
    child: LiveChildGuard,
    last_reconnect_at: Option<Instant>,
}

impl LiveIngestCtl {
    fn new(
        backend: LiveBackend,
        device: Option<String>,
        ingest: ingest::IngestHandle,
        child: std::process::Child,
    ) -> Self {
        Self {
            backend,
            device,
            ingest: Some(ingest),
            child: LiveChildGuard::new(Some(child)),
            last_reconnect_at: None,
        }
    }

    /// When `app.ingest_done`, attempt respawn subject to [`RECONNECT_BACKOFF`].
    /// On success: swap ring/child, [`App::mark_live_reconnected`], keep buffers.
    /// `spawn` is injectable for tests; production uses [`Self::spawn_for_reconnect`].
    ///
    /// `last_reconnect_at` is always stamped on an attempt (success or fail) so a
    /// short-lived false session cannot immediately re-flash `RECONNECTED`.
    fn try_reconnect<F>(&mut self, app: &mut App, now: Instant, spawn: F) -> bool
    where
        F: FnOnce() -> Result<alnav::live::LiveSession, String>,
    {
        if !app.ingest_done {
            return false;
        }
        if let Some(at) = self.last_reconnect_at {
            if now.duration_since(at) < RECONNECT_BACKOFF {
                return false;
            }
        }
        self.last_reconnect_at = Some(now);
        match spawn() {
            Ok(session) => match ensure_capture_alive(session, RECONNECT_HEALTH_WAIT) {
                Ok(session) => {
                    let (ring, child) = ingest::spawn_live_ingest(session);
                    self.ingest = Some(ingest::IngestHandle::Ring(ring));
                    self.child.replace(Some(child));
                    app.mark_live_reconnected();
                    true
                }
                Err(_) => false,
            },
            Err(_) => false,
        }
    }

    /// Probe device reachability, then spawn. Spawn alone is not enough: `hdc`
    /// / `adb` often start successfully with no device and exit immediately.
    ///
    /// Reachability uses [`alnav::adb::device_reachable`] /
    /// [`alnav::hdc::device_reachable`], **not** [`now_marker`]: a failed clock
    /// query only disables ADB history filtering and must not be reported as
    /// "device unreachable". HDC clears the in-memory ring inside `spawn_hilog`.
    fn spawn_for_reconnect(
        backend: LiveBackend,
        device: Option<&str>,
    ) -> Result<alnav::live::LiveSession, String> {
        match backend {
            LiveBackend::Hdc => {
                if !alnav::hdc::device_reachable(device) {
                    return Err("hdc device unreachable".into());
                }
                alnav::hdc::spawn_hilog(device)
            }
            LiveBackend::Adb => {
                if !alnav::adb::device_reachable(device) {
                    return Err("adb device unreachable".into());
                }
                alnav::adb::spawn_logcat(device)
            }
        }
    }

    pub(crate) fn try_reconnect_now(&mut self, app: &mut App, now: Instant) -> bool {
        let backend = self.backend;
        let device = self.device.clone();
        self.try_reconnect(app, now, || {
            Self::spawn_for_reconnect(backend, device.as_deref())
        })
    }
}

pub(crate) fn spawn_live_ctl(
    backend: LiveBackend,
    device: Option<String>,
) -> Result<LiveIngestCtl, String> {
    let session = LiveIngestCtl::spawn_for_reconnect(backend, device.as_deref())?;
    let (ring, child) = ingest::spawn_live_ingest(session);
    Ok(LiveIngestCtl::new(
        backend,
        device,
        ingest::IngestHandle::Ring(ring),
        child,
    ))
}

/// Reject capture children that die during the post-spawn grace window.
fn ensure_capture_alive(
    mut session: alnav::live::LiveSession,
    wait: Duration,
) -> Result<alnav::live::LiveSession, String> {
    std::thread::sleep(wait);
    match session.child.try_wait() {
        Ok(None) => Ok(session),
        Ok(Some(status)) => {
            let _ = session.child.kill();
            let _ = session.child.wait();
            Err(format!("capture exited immediately ({status})"))
        }
        Err(e) => {
            let _ = session.child.kill();
            let _ = session.child.wait();
            Err(format!("capture health check failed: {e}"))
        }
    }
}

#[cfg(test)]
mod tests;
