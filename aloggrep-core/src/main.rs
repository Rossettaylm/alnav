use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, LineWriter, Write};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use glob::glob;

use aloggrep::crash::{self, CrashDetector};
use aloggrep::dedupe::Deduper;
use aloggrep::filter::FilterChain;
use aloggrep::formatter::{FieldSet, Formatter};
use aloggrep::histogram::{self, Histogram};
use aloggrep::multiline::MultilineMerger;
use aloggrep::parser::LogEntry;
use aloggrep::sampler::Sampler;
use aloggrep::summary::Summary;
use aloggrep::{Cli, OutputFormat};

/// Resolve effective before/after context from -C/-A/-B flags.
fn context_sizes(cli: &Cli) -> (usize, usize) {
    let before = cli.before_context.or(cli.context).unwrap_or(0);
    let after = cli.after_context.or(cli.context).unwrap_or(0);
    (before, after)
}

/// Whether context lines should be printed (text mode, not count/summary/crashes).
fn use_context(cli: &Cli) -> bool {
    let (b, a) = context_sizes(cli);
    (b > 0 || a > 0) && !cli.count && !cli.summary && !cli.crashes
}

/// Read all lines from files matching glob patterns into a Vec.
fn collect_file_lines(patterns: &[String]) -> Vec<String> {
    let mut all_lines = Vec::new();
    for pattern in patterns {
        let paths = match glob(pattern) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("loggrep: invalid glob '{}': {}", pattern, e);
                process::exit(2);
            }
        };
        for path in paths.flatten() {
            let file = match File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("loggrep: cannot open '{}': {}", path.display(), e);
                    continue;
                }
            };
            for line in BufReader::new(file).lines().flatten() {
                all_lines.push(line);
            }
        }
    }
    all_lines
}

/// Query the device's current time as `MM-DD HH:MM:SS`, matching the prefix
/// `LogEntry::time_full()` extracts from hilog lines. Used by `--hdc` to skip
/// hilogd's buffered history and start from "now". Returns `None` if the
/// query fails, in which case `--hdc` falls back to showing whatever hilog
/// dumps on start.
fn hdc_now_marker(device: Option<&str>) -> Option<String> {
    let mut cmd = Command::new("hdc");
    if let Some(serial) = device {
        cmd.arg("-t").arg(serial);
    }
    cmd.arg("shell").arg("date").arg("+%m-%d %H:%M:%S");
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
}

/// Wraps a raw hilog line iterator and drops lines older than `start_marker`
/// (device-clock "MM-DD HH:MM:SS"). `hdc hilog` dumps hilogd's buffered
/// history before streaming live, which floods `--hdc` with stale entries;
/// this restores "only what happens from now on" semantics without touching
/// the shared device-side ring buffer (so other readers, e.g. a persistent
/// capture daemon, are unaffected).
struct HdcLiveFilter<I> {
    inner: I,
    start_marker: Option<String>,
}

impl<I: Iterator<Item = io::Result<String>>> Iterator for HdcLiveFilter<I> {
    type Item = io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        let Some(marker) = &self.start_marker else {
            return self.inner.next();
        };
        for line in self.inner.by_ref() {
            let l = match &line {
                Ok(l) => l,
                Err(_) => continue,
            };
            let is_live = LogEntry::parse(l)
                .and_then(|e| e.time_full().map(str::to_string))
                .map_or(false, |t| t.as_str() >= marker.as_str());
            if is_live {
                self.start_marker = None;
                return Some(line);
            }
        }
        None
    }
}

/// Time-based context: two-pass approach.
/// Pass 1: collect all lines, find matching timestamps.
/// Pass 2: output all lines whose timestamp falls within [match_ts - window, match_ts + window].
fn run_time_context<W: io::Write>(
    all_lines: &[String],
    filter_chain: &FilterChain,
    formatter: &Formatter,
    cli: &Cli,
    window_secs: u64,
    out: &mut W,
) -> usize {
    // Pass 1: find match timestamps as seconds offsets
    struct TimedLine {
        secs: Option<f64>,
    }

    // Parse all lines and compute timestamps as comparable seconds
    let mut timed: Vec<TimedLine> = Vec::with_capacity(all_lines.len());
    let mut match_secs: Vec<f64> = Vec::new();

    for line in all_lines {
        let entry = LogEntry::parse(line);
        let secs = entry.as_ref().and_then(|e| timestamp_to_secs(e.timestamp));
        timed.push(TimedLine { secs });

        if let Some(ref e) = entry {
            let is_match = filter_chain.matches(e);
            let is_match = if cli.invert { !is_match } else { is_match };
            if is_match {
                if let Some(s) = secs {
                    match_secs.push(s);
                }
            }
        }
    }

    if match_secs.is_empty() {
        return 0;
    }

    // Sort match_secs for binary search (already mostly sorted, but be safe)
    match_secs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Pass 2: output lines within any window (binary search for O(N log M))
    let window = window_secs as f64;
    let mut matched = 0usize;
    let mut last_printed = false;

    for (i, line) in all_lines.iter().enumerate() {
        let in_window = timed[i].secs.map_or(false, |s| {
            // Binary search: find the closest match_secs entry
            let idx = match match_secs.binary_search_by(|ms| ms.partial_cmp(&s).unwrap_or(std::cmp::Ordering::Equal)) {
                Ok(i) => i,
                Err(i) => i,
            };
            // Check neighbors at idx-1 and idx
            (idx < match_secs.len() && (match_secs[idx] - s).abs() <= window)
                || (idx > 0 && (match_secs[idx - 1] - s).abs() <= window)
        });

        if in_window {
            if !last_printed && matched > 0 {
                if cli.format == OutputFormat::Json {
                    let _ = writeln!(out, "{{\"separator\":true}}");
                } else {
                    let _ = writeln!(out, "--");
                }
            }
            if let Some(entry) = LogEntry::parse(line) {
                let _ = formatter.write_entry(&entry, line, out);
            } else {
                let _ = writeln!(out, "{line}");
            }
            matched += 1;
            last_printed = true;
            if cli.limit > 0 && matched >= cli.limit {
                break;
            }
        } else {
            last_printed = false;
        }
    }

    matched
}

/// Follow-PID/TID mode: two-pass approach.
/// Pass 1: apply filters, collect PIDs/TIDs from matches.
/// Pass 2: output all entries whose PID/TID is in the collected set.
fn run_follow<W: io::Write>(
    all_lines: &[String],
    filter_chain: &FilterChain,
    formatter: &Formatter,
    cli: &Cli,
    out: &mut W,
) -> usize {
    use std::collections::HashSet;

    let mut pids: HashSet<String> = HashSet::new();
    let mut tids: HashSet<String> = HashSet::new();

    // Pass 1: collect PIDs/TIDs from matching entries
    for line in all_lines {
        if let Some(entry) = LogEntry::parse(line) {
            let is_match = filter_chain.matches(&entry);
            let is_match = if cli.invert { !is_match } else { is_match };
            if is_match {
                if cli.follow_pid && !entry.pid.is_empty() {
                    pids.insert(entry.pid.to_string());
                }
                if cli.follow_tid && !entry.tid.is_empty() {
                    tids.insert(entry.tid.to_string());
                }
            }
        }
    }

    if pids.is_empty() && tids.is_empty() {
        return 0;
    }

    // Pass 2: output all entries whose PID/TID is in the collected sets
    let mut matched = 0usize;
    for line in all_lines {
        if let Some(entry) = LogEntry::parse(line) {
            let pid_ok = !cli.follow_pid || pids.contains(entry.pid);
            let tid_ok = !cli.follow_tid || tids.contains(entry.tid);
            if pid_ok && tid_ok {
                let _ = formatter.write_entry(&entry, line, out);
                matched += 1;
                if cli.limit > 0 && matched >= cli.limit {
                    break;
                }
            }
        }
    }

    matched
}

/// Convert a timestamp string to seconds (for time-context comparison).
/// xlog: "YYYY-MM-DD HH:MM:SS.mmm" → day_offset * 86400 + secs
/// threadtime: "MM-DD HH:MM:SS.mmm" → day_offset * 86400 + secs
fn timestamp_to_secs(ts: &str) -> Option<f64> {
    let ts = ts.trim();
    if ts.len() >= 23 && ts.as_bytes()[4] == b'-' {
        // xlog: YYYY-MM-DD HH:MM:SS.mmm
        let day: f64 = ts[8..10].parse().ok()?;
        let h: f64 = ts[11..13].parse().ok()?;
        let m: f64 = ts[14..16].parse().ok()?;
        let s: f64 = ts[17..19].parse().ok()?;
        let ms: f64 = ts[20..23].parse().ok()?;
        // Use month*31+day as rough day offset for cross-day comparison
        let month: f64 = ts[5..7].parse().ok()?;
        Some((month * 31.0 + day) * 86400.0 + h * 3600.0 + m * 60.0 + s + ms / 1000.0)
    } else if ts.len() >= 18 {
        // threadtime: MM-DD HH:MM:SS.mmm
        let day: f64 = ts[3..5].parse().ok()?;
        let h: f64 = ts[6..8].parse().ok()?;
        let m: f64 = ts[9..11].parse().ok()?;
        let s: f64 = ts[12..14].parse().ok()?;
        let ms: f64 = ts[15..18].parse().ok()?;
        let month: f64 = ts[0..2].parse().ok()?;
        Some((month * 31.0 + day) * 86400.0 + h * 3600.0 + m * 60.0 + s + ms / 1000.0)
    } else {
        None
    }
}

fn run_lines<W: io::Write>(
    lines: impl Iterator<Item = io::Result<String>>,
    filter_chain: &FilterChain,
    formatter: &Formatter,
    cli: &Cli,
    matched: &mut usize,
    summary: &mut Option<Summary>,
    deduper: &mut Option<Deduper>,
    histogram: &mut Option<Histogram>,
    crash_detector: Option<&CrashDetector>,
    sampler: &mut Sampler,
    out: &mut W,
) {
    if use_context(cli) && !sampler.needs_full_scan() {
        run_with_context(lines, filter_chain, formatter, cli, matched, summary, deduper, histogram, out);
    } else {
        run_simple(lines, filter_chain, formatter, cli, matched, summary, deduper, histogram, crash_detector, sampler, out);
    }
}

/// Original fast path — no context buffering.
fn run_simple<W: io::Write>(
    lines: impl Iterator<Item = io::Result<String>>,
    filter_chain: &FilterChain,
    formatter: &Formatter,
    cli: &Cli,
    matched: &mut usize,
    summary: &mut Option<Summary>,
    deduper: &mut Option<Deduper>,
    histogram: &mut Option<Histogram>,
    crash_detector: Option<&CrashDetector>,
    sampler: &mut Sampler,
    out: &mut W,
) {
    for line in lines {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let entry = match LogEntry::parse(&line) {
            Some(e) => e,
            None => {
                if filter_chain.is_empty() && !cli.count && !cli.summary && !cli.dedupe && crash_detector.is_none() && histogram.is_none() {
                    let _ = writeln!(out, "{line}");
                }
                continue;
            }
        };

        let is_match = filter_chain.matches(&entry);
        let is_match = if cli.invert { !is_match } else { is_match };

        // Crash filter: when --crashes, entry must also be a crash
        let crash_type = if is_match {
            crash_detector.and_then(|d| d.detect(&entry))
        } else {
            None
        };
        let is_match = is_match && crash_detector.map_or(true, |_| crash_type.is_some());

        if is_match {
            *matched += 1;
            if let Some(ref mut h) = histogram {
                h.record(&entry);
            } else if let Some(ref mut s) = summary {
                s.record(&entry);
            } else if let Some(ref mut d) = deduper {
                d.record(&entry);
            } else if !cli.count {
                if sampler.should_emit(&line) {
                    emit_entry(&entry, &line, formatter, crash_type, crash_detector, out);
                }
            }
            if cli.limit > 0 && *matched >= cli.limit && !sampler.needs_full_scan() {
                break;
            }
        }
    }
}

/// Write a single matched entry (text/json/csv or crash JSON).
fn emit_entry<W: io::Write>(
    entry: &LogEntry,
    line: &str,
    formatter: &Formatter,
    crash_type: Option<crash::CrashType>,
    crash_detector: Option<&CrashDetector>,
    out: &mut W,
) {
    if let (Some(ct), Some(detector)) = (crash_type, crash_detector) {
        let info = detector.parse_crash(entry, ct);
        let json = serde_json::to_string(&info).unwrap_or_default();
        let _ = writeln!(out, "{json}");
    } else {
        let _ = formatter.write_entry(entry, line, out);
    }
}

/// Re-parse and output a buffered line (used by sampler flush).
fn emit_buffered_line<W: io::Write>(
    line: &str,
    formatter: &Formatter,
    crash_detector: Option<&CrashDetector>,
    out: &mut W,
) {
    if let Some(entry) = LogEntry::parse(line) {
        let crash_type = crash_detector.and_then(|d| d.detect(&entry));
        emit_entry(&entry, line, formatter, crash_type, crash_detector, out);
    } else {
        let _ = writeln!(out, "{line}");
    }
}

/// Context-aware path: buffer before-lines, print after-lines.
fn run_with_context<W: io::Write>(
    lines: impl Iterator<Item = io::Result<String>>,
    filter_chain: &FilterChain,
    formatter: &Formatter,
    cli: &Cli,
    matched: &mut usize,
    summary: &mut Option<Summary>,
    deduper: &mut Option<Deduper>,
    histogram: &mut Option<Histogram>,
    out: &mut W,
) {
    let (ctx_before, ctx_after) = context_sizes(cli);
    let mut before_buf: VecDeque<String> = VecDeque::with_capacity(ctx_before + 1);
    let mut after_remaining: usize = 0;
    // Track the line number of the last printed line to detect gaps for "--" separator
    let mut last_printed: Option<usize> = None;
    let mut line_no: usize = 0;

    for line in lines {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        line_no += 1;

        let entry = LogEntry::parse(&line);
        let is_match = entry
            .as_ref()
            .map(|e| {
                let m = filter_chain.matches(e);
                if cli.invert { !m } else { m }
            })
            .unwrap_or(false);

        if is_match {
            *matched += 1;
            if let Some(ref mut h) = histogram {
                if let Some(ref e) = entry {
                    h.record(e);
                }
            }
            if let Some(ref mut s) = summary {
                if let Some(ref e) = entry {
                    s.record(e);
                }
            }
            if let Some(ref mut d) = deduper {
                if let Some(ref e) = entry {
                    d.record(e);
                }
            }

            // Print separator if there's a gap from previous context group
            let before_start = line_no.saturating_sub(before_buf.len());
            if let Some(lp) = last_printed {
                if before_start > lp + 1 {
                    if cli.format == OutputFormat::Json {
                        let _ = writeln!(out, "{{\"separator\":true}}");
                    } else {
                        let _ = writeln!(out, "--");
                    }
                }
            }

            // Flush before-context buffer
            for (i, ctx_line) in before_buf.drain(..).enumerate() {
                let ctx_line_no = before_start + i;
                if last_printed.map_or(true, |lp| ctx_line_no > lp) {
                    let _ = formatter.write_context_line(&ctx_line, out);
                }
            }

            // Print the matching line
            if let Some(ref e) = entry {
                let _ = formatter.write_entry(e, &line, out);
            } else {
                let _ = writeln!(out, "{line}");
            }
            last_printed = Some(line_no);
            after_remaining = ctx_after;

            if cli.limit > 0 && *matched >= cli.limit && after_remaining == 0 {
                break;
            }
        } else if after_remaining > 0 {
            // Print as after-context
            let _ = formatter.write_context_line(&line, out);
            last_printed = Some(line_no);
            after_remaining -= 1;

            if cli.limit > 0 && *matched >= cli.limit && after_remaining == 0 {
                break;
            }
        } else {
            // Buffer for potential before-context
            if ctx_before > 0 {
                if before_buf.len() == ctx_before {
                    before_buf.pop_front();
                }
                before_buf.push_back(line);
            }
        }
    }
}

fn finish_output<W: io::Write>(
    out: &mut W,
    formatter: &Formatter,
    cli: &Cli,
    matched: usize,
    summary: Option<Summary>,
    deduper: Option<Deduper>,
    histogram: Option<Histogram>,
    sampler: Sampler,
    crash_detector: Option<&CrashDetector>,
) {
    if cli.count {
        let _ = writeln!(out, "{matched}");
    }
    if let Some(h) = histogram {
        let _ = h.write_json(out);
    }
    if let Some(s) = summary {
        let _ = writeln!(out, "{}", s.to_json(matched));
    }
    if let Some(d) = deduper {
        let mut groups = d.into_groups();
        if cli.limit > 0 {
            groups.truncate(cli.limit);
        }
        for g in &groups {
            let _ = formatter.write_dedupe_group(g, out);
        }
    }

    // Flush sampler buffer (tail / sample)
    let result = sampler.finish();
    if let Some(ref header) = result.header {
        let _ = writeln!(out, "\n--- {header} ---\n");
    }
    for line in &result.lines {
        emit_buffered_line(line, formatter, crash_detector, out);
    }

    let _ = out.flush();
}

fn print_examples() {
    let examples = r"Examples:

  Basic filtering
  adb logcat | aloggrep --tag OkHttp --level W
  aloggrep -f app.log --tag 'OkHttp|Retrofit' --level E
  aloggrep -f app.log --msg error -i              # case-insensitive
  aloggrep -f app.log --tag Debug -v              # invert match
  aloggrep -f app.log --tag A --tag B             # tag=A OR tag=B
  aloggrep -f app.log --tag A --tag B --and       # tag=A AND tag=B

  Boolean expressions (-e)
  aloggrep -f app.log -e 'msg ~ timeout and level >= W'
  aloggrep -f app.log -e '(tag ~ OkHttp or tag ~ Retrofit) and level >= W'
  aloggrep -f app.log -e 'not tag ~ Debug'
  aloggrep -f app.log -e 'tag ~ OkHttp' -e 'tag ~ Retrofit'  # multiple -e = OR
  # Syntax: tag|msg|pkg ~ <regex>, level >= V|D|I|W|E|F
  # Combine with: and, or, not, ( )

  Context lines
  aloggrep -f app.log --tag crash -C 3            # 3 lines before + after
  aloggrep -f app.log -e 'level >= E' -B 5 -A 2  # 5 before, 2 after

  Multi-line merge
  aloggrep -f app.log --tag AndroidRuntime -M     # merge stack traces
  adb logcat | aloggrep -M --level E              # merged error entries

  Crash extraction
  aloggrep -f app.log --crashes                   # all crashes => JSON
  aloggrep -f app.log --crashes --tag MyApp       # filter + extract
  aloggrep -f app.log --crashes --limit 5         # first 5 crashes

  Sampling (manage large output)
  aloggrep -f app.log --level E --tail 50           # last 50 errors
  aloggrep -f app.log --level E --limit 20 --tail 20 # first 20 + last 20
  aloggrep -f app.log --sample 100                  # uniform sample of 100

  Deduplicate (group similar lines)
  aloggrep -f app.log --level E --dedupe          # group errors by pattern
  aloggrep -f app.log --dedupe --limit 20         # top 20 patterns
  aloggrep -f app.log --dedupe --format json      # JSON output for AI
  # Numbers/hex/UUIDs are normalized

  Output formats
  aloggrep -f app.log --tag crash --format json --limit 50
  aloggrep -f app.log --format csv > out.csv
  aloggrep -f app.log --tag crash --count         # print match count only
  aloggrep -f app.log --summary                   # stats + top errors + crash count

  Time range
  aloggrep -f app.log --since 10:30:00 --until 10:35:00
  aloggrep -f app.log --since '2026-03-04 10:30:00' --until '2026-03-04 10:35:00'
  aloggrep -f app.log --since '04-02 12:00:00'      # threadtime date+time

  PID/TID filtering
  aloggrep -f app.log --tid 5678 --level W           # track specific thread
  aloggrep -f app.log --pid 1234 --tid 5678          # PID + TID combined
  aloggrep -f app.log -e 'pid ~ 3542 and level >= E' # expression with pid/tid

  Histogram (time distribution)
  aloggrep -f app.log --histogram 1m                 # level distribution per minute
  aloggrep -f app.log --histogram 10s --level E      # error count per 10 seconds

  Field selection
  aloggrep -f app.log --level E --fields level,tag,msg --format json  # minimal output
  aloggrep -f app.log --fields timestamp,msg         # time + message only

  Time-based context
  aloggrep -f app.log --level F --time-context 5s    # all logs within 5s of fatal errors
  aloggrep -f app.log --tag crash --time-context 10s # 10s window around crash lines

  Multi-file time sort
  aloggrep -f 'logs/*.log' --sort-time --level E     # merge-sort by timestamp

  Follow PID/TID
  aloggrep -f app.log --tag OkHttp --level E --follow-pid  # all logs from error PIDs
  aloggrep -f app.log --crashes --follow-tid                # all logs from crashed threads
";
    println!("{examples}");
}

fn main() {
    let cli = Cli::parse();

    if cli.example {
        print_examples();
        return;
    }

    let filter_chain = match FilterChain::from_cli(&cli) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("loggrep: {e}");
            process::exit(2);
        }
    };

    let use_color = !cli.no_color && cli.format == OutputFormat::Text && atty::is(atty::Stream::Stdout);
    let fields = match cli.fields.as_ref() {
        Some(f) => match FieldSet::parse(f) {
            Ok(fs) => fs,
            Err(e) => {
                eprintln!("loggrep: {e}");
                process::exit(2);
            }
        },
        None => FieldSet::all(),
    };
    let formatter = Formatter::new(cli.format, use_color, &filter_chain, fields, &cli.highlight);

    let mut matched: usize = 0;
    let mut summary = if cli.summary { Some(Summary::new()) } else { None };
    let mut deduper = if cli.dedupe { Some(Deduper::new()) } else { None };
    let crash_detector = if cli.crashes { Some(CrashDetector::new()) } else { None };
    let merge = cli.multiline || cli.crashes;

    let mut hist = cli.histogram.as_ref().map(|interval_str| {
        match histogram::parse_interval(interval_str) {
            Ok(secs) => Histogram::new(secs),
            Err(e) => {
                eprintln!("loggrep: bad --histogram interval: {e}");
                process::exit(2);
            }
        }
    });

    if cli.tail > 0 && cli.sample > 0 {
        eprintln!("loggrep: --tail and --sample are mutually exclusive");
        process::exit(2);
    }
    if (cli.follow_pid || cli.follow_tid) && cli.time_context.is_some() {
        eprintln!("loggrep: --follow-pid/--follow-tid cannot be combined with --time-context");
        process::exit(2);
    }
    if cli.hdc && !cli.file.is_empty() {
        eprintln!("loggrep: --hdc cannot be combined with -f (file input)");
        process::exit(2);
    }
    if cli.hdc && cli.time_context.is_some() {
        eprintln!("loggrep: --hdc cannot be combined with --time-context (requires two-pass)");
        process::exit(2);
    }
    if cli.hdc && (cli.follow_pid || cli.follow_tid) {
        eprintln!("loggrep: --hdc cannot be combined with --follow-pid/--follow-tid (requires two-pass)");
        process::exit(2);
    }
    if cli.hdc && cli.sort_time {
        eprintln!("loggrep: --hdc cannot be combined with --sort-time (requires all lines in memory)");
        process::exit(2);
    }
    if cli.device.is_some() && !cli.hdc {
        eprintln!("loggrep: --device requires --hdc");
        process::exit(2);
    }
    let mut sampler = Sampler::new(cli.tail, cli.sample, cli.limit);
    let needs_full_scan = sampler.needs_full_scan();

    // Parse --time-context duration
    let time_context_secs = cli.time_context.as_ref().map(|tc| {
        match histogram::parse_interval(tc) {
            Ok(secs) => secs,
            Err(e) => {
                eprintln!("loggrep: bad --time-context duration: {e}");
                process::exit(2);
            }
        }
    });

    // --time-context mode: two-pass, needs all lines in memory
    if let Some(window_secs) = time_context_secs {
        let all_lines = if cli.file.is_empty() {
            let stdin = io::stdin();
            BufReader::new(stdin.lock()).lines().flatten().collect()
        } else {
            collect_file_lines(&cli.file)
        };

        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        matched = run_time_context(&all_lines, &filter_chain, &formatter, &cli, window_secs, &mut out);
        let _ = out.flush();
        process::exit(if matched > 0 { 0 } else { 1 });
    }

    // --follow-pid / --follow-tid mode: two-pass, needs all lines in memory
    if cli.follow_pid || cli.follow_tid {
        let all_lines = if cli.file.is_empty() {
            let stdin = io::stdin();
            BufReader::new(stdin.lock()).lines().flatten().collect()
        } else {
            collect_file_lines(&cli.file)
        };

        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        matched = run_follow(&all_lines, &filter_chain, &formatter, &cli, &mut out);
        let _ = out.flush();
        process::exit(if matched > 0 { 0 } else { 1 });
    }

    /// Dispatch `run_lines` with optional `MultilineMerger` wrapping.
    macro_rules! dispatch_lines {
        ($lines:expr, $out:expr) => {
            if merge {
                run_lines(
                    MultilineMerger::new($lines),
                    &filter_chain, &formatter, &cli, &mut matched, &mut summary, &mut deduper,
                    &mut hist, crash_detector.as_ref(), &mut sampler, $out,
                );
            } else {
                run_lines(
                    $lines,
                    &filter_chain, &formatter, &cli, &mut matched, &mut summary, &mut deduper,
                    &mut hist, crash_detector.as_ref(), &mut sampler, $out,
                );
            }
        };
    }

    // --hdc mode: spawn hdc hilog and read from child stdout
    if cli.hdc {
        let start_marker = hdc_now_marker(cli.device.as_deref());
        if start_marker.is_none() {
            eprintln!("loggrep: warning: could not query device time, --hdc will include hilogd's buffered history");
        }

        let mut cmd = Command::new("hdc");
        if let Some(ref serial) = cli.device {
            cmd.arg("-t").arg(serial);
        }
        cmd.arg("hilog").arg("--no-block");
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    eprintln!("loggrep: hdc not found, please install HDC tools");
                } else {
                    eprintln!("loggrep: failed to start 'hdc hilog': {e}");
                }
                process::exit(2);
            }
        };

        let child_stdout = child.stdout.take().expect("piped stdout");
        let lines = HdcLiveFilter {
            inner: BufReader::new(child_stdout).lines(),
            start_marker,
        };

        let interactive_tty =
            atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stdout);
        let cbreak_guard = if interactive_tty {
            aloggrep::clearkey::CbreakGuard::enable().ok()
        } else {
            None
        };
        let key_rx = if cbreak_guard.is_some() {
            aloggrep::clearkey::spawn_key_listener()
        } else {
            aloggrep::clearkey::disabled_listener()
        };
        let lines = aloggrep::clearkey::KeypressGate::new(lines, key_rx, |byte| {
            if byte == 0x0C {
                aloggrep::clearkey::write_clear_screen();
            }
        });

        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_clone = Arc::clone(&interrupted);
        ctrlc::set_handler(move || {
            interrupted_clone.store(true, Ordering::SeqCst);
        })
        .expect("failed to set Ctrl+C handler");

        let stdout = io::stdout();
        let mut out = LineWriter::new(stdout.lock());
        dispatch_lines!(lines, &mut out);

        let _ = child.kill();
        let _ = child.wait();

        if interrupted.load(Ordering::SeqCst) {
            let _ = writeln!(io::stderr(), "");
        }

        finish_output(&mut out, &formatter, &cli, matched, summary, deduper, hist, sampler, crash_detector.as_ref());
        drop(cbreak_guard);
        process::exit(if matched > 0 { 0 } else { 1 });
    }

    if cli.file.is_empty() {
        let stdout = io::stdout();
        let mut out = LineWriter::new(stdout.lock());
        let stdin = io::stdin();
        dispatch_lines!(BufReader::new(stdin.lock()).lines(), &mut out);
        finish_output(&mut out, &formatter, &cli, matched, summary, deduper, hist, sampler, crash_detector.as_ref());
    } else if cli.sort_time {
        // --sort-time: read all lines, sort by timestamp, then process
        let mut all_lines = collect_file_lines(&cli.file);
        // Sort by timestamp (lexicographic on time_full, fallback to original order)
        all_lines.sort_by_cached_key(|line| {
            LogEntry::parse(line).and_then(|e| e.time_full().map(|s| s.to_string()))
        });

        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        let sorted_iter = all_lines.into_iter().map(Ok::<String, io::Error>);
        dispatch_lines!(sorted_iter, &mut out);
        finish_output(&mut out, &formatter, &cli, matched, summary, deduper, hist, sampler, crash_detector.as_ref());
    } else {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());

        for pattern in &cli.file {
            let paths = match glob(pattern) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("loggrep: invalid glob '{}': {}", pattern, e);
                    process::exit(2);
                }
            };
            for path in paths.flatten() {
                let file = match File::open(&path) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("loggrep: cannot open '{}': {}", path.display(), e);
                        continue;
                    }
                };
                dispatch_lines!(BufReader::new(file).lines(), &mut out);
                if cli.limit > 0 && matched >= cli.limit && !cli.dedupe && !needs_full_scan {
                    break;
                }
            }
            if cli.limit > 0 && matched >= cli.limit && !cli.dedupe && !needs_full_scan {
                break;
            }
        }

        finish_output(&mut out, &formatter, &cli, matched, summary, deduper, hist, sampler, crash_detector.as_ref());
    }

    process::exit(if matched > 0 { 0 } else { 1 });
}

#[cfg(test)]
mod tests {
    use super::*;
    use aloggrep::formatter::FieldSet;
    use clap::Parser;

    #[test]
    fn test_follow_pid_basic() {
        let lines: Vec<String> = vec![
            "04-02 10:00:00.000  1234  5678 E OkHttp  : error".to_string(),
            "04-02 10:00:01.000  1234  5679 D OkHttp  : debug same pid".to_string(),
            "04-02 10:00:02.000  9999  1111 I Other   : different pid".to_string(),
            "04-02 10:00:03.000  1234  9999 W Tag2    : same pid again".to_string(),
        ];
        let cli = Cli::parse_from(["aloggrep", "--level", "E", "--follow-pid"]);
        let filter_chain = FilterChain::from_cli(&cli).unwrap();
        let formatter = Formatter::new(OutputFormat::Text, false, &filter_chain, FieldSet::all(), &[]);
        let mut buf = Vec::new();
        let matched = run_follow(&lines, &filter_chain, &formatter, &cli, &mut buf);
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(matched, 3);
        assert!(out.contains("error"));
        assert!(out.contains("debug same pid"));
        assert!(!out.contains("different pid"));
        assert!(out.contains("same pid again"));
    }

    #[test]
    fn test_follow_tid_basic() {
        let lines: Vec<String> = vec![
            "04-02 10:00:00.000  1234  5678 E OkHttp  : error".to_string(),
            "04-02 10:00:01.000  9999  5678 D Other   : same tid".to_string(),
            "04-02 10:00:02.000  1234  9999 I Tag2    : different tid".to_string(),
        ];
        let cli = Cli::parse_from(["aloggrep", "--level", "E", "--follow-tid"]);
        let filter_chain = FilterChain::from_cli(&cli).unwrap();
        let formatter = Formatter::new(OutputFormat::Text, false, &filter_chain, FieldSet::all(), &[]);
        let mut buf = Vec::new();
        let matched = run_follow(&lines, &filter_chain, &formatter, &cli, &mut buf);
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(matched, 2);
        assert!(out.contains("error"));
        assert!(out.contains("same tid"));
        assert!(!out.contains("different tid"));
    }

    #[test]
    fn test_follow_pid_and_tid() {
        let lines: Vec<String> = vec![
            "04-02 10:00:00.000  1234  5678 E Tag     : error".to_string(),
            "04-02 10:00:01.000  1234  5678 D Tag     : same both".to_string(),
            "04-02 10:00:02.000  1234  9999 I Tag     : same pid diff tid".to_string(),
            "04-02 10:00:03.000  9999  5678 I Tag     : diff pid same tid".to_string(),
        ];
        let cli = Cli::parse_from(["aloggrep", "--level", "E", "--follow-pid", "--follow-tid"]);
        let filter_chain = FilterChain::from_cli(&cli).unwrap();
        let formatter = Formatter::new(OutputFormat::Text, false, &filter_chain, FieldSet::all(), &[]);
        let mut buf = Vec::new();
        let matched = run_follow(&lines, &filter_chain, &formatter, &cli, &mut buf);
        assert_eq!(matched, 2);
    }

    #[test]
    fn test_follow_pid_no_matches() {
        let lines: Vec<String> = vec![
            "04-02 10:00:00.000  1234  5678 D Tag     : debug only".to_string(),
        ];
        let cli = Cli::parse_from(["aloggrep", "--level", "E", "--follow-pid"]);
        let filter_chain = FilterChain::from_cli(&cli).unwrap();
        let formatter = Formatter::new(OutputFormat::Text, false, &filter_chain, FieldSet::all(), &[]);
        let mut buf = Vec::new();
        let matched = run_follow(&lines, &filter_chain, &formatter, &cli, &mut buf);
        assert_eq!(matched, 0);
    }

    #[test]
    fn test_hdc_live_filter_skips_lines_before_marker() {
        let lines: Vec<io::Result<String>> = vec![
            Ok("04-02 09:59:58.000  1234  5678 I Tag     : old boot log".to_string()),
            Ok("04-02 09:59:59.000  1234  5678 I Tag     : also old".to_string()),
            Ok("04-02 10:00:00.000  1234  5678 I Tag     : right at marker".to_string()),
            Ok("04-02 10:00:01.000  1234  5678 E Tag     : live entry".to_string()),
        ];
        let filter = HdcLiveFilter {
            inner: lines.into_iter(),
            start_marker: Some("04-02 10:00:00".to_string()),
        };
        let out: Vec<String> = filter.map(|l| l.unwrap()).collect();
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("right at marker"));
        assert!(out[1].contains("live entry"));
    }

    #[test]
    fn test_hdc_live_filter_passes_unparsed_lines_once_started() {
        let lines: Vec<io::Result<String>> = vec![
            Ok("========Zeroth log of type: init".to_string()),
            Ok("04-02 10:00:01.000  1234  5678 E Tag     : live entry".to_string()),
            Ok("    at some.stack.trace(File.java:10)".to_string()),
        ];
        let filter = HdcLiveFilter {
            inner: lines.into_iter(),
            start_marker: Some("04-02 10:00:00".to_string()),
        };
        let out: Vec<String> = filter.map(|l| l.unwrap()).collect();
        assert_eq!(out.len(), 2);
        assert!(out[0].contains("live entry"));
        assert!(out[1].contains("stack.trace"));
    }

    #[test]
    fn test_hdc_live_filter_no_marker_passes_everything() {
        let lines: Vec<io::Result<String>> = vec![
            Ok("04-02 09:00:00.000  1234  5678 I Tag     : old boot log".to_string()),
            Ok("anything".to_string()),
        ];
        let filter = HdcLiveFilter {
            inner: lines.into_iter(),
            start_marker: None,
        };
        let out: Vec<String> = filter.map(|l| l.unwrap()).collect();
        assert_eq!(out.len(), 2);
    }
}
