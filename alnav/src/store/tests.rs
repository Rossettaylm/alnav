use super::file::VOCAB_MAX_SAMPLES;
use super::workers::should_flush_filter_batch;
use super::*;
use std::io::Write;
use std::thread;

fn write_temp(contents: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(contents.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

#[test]
fn should_flush_filter_batch_first_threshold() {
    assert!(!should_flush_filter_batch(0, false, false));
    assert!(!should_flush_filter_batch(63, false, false));
    assert!(should_flush_filter_batch(64, false, false));
    assert!(should_flush_filter_batch(100, false, false));
}

#[test]
fn should_flush_filter_batch_subsequent_threshold() {
    assert!(!should_flush_filter_batch(63, true, false));
    assert!(!should_flush_filter_batch(2047, true, false));
    assert!(should_flush_filter_batch(2048, true, false));
}

#[test]
fn should_flush_filter_batch_chunk_end_sparse() {
    assert!(!should_flush_filter_batch(0, false, true));
    assert!(should_flush_filter_batch(1, false, true));
    assert!(should_flush_filter_batch(1, true, true));
}

#[test]
fn scan_empty_file() {
    let f = write_temp("");
    let store = FileStore::open_sync(f.path()).unwrap();
    assert_eq!(store.line_count(), 0);
    assert!(store.index_done());
}

#[test]
fn scan_no_trailing_newline() {
    let f =
        write_temp("04-02 10:00:00.000  1  1 I TagA   : a\n04-02 10:00:01.000  1  1 E TagB   : b");
    let store = FileStore::open_sync(f.path()).unwrap();
    assert_eq!(store.line_count(), 2);
    assert_eq!(store.row_at(0).unwrap().tag, "TagA");
    assert_eq!(store.row_at(1).unwrap().tag, "TagB");
}

#[test]
fn unparseable_line_raw_fallback() {
    let f = write_temp("not a log line\n04-02 10:00:00.000  1  1 I TagA   : ok\n");
    let store = FileStore::open_sync(f.path()).unwrap();
    assert_eq!(store.line_count(), 2);
    let raw = store.row_at(0).unwrap();
    assert_eq!(raw.raw, "not a log line");
    assert!(raw.tag.is_empty());
    assert!(!raw.is_parsed());
    assert_eq!(raw.row_id, 1);
    assert!(store.row_at(1).unwrap().is_parsed());
    assert_eq!(store.row_at(1).unwrap().tag, "TagA");
}

#[test]
fn filter_sync_subset() {
    let f = write_temp(
        "04-02 10:00:00.000  1  1 I TagA   : a\n\
         04-02 10:00:01.000  1  1 E TagB   : b\n\
         04-02 10:00:02.000  1  1 I TagA   : c\n",
    );
    let store = FileStore::open_sync(f.path()).unwrap();
    let hits = store.scan_filter_sync(|r| r.tag == "TagA");
    assert_eq!(hits, vec![0, 2]);
}

#[test]
fn row_ref_deref() {
    let row = EntryRow::from_line("04-02 10:00:00.000  1  1 I TagA   : m").unwrap();
    let r = RowRef::Owned(row);
    assert_eq!(r.tag, "TagA");
}

#[test]
fn non_utf8_lossy() {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(b"04-02 10:00:00.000  1  1 I TagA   : ")
        .unwrap();
    f.write_all(&[0xFF, 0xFE]).unwrap();
    f.write_all(b"\n").unwrap();
    f.flush().unwrap();
    let store = FileStore::open_sync(f.path()).unwrap();
    let row = store.row_at(0).unwrap();
    assert!(!row.raw.is_empty());
}

#[test]
fn bg_index_completes() {
    let f = write_temp(
        "04-02 10:00:00.000  1  1 I TagA   : a\n04-02 10:00:01.000  1  1 E TagB   : b\n",
    );
    let store = FileStore::open(f.path()).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !store.index_done() {
        if std::time::Instant::now() > deadline {
            panic!("index timed out");
        }
        let _ = store.drain_events();
        thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(store.line_count(), 2);
}

#[test]
fn vocab_sample_capped() {
    let mut body = String::new();
    for i in 0..20_000 {
        body.push_str(&format!("04-02 10:00:00.000  1  1 I Tag{i}   : line{i}\n"));
    }
    let f = write_temp(&body);
    let store = FileStore::open_sync(f.path()).unwrap();
    let mut feeds = 0usize;
    store.feed_vocab_sample(|_, _, _| feeds += 1);
    // stride + optional last line; must stay near VOCAB_MAX_SAMPLES.
    assert!(feeds <= VOCAB_MAX_SAMPLES + 1, "feeds={feeds}");
    assert!(feeds >= VOCAB_MAX_SAMPLES / 2, "feeds={feeds}");
}
