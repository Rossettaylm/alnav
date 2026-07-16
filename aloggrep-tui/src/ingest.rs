use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::model::EntryRow;

/// Spawn a background thread that reads `path` line by line, parses each
/// line into an `EntryRow`, and sends it on the returned channel. The
/// channel closes (sender dropped) once the file is fully read — this is
/// the same channel shape `--hdc` streaming will use in Task 15, just with
/// a finite instead of unbounded producer.
pub fn spawn_file_ingest(path: String) -> Receiver<EntryRow> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok(file) = File::open(&path) else { return };
        for line in BufReader::new(file).lines().flatten() {
            if let Some(row) = EntryRow::from_line(&line) {
                if tx.send(row).is_err() {
                    return; // receiver dropped, stop reading
                }
            }
        }
    });
    rx
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

        let rx = spawn_file_ingest(file.path().to_string_lossy().into_owned());

        let first = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(first.tag, "TagA");
        let second = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(second.tag, "TagB");

        // channel closes after EOF: recv should now return Err
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_err());
    }
}
