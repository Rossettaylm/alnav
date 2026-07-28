use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;

use alnav::live::LiveSession;

use crate::model::EntryRow;

/// Capacity of the live ingest drop-oldest ring (P-after backpressure).
/// When full, the producer pops the oldest undrained row before pushing —
/// never blocks reading `hilog`.
pub const INGEST_RING_CAP: usize = 8192;

/// Non-blocking ingest poll result used by [`App::drain`](crate::app::App::drain).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvKind {
    Empty,
    Disconnected,
}

/// Unified non-blocking row source for channel (tests) and live drop-oldest ring.
pub trait TryRecvRow {
    fn try_recv_row(&self) -> Result<EntryRow, TryRecvKind>;
}

impl TryRecvRow for Receiver<EntryRow> {
    fn try_recv_row(&self) -> Result<EntryRow, TryRecvKind> {
        match self.try_recv() {
            Ok(row) => Ok(row),
            Err(TryRecvError::Empty) => Err(TryRecvKind::Empty),
            Err(TryRecvError::Disconnected) => Err(TryRecvKind::Disconnected),
        }
    }
}

impl TryRecvRow for DropOldestRing {
    fn try_recv_row(&self) -> Result<EntryRow, TryRecvKind> {
        self.try_pop()
    }
}

impl TryRecvRow for Arc<DropOldestRing> {
    fn try_recv_row(&self) -> Result<EntryRow, TryRecvKind> {
        self.as_ref().try_pop()
    }
}

/// Owned ingest handle for the main event loop.
///
/// `-f` now uses [`crate::store::FileStore`] (no row channel). `Channel` remains
/// for unit tests and any transitional callers; live sources use [`Self::Ring`].
pub enum IngestHandle {
    Channel(Receiver<EntryRow>),
    Ring(Arc<DropOldestRing>),
}

impl TryRecvRow for IngestHandle {
    fn try_recv_row(&self) -> Result<EntryRow, TryRecvKind> {
        match self {
            IngestHandle::Channel(rx) => rx.try_recv_row(),
            IngestHandle::Ring(ring) => ring.try_recv_row(),
        }
    }
}

struct RingInner {
    buf: VecDeque<EntryRow>,
    /// Producer finished. Drain reports Disconnected only when
    /// this is set **and** the buffer is empty.
    disconnected: bool,
}

/// Bounded drop-oldest queue shared by a live producer thread and the UI.
///
/// Not a `sync_channel`: on full we discard the oldest undrained row so the
/// producer never blocks.
pub struct DropOldestRing {
    cap: usize,
    inner: Mutex<RingInner>,
}

impl DropOldestRing {
    pub fn new(cap: usize) -> Arc<Self> {
        Arc::new(Self {
            cap: cap.max(1),
            inner: Mutex::new(RingInner {
                buf: VecDeque::with_capacity(cap.min(1024)),
                disconnected: false,
            }),
        })
    }

    /// Producer path: parse already done; never blocks. Drops oldest when full.
    pub fn push(&self, row: EntryRow) {
        let mut g = self.inner.lock().expect("ingest ring mutex");
        if g.disconnected {
            return;
        }
        if g.buf.len() >= self.cap {
            g.buf.pop_front();
        }
        g.buf.push_back(row);
    }

    /// Mark the producer finished. Further `push` calls are no-ops.
    pub fn mark_disconnected(&self) {
        let mut g = self.inner.lock().expect("ingest ring mutex");
        g.disconnected = true;
    }

    /// Non-blocking consumer pop. `Disconnected` only when the producer has
    /// finished and no rows remain.
    pub fn try_pop(&self) -> Result<EntryRow, TryRecvKind> {
        let mut g = self.inner.lock().expect("ingest ring mutex");
        if let Some(row) = g.buf.pop_front() {
            return Ok(row);
        }
        if g.disconnected {
            Err(TryRecvKind::Disconnected)
        } else {
            Err(TryRecvKind::Empty)
        }
    }

    /// Current buffered length (tests / diagnostics).
    pub fn len(&self) -> usize {
        self.inner.lock().expect("ingest ring mutex").buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }
}

/// Legacy line→`EntryRow` channel ingest (unit tests). Production `-f` uses
/// [`crate::store::FileStore::open`] instead.
///
/// The file is opened before spawning the thread so a missing/unreadable
/// path surfaces as an immediate `Err` to the caller.
pub fn spawn_file_ingest(path: String) -> io::Result<Receiver<EntryRow>> {
    let file = File::open(&path)?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(file).lines().flatten() {
            if let Some(row) = EntryRow::from_line(&line) {
                if tx.send(row).is_err() {
                    return; // receiver dropped, stop reading
                }
            }
        }
    });
    Ok(rx)
}

/// Continuously read `session.lines`, parse each line (P-after), and push into
/// a bounded drop-oldest ring until the iterator ends or the process exits.
/// Used by both live backends. The ring never blocks the producer when full.
pub fn spawn_live_ingest(mut session: LiveSession) -> (Arc<DropOldestRing>, std::process::Child) {
    let ring = DropOldestRing::new(INGEST_RING_CAP);
    let child = session.child;
    let producer = Arc::clone(&ring);
    thread::spawn(move || {
        for line in session.lines.by_ref() {
            let Ok(line) = line else { continue };
            if let Some(row) = EntryRow::from_line(&line) {
                producer.push(row);
            }
        }
        producer.mark_disconnected();
    });
    (ring, child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;

    #[test]
    fn test_spawn_file_ingest_sends_parsed_rows_then_closes() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "04-02 10:00:00.000  1234  5678 I TagA    : first").unwrap();
        writeln!(file, "not parseable").unwrap();
        writeln!(file, "04-02 10:00:01.000  1234  5678 E TagB    : second").unwrap();
        file.flush().unwrap();

        let rx = spawn_file_ingest(file.path().to_string_lossy().into_owned()).unwrap();

        let first = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(first.tag, "TagA");
        let second = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(second.tag, "TagB");

        // channel closes after EOF: recv should now return Err
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
    }

    #[test]
    fn test_spawn_file_ingest_missing_file_errors_immediately() {
        let result = spawn_file_ingest("/nonexistent/path/that/does/not/exist.log".to_string());
        assert!(result.is_err());
    }

    fn sample_row(tag: &str) -> EntryRow {
        EntryRow::from_line(&format!("04-02 10:00:00.000  1  1 I {tag}   : m")).unwrap()
    }

    #[test]
    fn drop_oldest_ring_drops_front_when_full() {
        let ring = DropOldestRing::new(3);
        ring.push(sample_row("A"));
        ring.push(sample_row("B"));
        ring.push(sample_row("C"));
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.capacity(), 3);
        assert!(!ring.is_empty());
        ring.push(sample_row("D")); // drops A
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.try_pop().unwrap().tag, "B");
        assert_eq!(ring.try_pop().unwrap().tag, "C");
        assert_eq!(ring.try_pop().unwrap().tag, "D");
        assert!(matches!(ring.try_pop(), Err(TryRecvKind::Empty)));
    }

    #[test]
    fn drop_oldest_ring_disconnected_after_drain() {
        let ring = DropOldestRing::new(8);
        ring.push(sample_row("A"));
        ring.mark_disconnected();
        assert_eq!(ring.try_pop().unwrap().tag, "A");
        assert!(matches!(ring.try_pop(), Err(TryRecvKind::Disconnected)));
    }
}

#[cfg(test)]
mod live_ingest_tests {
    use super::*;
    use alnav::live::{LiveFilter, LiveSession};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    #[test]
    fn test_spawn_live_ingest_forwards_parsed_lines() {
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("printf '04-02 10:00:00.000  1  1 I TagA    : hello\\n'; sleep 0.2")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let lines = LiveFilter {
            inner: std::io::BufReader::new(stdout).lines(),
            start_marker: None,
        };
        let session = LiveSession {
            child,
            lines,
            used_history_fallback: true,
        };

        let (ring, mut real_child) = spawn_live_ingest(session);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        let row = loop {
            match ring.try_pop() {
                Ok(row) => break row,
                Err(TryRecvKind::Empty) => {
                    if std::time::Instant::now() > deadline {
                        panic!("timed out waiting for live ingest row");
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(TryRecvKind::Disconnected) => panic!("disconnected before row"),
            }
        };
        assert_eq!(row.tag, "TagA");

        let _ = real_child.kill();
        let _ = real_child.wait();
    }
}
