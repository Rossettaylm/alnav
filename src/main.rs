mod dedupe;
mod expr;
mod filter;
mod formatter;
mod parser;
mod summary;

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, LineWriter};
use std::process;

use clap::{Parser, ValueEnum};
use glob::glob;

use dedupe::Deduper;
use filter::FilterChain;
use formatter::Formatter;
use parser::LogEntry;
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

  \x1b[4mDeduplicate (group similar lines)\x1b[0m
  loggrep -f app.log --level E --dedupe          # group errors by pattern
  loggrep -f app.log --dedupe --limit 20         # top 20 patterns
  loggrep -f app.log --dedupe --format json      # JSON output for AI
  # Numbers/hex/UUIDs are normalized: \"timeout 100ms\" ≈ \"timeout 200ms\"

  \x1b[4mOutput formats\x1b[0m
  loggrep -f app.log --tag crash --format json --limit 50
  loggrep -f app.log --format csv > out.csv
  loggrep -f app.log --tag crash --count         # print match count only
  loggrep -f app.log --summary                   # aggregated stats (JSON)

  \x1b[4mTime range\x1b[0m
  loggrep -f app.log --since 10:30:00 --until 10:35:00"
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

    /// Start time filter (HH:MM:SS)
    #[arg(long, value_name = "TIME")]
    since: Option<String>,

    /// End time filter (HH:MM:SS)
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
}

/// Resolve effective before/after context from -C/-A/-B flags.
fn context_sizes(cli: &Cli) -> (usize, usize) {
    let before = cli.before_context.or(cli.context).unwrap_or(0);
    let after = cli.after_context.or(cli.context).unwrap_or(0);
    (before, after)
}

/// Whether context lines should be printed (text mode, not count/summary).
fn use_context(cli: &Cli) -> bool {
    let (b, a) = context_sizes(cli);
    (b > 0 || a > 0) && !cli.count && !cli.summary
}

fn run_lines<W: io::Write>(
    lines: impl Iterator<Item = io::Result<String>>,
    filter_chain: &FilterChain,
    formatter: &Formatter,
    cli: &Cli,
    matched: &mut usize,
    summary: &mut Option<Summary>,
    deduper: &mut Option<Deduper>,
    out: &mut W,
) {
    if use_context(cli) {
        run_with_context(lines, filter_chain, formatter, cli, matched, summary, deduper, out);
    } else {
        run_simple(lines, filter_chain, formatter, cli, matched, summary, deduper, out);
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
                if filter_chain.is_empty() && !cli.count && !cli.summary && !cli.dedupe {
                    let _ = writeln!(out, "{line}");
                }
                continue;
            }
        };

        let is_match = filter_chain.matches(&entry);
        let is_match = if cli.invert { !is_match } else { is_match };

        if is_match {
            *matched += 1;
            if let Some(ref mut s) = summary {
                s.record(&entry);
            } else if let Some(ref mut d) = deduper {
                d.record(&entry);
            } else if !cli.count {
                let _ = formatter.write_entry(&entry, &line, out);
            }
            if cli.limit > 0 && *matched >= cli.limit {
                break;
            }
        }
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

        let is_match = LogEntry::parse(&line)
            .map(|entry| {
                let m = filter_chain.matches(&entry);
                if cli.invert { !m } else { m }
            })
            .unwrap_or(false);

        if is_match {
            *matched += 1;
            if let Some(ref mut s) = summary {
                if let Some(entry) = LogEntry::parse(&line) {
                    s.record(&entry);
                }
            }
            if let Some(ref mut d) = deduper {
                if let Some(entry) = LogEntry::parse(&line) {
                    d.record(&entry);
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
            if let Some(entry) = LogEntry::parse(&line) {
                let _ = formatter.write_entry(&entry, &line, out);
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
) {
    if cli.count {
        let _ = writeln!(out, "{matched}");
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
    let formatter = Formatter::new(cli.format, use_color, &filter_chain);

    let mut matched: usize = 0;
    let mut summary = if cli.summary { Some(Summary::new()) } else { None };
    let mut deduper = if cli.dedupe { Some(Deduper::new()) } else { None };

    if cli.file.is_empty() {
        let stdout = io::stdout();
        let mut out = LineWriter::new(stdout.lock());
        let stdin = io::stdin();
        run_lines(
            BufReader::new(stdin.lock()).lines(),
            &filter_chain, &formatter, &cli, &mut matched, &mut summary, &mut deduper, &mut out,
        );
        finish_output(&mut out, &formatter, &cli, matched, summary, deduper);
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
                run_lines(
                    BufReader::new(file).lines(),
                    &filter_chain, &formatter, &cli, &mut matched, &mut summary, &mut deduper, &mut out,
                );
                if cli.limit > 0 && matched >= cli.limit && !cli.dedupe {
                    break;
                }
            }
            if cli.limit > 0 && matched >= cli.limit && !cli.dedupe {
                break;
            }
        }

        finish_output(&mut out, &formatter, &cli, matched, summary, deduper);
    }

    process::exit(if matched > 0 { 0 } else { 1 });
}
