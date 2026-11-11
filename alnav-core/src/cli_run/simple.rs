use std::io;

use crate::crash::{self, CrashDetector};
use crate::dedupe::Deduper;
use crate::filter::FilterChain;
use crate::formatter::Formatter;
use crate::histogram::Histogram;
use crate::parser::LogEntry;
use crate::sampler::Sampler;
use crate::summary::Summary;
use crate::Cli;

use super::context::run_with_context;
use super::use_context;

pub(super) fn run_lines<W: io::Write>(
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
        run_with_context(
            lines,
            filter_chain,
            formatter,
            cli,
            matched,
            summary,
            deduper,
            histogram,
            out,
        );
    } else {
        run_simple(
            lines,
            filter_chain,
            formatter,
            cli,
            matched,
            summary,
            deduper,
            histogram,
            crash_detector,
            sampler,
            out,
        );
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
                if filter_chain.is_empty()
                    && !cli.count
                    && !cli.summary
                    && !cli.dedupe
                    && crash_detector.is_none()
                    && histogram.is_none()
                {
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

pub(super) fn finish_output<W: io::Write>(
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
