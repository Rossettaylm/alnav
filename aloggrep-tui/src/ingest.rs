use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::model::EntryRow;

/// Open `path` synchronously and, on success, spawn a background thread
/// that reads it line by line, parses each line into an `EntryRow`, and
/// sends it on the returned channel. The channel closes (sender dropped)
/// once the file is fully read — this is the same channel shape `--hdc`
/// streaming will use in Task 15, just with a finite instead of unbounded
/// producer.
///
/// The file is opened before spawning the thread so a missing/unreadable
/// path surfaces as an immediate `Err` to the caller, rather than as a
/// `Receiver` that silently closes with zero rows sent (indistinguishable
/// from "opened fine but had no matching lines").
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
}
