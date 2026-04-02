mod filter;
mod formatter;
mod parser;
mod summary;

use std::fs::File;
use std::io::{self, BufRead, BufReader, LineWriter};
use std::process;

use clap::{Parser, ValueEnum};
use glob::glob;

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
#[command(name = "loggrep", about = "Lightweight Android logcat filter & analyzer")]
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
}

/// Process a single line. Returns false to stop reading.
fn process_line<W: io::Write>(
    line: &str,
    filter_chain: &FilterChain,
    formatter: &Formatter,
    cli: &Cli,
    matched: &mut usize,
    summary: &mut Option<Summary>,
    out: &mut W,
) -> bool {
    let entry = match LogEntry::parse(line) {
        Some(e) => e,
        None => {
            if filter_chain.is_empty() && !cli.count && !cli.summary {
                let _ = writeln!(out, "{line}");
            }
            return true;
        }
    };

    let is_match = filter_chain.matches(&entry);
    let is_match = if cli.invert { !is_match } else { is_match };

    if is_match {
        *matched += 1;
        if let Some(ref mut s) = summary {
            s.record(&entry);
        } else if !cli.count {
            let _ = formatter.write_entry(&entry, line, out);
        }
        if cli.limit > 0 && *matched >= cli.limit {
            return false;
        }
    }
    true
}

fn finish_output<W: io::Write>(out: &mut W, cli: &Cli, matched: usize, summary: Option<Summary>) {
    if cli.count {
        let _ = writeln!(out, "{matched}");
    }
    if let Some(s) = summary {
        let _ = writeln!(out, "{}", s.to_json(matched));
    }
    let _ = out.flush();
}

fn run_reader<R: BufRead, W: io::Write>(
    reader: R,
    filter_chain: &FilterChain,
    formatter: &Formatter,
    cli: &Cli,
    matched: &mut usize,
    summary: &mut Option<Summary>,
    out: &mut W,
) {
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if !process_line(&line, filter_chain, formatter, cli, matched, summary, out) {
            break;
        }
    }
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

    if cli.file.is_empty() {
        let stdout = io::stdout();
        let mut out = LineWriter::new(stdout.lock());
        let stdin = io::stdin();
        run_reader(BufReader::new(stdin.lock()), &filter_chain, &formatter, &cli, &mut matched, &mut summary, &mut out);
        finish_output(&mut out, &cli, matched, summary);
    } else {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());

        'outer: for pattern in &cli.file {
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
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    if !process_line(&line, &filter_chain, &formatter, &cli, &mut matched, &mut summary, &mut out) {
                        break 'outer;
                    }
                }
            }
        }

        finish_output(&mut out, &cli, matched, summary);
    }

    process::exit(if matched > 0 { 0 } else { 1 });
}
