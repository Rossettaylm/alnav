use std::io::{self};

use crate::filter::FilterChain;
use crate::formatter::Formatter;
use crate::parser::LogEntry;
use crate::Cli;

/// Follow-PID/TID mode: two-pass approach.
/// Pass 1: apply filters, collect PIDs/TIDs from matches.
/// Pass 2: output all entries whose PID/TID is in the collected set.
pub(super) fn run_follow<W: io::Write>(
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
