use std::fs::File;
use std::io::{self, BufRead, BufReader, LineWriter, Write};
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use glob::glob;

use crate::crash::CrashDetector;
use crate::dedupe::Deduper;
use crate::filter::FilterChain;
use crate::formatter::{FieldSet, Formatter};
use crate::histogram::{self, Histogram};
use crate::multiline::MultilineMerger;
use crate::parser::LogEntry;
use crate::sampler::Sampler;
use crate::summary::Summary;
use crate::{Cli, OutputFormat};

mod context;
mod follow;
mod simple;
#[cfg(test)]
mod tests;

use self::context::run_time_context;
use self::follow::run_follow;
use self::simple::{finish_output, run_lines};

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

fn validate_source(cli: &Cli) -> Result<(), String> {
    let live = cli.hdc || cli.adb;
    if cli.hdc && cli.adb {
        return Err("--hdc and --adb are mutually exclusive".into());
    }
    if live && !cli.file.is_empty() {
        return Err("--hdc/--adb cannot be combined with -f (file input)".into());
    }
    if live && cli.time_context.is_some() {
        return Err(
            "--hdc/--adb cannot be combined with --time-context (requires two-pass)".into(),
        );
    }
    if live && (cli.follow_pid || cli.follow_tid) {
        return Err(
            "--hdc/--adb cannot be combined with --follow-pid/--follow-tid (requires two-pass)"
                .into(),
        );
    }
    if live && cli.sort_time {
        return Err(
            "--hdc/--adb cannot be combined with --sort-time (requires all lines in memory)".into(),
        );
    }
    if cli.device.is_some() && !live {
        return Err("--device requires --hdc or --adb".into());
    }
    Ok(())
}

/// Read all lines from files matching glob patterns into a Vec.
fn collect_file_lines(patterns: &[String]) -> Vec<String> {
    let mut all_lines = Vec::new();
    for pattern in patterns {
        let paths = match glob(pattern) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("alnav: invalid glob '{}': {}", pattern, e);
                process::exit(2);
            }
        };
        for path in paths.flatten() {
            let file = match File::open(&path) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("alnav: cannot open '{}': {}", path.display(), e);
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

fn print_examples() {
    let examples = r"Examples:

  Basic filtering
  adb logcat | alnav grep --tag OkHttp --level W
  alnav grep -f app.log --tag 'OkHttp|Retrofit' --level E
  alnav grep -f app.log --msg error -i              # case-insensitive
  alnav grep -f app.log --tag Debug -v              # invert match
  alnav grep -f app.log --tag A --tag B             # tag=A OR tag=B
  alnav grep -f app.log --tag A --tag B --and       # tag=A AND tag=B

  Boolean expressions (-e)
  alnav grep -f app.log -e 'msg ~ timeout and level >= W'
  alnav grep -f app.log -e '(tag ~ OkHttp or tag ~ Retrofit) and level >= W'
  alnav grep -f app.log -e 'not tag ~ Debug'
  alnav grep -f app.log -e 'tag ~ OkHttp' -e 'tag ~ Retrofit'  # multiple -e = OR
  # Syntax: tag|msg|pkg|pid|tid ~ <regex>, pid|tid == <id>, level >= V|D|I|W|E|F,
  #         time >= T, time <= T (T = HH:MM:SS | MM-DD HH:MM:SS | YYYY-MM-DD HH:MM:SS)
  # Combine with: and, or, not, ( )

  Context lines
  alnav grep -f app.log --tag crash -C 3            # 3 lines before + after
  alnav grep -f app.log -e 'level >= E' -B 5 -A 2  # 5 before, 2 after

  Multi-line merge
  alnav grep -f app.log --tag AndroidRuntime -M     # merge stack traces
  adb logcat | alnav grep -M --level E              # merged error entries

  Crash extraction
  alnav grep -f app.log --crashes                   # all crashes => JSON
  alnav grep -f app.log --crashes --tag MyApp       # filter + extract
  alnav grep -f app.log --crashes --limit 5         # first 5 crashes

  Sampling (manage large output)
  alnav grep -f app.log --level E --tail 50           # last 50 errors
  alnav grep -f app.log --level E --limit 20 --tail 20 # first 20 + last 20
  alnav grep -f app.log --sample 100                  # uniform sample of 100

  Deduplicate (group similar lines)
  alnav grep -f app.log --level E --dedupe          # group errors by pattern
  alnav grep -f app.log --dedupe --limit 20         # top 20 patterns
  alnav grep -f app.log --dedupe --format json      # JSON output for AI
  # Numbers/hex/UUIDs are normalized

  Output formats
  alnav grep -f app.log --tag crash --format json --limit 50
  alnav grep -f app.log --format csv > out.csv
  alnav grep -f app.log --tag crash --count         # print match count only
  alnav grep -f app.log --summary                   # stats + top errors + crash count

  Time range
  alnav grep -f app.log --since 10:30:00 --until 10:35:00
  alnav grep -f app.log --since '2026-03-04 10:30:00' --until '2026-03-04 10:35:00'
  alnav grep -f app.log --since '04-02 12:00:00'      # threadtime date+time

  PID/TID filtering
  alnav grep -f app.log --tid 5678 --level W           # track specific thread (exact)
  alnav grep -f app.log --pid 1234 --tid 5678          # PID + TID combined (exact)
  alnav grep -f app.log -e 'pid == 3542 and level >= E' # expression with pid/tid

  Histogram (time distribution)
  alnav grep -f app.log --histogram 1m                 # level distribution per minute
  alnav grep -f app.log --histogram 10s --level E      # error count per 10 seconds

  Field selection
  alnav grep -f app.log --level E --fields level,tag,msg --format json  # minimal output
  alnav grep -f app.log --fields timestamp,msg         # time + message only

  Time-based context
  alnav grep -f app.log --level F --time-context 5s    # all logs within 5s of fatal errors
  alnav grep -f app.log --tag crash --time-context 10s # 10s window around crash lines

  Multi-file time sort
  alnav grep -f 'logs/*.log' --sort-time --level E     # merge-sort by timestamp

  Follow PID/TID
  alnav grep -f app.log --tag OkHttp --level E --follow-pid  # all logs from error PIDs
  alnav grep -f app.log --crashes --follow-tid                # all logs from crashed threads
";
    println!("{examples}");
}

/// Run the CLI filter pipeline for a parsed [`Cli`].
///
/// Exits the process with code 0 (matches), 1 (no match), or 2 (usage/error).
pub fn run_cli(cli: Cli) {
    if cli.example {
        print_examples();
        return;
    }

    if let Err(error) = validate_source(&cli) {
        eprintln!("alnav: {error}");
        process::exit(2);
    }

    let filter_chain = match FilterChain::from_cli(&cli) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("alnav: {e}");
            process::exit(2);
        }
    };

    let use_color =
        !cli.no_color && cli.format == OutputFormat::Text && atty::is(atty::Stream::Stdout);
    let fields = match cli.fields.as_ref() {
        Some(f) => match FieldSet::parse(f) {
            Ok(fs) => fs,
            Err(e) => {
                eprintln!("alnav: {e}");
                process::exit(2);
            }
        },
        None => FieldSet::all(),
    };
    let formatter = Formatter::new(cli.format, use_color, &filter_chain, fields, &cli.highlight);

    let mut matched: usize = 0;
    let mut summary = if cli.summary {
        Some(Summary::new())
    } else {
        None
    };
    let mut deduper = if cli.dedupe {
        Some(Deduper::new())
    } else {
        None
    };
    let crash_detector = if cli.crashes {
        Some(CrashDetector::new())
    } else {
        None
    };
    let merge = cli.multiline || cli.crashes;

    let mut hist =
        cli.histogram.as_ref().map(
            |interval_str| match histogram::parse_interval(interval_str) {
                Ok(secs) => Histogram::new(secs),
                Err(e) => {
                    eprintln!("alnav: bad --histogram interval: {e}");
                    process::exit(2);
                }
            },
        );

    if cli.tail > 0 && cli.sample > 0 {
        eprintln!("alnav: --tail and --sample are mutually exclusive");
        process::exit(2);
    }
    if (cli.follow_pid || cli.follow_tid) && cli.time_context.is_some() {
        eprintln!("alnav: --follow-pid/--follow-tid cannot be combined with --time-context");
        process::exit(2);
    }
    let mut sampler = Sampler::new(cli.tail, cli.sample, cli.limit);
    let needs_full_scan = sampler.needs_full_scan();

    // Parse --time-context duration
    let time_context_secs =
        cli.time_context
            .as_ref()
            .map(|tc| match histogram::parse_interval(tc) {
                Ok(secs) => secs,
                Err(e) => {
                    eprintln!("alnav: bad --time-context duration: {e}");
                    process::exit(2);
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
        matched = run_time_context(
            &all_lines,
            &filter_chain,
            &formatter,
            &cli,
            window_secs,
            &mut out,
        );
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
                    &filter_chain,
                    &formatter,
                    &cli,
                    &mut matched,
                    &mut summary,
                    &mut deduper,
                    &mut hist,
                    crash_detector.as_ref(),
                    &mut sampler,
                    $out,
                );
            } else {
                run_lines(
                    $lines,
                    &filter_chain,
                    &formatter,
                    &cli,
                    &mut matched,
                    &mut summary,
                    &mut deduper,
                    &mut hist,
                    crash_detector.as_ref(),
                    &mut sampler,
                    $out,
                );
            }
        };
    }

    // Live mode: spawn the selected device logger and read from child stdout.
    if cli.hdc || cli.adb {
        let session = if cli.hdc {
            crate::hdc::spawn_hilog(cli.device.as_deref())
        } else {
            crate::adb::spawn_logcat(cli.device.as_deref())
        };
        let mut session = match session {
            Ok(s) => s,
            Err(e) => {
                eprintln!("alnav: {e}");
                process::exit(2);
            }
        };
        if session.used_history_fallback {
            let source = if cli.hdc { "--hdc" } else { "--adb" };
            eprintln!(
                "alnav: warning: could not query device time, {source} will include buffered history"
            );
        }

        let lines = &mut session.lines;

        let interactive_tty = atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stdout);
        let cbreak_guard = if interactive_tty {
            crate::clearkey::CbreakGuard::enable().ok()
        } else {
            None
        };
        let key_rx = if cbreak_guard.is_some() {
            crate::clearkey::spawn_key_listener()
        } else {
            crate::clearkey::disabled_listener()
        };
        let lines = crate::clearkey::KeypressGate::new(lines, key_rx, |byte| {
            if byte == 0x0C {
                crate::clearkey::write_clear_screen();
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

        let _ = session.child.kill();
        let _ = session.child.wait();

        if interrupted.load(Ordering::SeqCst) {
            let _ = writeln!(io::stderr(), "");
        }

        finish_output(
            &mut out,
            &formatter,
            &cli,
            matched,
            summary,
            deduper,
            hist,
            sampler,
            crash_detector.as_ref(),
        );
        drop(cbreak_guard);
        process::exit(if matched > 0 { 0 } else { 1 });
    }

    if cli.file.is_empty() {
        let stdout = io::stdout();
        let mut out = LineWriter::new(stdout.lock());
        let stdin = io::stdin();
        dispatch_lines!(BufReader::new(stdin.lock()).lines(), &mut out);
        finish_output(
            &mut out,
            &formatter,
            &cli,
            matched,
            summary,
            deduper,
            hist,
            sampler,
            crash_detector.as_ref(),
        );
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
        finish_output(
            &mut out,
            &formatter,
            &cli,
            matched,
            summary,
            deduper,
            hist,
            sampler,
            crash_detector.as_ref(),
        );
    } else {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());

        for pattern in &cli.file {
            let paths = match glob(pattern) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("alnav: invalid glob '{}': {}", pattern, e);
                    process::exit(2);
                }
            };
            for path in paths.flatten() {
                let file = match File::open(&path) {
                    Ok(f) => f,
                    Err(e) => {
                        eprintln!("alnav: cannot open '{}': {}", path.display(), e);
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

        finish_output(
            &mut out,
            &formatter,
            &cli,
            matched,
            summary,
            deduper,
            hist,
            sampler,
            crash_detector.as_ref(),
        );
    }

    process::exit(if matched > 0 { 0 } else { 1 });
}
