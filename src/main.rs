mod crash;
mod dedupe;
mod expr;
mod filter;
mod formatter;
mod histogram;
mod multiline;
mod parser;
mod sampler;
mod summary;

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, LineWriter, Write};
use std::process;

use clap::{Parser, ValueEnum};
use glob::glob;

use crash::CrashDetector;
use dedupe::Deduper;
use filter::FilterChain;
use formatter::{FieldSet, Formatter};
use histogram::Histogram;
use multiline::MultilineMerger;
use parser::LogEntry;
use sampler::Sampler;
use summary::Summary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Csv,
}

#[derive(Parser)]
#[command(
    name = "loggrep",
    about = "Lightweight Android logcat filter & analyzer",
    after_long_help = "\x1b[1mExamples:\x1b[0m

  \x1b[4mBasic filtering\x1b[0m
  adb logcat | loggrep --tag OkHttp --level W
  loggrep -f app.log --tag \"OkHttp|Retrofit\" --level E
  loggrep -f app.log --msg error -i              # case-insensitive
  loggrep -f app.log --tag Debug -v              # invert match
  loggrep -f app.log --tag A --tag B             # tag=A OR tag=B
  loggrep -f app.log --tag A --tag B --and       # tag=A AND tag=B

  \x1b[4mBoolean expressions (-e)\x1b[0m
  loggrep -f app.log -e 'msg ~ timeout and level >= W'
  loggrep -f app.log -e '(tag ~ OkHttp or tag ~ Retrofit) and level >= W'
  loggrep -f app.log -e 'not tag ~ Debug'
  loggrep -f app.log -e 'tag ~ OkHttp' -e 'tag ~ Retrofit'  # multiple -e = OR
  # Syntax: tag|msg|pkg ~ <regex>, level >= V|D|I|W|E|F
  # Combine with: and, or, not, ( )

  \x1b[4mContext lines\x1b[0m
  loggrep -f app.log --tag crash -C 3            # 3 lines before + after
  loggrep -f app.log -e 'level >= E' -B 5 -A 2  # 5 before, 2 after

  \x1b[4mMulti-line merge\x1b[0m
  loggrep -f app.log --tag AndroidRuntime -M     # merge stack traces
  adb logcat | loggrep -M --level E              # merged error entries

  \x1b[4mCrash extraction\x1b[0m
  loggrep -f app.log --crashes                   # all crashes → JSON
  loggrep -f app.log --crashes --tag MyApp       # filter + extract
  loggrep -f app.log --crashes --limit 5         # first 5 crashes

  \x1b[4mSampling (manage large output)\x1b[0m
  loggrep -f app.log --level E --tail 50           # last 50 errors
  loggrep -f app.log --level E --limit 20 --tail 20 # first 20 + last 20
  loggrep -f app.log --sample 100                  # uniform sample of 100

  \x1b[4mDeduplicate (group similar lines)\x1b[0m
  loggrep -f app.log --level E --dedupe          # group errors by pattern
  loggrep -f app.log --dedupe --limit 20         # top 20 patterns
  loggrep -f app.log --dedupe --format json      # JSON output for AI
  # Numbers/hex/UUIDs are normalized: \"timeout 100ms\" ≈ \"timeout 200ms\"

  \x1b[4mOutput formats\x1b[0m
  loggrep -f app.log --tag crash --format json --limit 50
  loggrep -f app.log --format csv > out.csv
  loggrep -f app.log --tag crash --count         # print match count only
  loggrep -f app.log --summary                   # stats + top errors + crash count

  \x1b[4mTime range\x1b[0m
  loggrep -f app.log --since 10:30:00 --until 10:35:00
  loggrep -f app.log --since '2026-03-04 10:30:00' --until '2026-03-04 10:35:00'
  loggrep -f app.log --since '04-02 12:00:00'      # threadtime date+time

  \x1b[4mPID/TID filtering\x1b[0m
  loggrep -f app.log --tid 5678 --level W           # track specific thread
  loggrep -f app.log --pid 1234 --tid 5678          # PID + TID combined
  loggrep -f app.log -e 'pid ~ 3542 and level >= E' # expression with pid/tid

  \x1b[4mHistogram (time distribution)\x1b[0m
  loggrep -f app.log --histogram 1m                 # level distribution per minute
  loggrep -f app.log --histogram 10s --level E      # error count per 10 seconds

  \x1b[4mField selection\x1b[0m
  loggrep -f app.log --level E --fields level,tag,msg --format json  # minimal output
  loggrep -f app.log --fields timestamp,msg         # time + message only

  \x1b[4mTime-based context\x1b[0m
  loggrep -f app.log --level F --time-context 5s    # all logs within 5s of fatal errors
  loggrep -f app.log --tag crash --time-context 10s # 10s window around crash lines

  \x1b[4mMulti-file time sort\x1b[0m
  loggrep -f 'logs/*.log' --sort-time --level E     # merge-sort by timestamp"
)]
pub struct Cli {
    /// Filter by tag (regex, repeatable, OR logic within)
    #[arg(short, long, value_name = "REGEX")]
    tag: Vec<String>,

    /// Filter by message content (regex, repeatable, OR logic within)
    #[arg(short, long, value_name = "REGEX")]
    msg: Vec<String>,

    /// Minimum log level: V, D, I, W, E, F
    #[arg(short, long, value_name = "LEVEL")]
    level: Option<String>,

    /// Filter by package name (repeatable, OR logic within)
    #[arg(short, long, value_name = "NAME")]
    package: Vec<String>,

    /// Read from log file(s) instead of stdin (supports glob)
    #[arg(short, long, value_name = "PATH")]
    file: Vec<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    format: OutputFormat,

    /// Max lines to output (0 = unlimited)
    #[arg(long, default_value_t = 0)]
    limit: usize,

    /// Only print match count
    #[arg(long)]
    count: bool,

    /// Print aggregated summary (JSON)
    #[arg(long)]
    summary: bool,

    /// Start time filter (HH:MM:SS or YYYY-MM-DD HH:MM:SS or MM-DD HH:MM:SS)
    #[arg(long, value_name = "TIME")]
    since: Option<String>,

    /// End time filter (HH:MM:SS or YYYY-MM-DD HH:MM:SS or MM-DD HH:MM:SS)
    #[arg(long, value_name = "TIME")]
    until: Option<String>,

    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    /// Case-insensitive matching
    #[arg(short = 'i', long)]
    ignore_case: bool,

    /// Invert match (exclude matching lines)
    #[arg(short = 'v', long)]
    invert: bool,

    /// Use AND logic for same-type filters (default is OR)
    #[arg(long)]
    and: bool,

    /// Boolean expression filter (repeatable, OR between multiple -e)
    #[arg(short = 'e', long = "expr", value_name = "EXPR")]
    expr: Vec<String>,

    /// Show NUM lines of context around each match
    #[arg(short = 'C', long = "context", value_name = "NUM")]
    context: Option<usize>,

    /// Show NUM lines after each match
    #[arg(short = 'A', long = "after-context", value_name = "NUM")]
    after_context: Option<usize>,

    /// Show NUM lines before each match
    #[arg(short = 'B', long = "before-context", value_name = "NUM")]
    before_context: Option<usize>,

    /// Deduplicate: group similar lines, show count + time range
    #[arg(long)]
    dedupe: bool,

    /// Merge multi-line entries (e.g. stack traces) into one logical entry
    #[arg(short = 'M', long)]
    multiline: bool,

    /// Extract crashes as structured JSON (implies --multiline)
    #[arg(long)]
    crashes: bool,

    /// Show last N matched entries (combine with --limit for head+tail)
    #[arg(long, value_name = "N", default_value_t = 0)]
    tail: usize,

    /// Uniformly sample N entries from all matches (reservoir sampling)
    #[arg(long, value_name = "N", default_value_t = 0)]
    sample: usize,

    /// Filter by PID (regex, repeatable, OR logic within)
    #[arg(long, value_name = "REGEX")]
    pid: Vec<String>,

    /// Filter by TID (regex, repeatable, OR logic within)
    #[arg(long, value_name = "REGEX")]
    tid: Vec<String>,

    /// Time bucket histogram: group entries by interval (e.g. 10s, 1m, 5m)
    #[arg(long, value_name = "INTERVAL")]
    histogram: Option<String>,

    /// Select output fields: timestamp,pid,tid,level,tag,msg (comma-separated)
    #[arg(long, value_name = "FIELDS")]
    fields: Option<String>,

    /// Sort entries by timestamp across multiple files
    #[arg(long)]
    sort_time: bool,

    /// Show context by time window (e.g. 5s, 10s) instead of line count
    #[arg(long, value_name = "DURATION")]
    time_context: Option<String>,
}

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

    // Pass 2: output lines within any window
    let window = window_secs as f64;
    let mut matched = 0usize;
    let mut last_printed = false;

    for (i, line) in all_lines.iter().enumerate() {
        let in_window = timed[i].secs.map_or(false, |s| {
            match_secs.iter().any(|&ms| (s - ms).abs() <= window)
        });

        if in_window {
            if !last_printed && matched > 0 {
                let _ = writeln!(out, "--");
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
            Err(_) => break,
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
            Err(_) => break,
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
                    let _ = writeln!(out, "--");
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

fn main() {
    let cli = Cli::parse();

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
    let formatter = Formatter::new(cli.format, use_color, &filter_chain, fields);

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
        let mut all_lines: Vec<String> = Vec::new();

        if cli.file.is_empty() {
            let stdin = io::stdin();
            for line in BufReader::new(stdin.lock()).lines() {
                match line {
                    Ok(l) => all_lines.push(l),
                    Err(_) => break,
                }
            }
        } else {
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
                    for line in BufReader::new(file).lines() {
                        match line {
                            Ok(l) => all_lines.push(l),
                            Err(_) => break,
                        }
                    }
                }
            }
        }

        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        matched = run_time_context(&all_lines, &filter_chain, &formatter, &cli, window_secs, &mut out);
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

    if cli.file.is_empty() {
        let stdout = io::stdout();
        let mut out = LineWriter::new(stdout.lock());
        let stdin = io::stdin();
        dispatch_lines!(BufReader::new(stdin.lock()).lines(), &mut out);
        finish_output(&mut out, &formatter, &cli, matched, summary, deduper, hist, sampler, crash_detector.as_ref());
    } else if cli.sort_time {
        // --sort-time: read all lines, sort by timestamp, then process
        let mut all_lines: Vec<String> = Vec::new();
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
                for line in BufReader::new(file).lines().flatten() {
                    all_lines.push(line);
                }
            }
        }
        // Sort by timestamp (lexicographic on time_full, fallback to original order)
        all_lines.sort_by(|a, b| {
            let ta = LogEntry::parse(a).and_then(|e| e.time_full().map(|s| s.to_string()));
            let tb = LogEntry::parse(b).and_then(|e| e.time_full().map(|s| s.to_string()));
            ta.cmp(&tb)
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
