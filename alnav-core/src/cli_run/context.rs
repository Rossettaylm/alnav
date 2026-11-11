use std::collections::VecDeque;
use std::io;

use crate::dedupe::Deduper;
use crate::filter::FilterChain;
use crate::formatter::Formatter;
use crate::histogram::Histogram;
use crate::parser::LogEntry;
use crate::summary::Summary;
use crate::{Cli, OutputFormat};

use super::context_sizes;

/// Time-based context: two-pass approach.
/// Pass 1: collect all lines, find matching timestamps.
/// Pass 2: output all lines whose timestamp falls within [match_ts - window, match_ts + window].
pub(super) fn run_time_context<W: io::Write>(
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
            let idx = match match_secs
                .binary_search_by(|ms| ms.partial_cmp(&s).unwrap_or(std::cmp::Ordering::Equal))
            {
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

/// Context-aware path: buffer before-lines, print after-lines.
pub(super) fn run_with_context<W: io::Write>(
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
                if cli.invert {
                    !m
                } else {
                    m
                }
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
