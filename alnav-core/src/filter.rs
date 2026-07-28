use regex::Regex;

use crate::expr::Expr;
use crate::parser::{Level, LogEntry};
use crate::Cli;

/// Time filter that auto-detects user input format.
///
/// - `HH:MM:SS` → compare against `time_hms()` (time only)
/// - `YYYY-MM-DD HH:MM:SS` → compare against `time_full()` (full datetime)
/// - `MM-DD HH:MM:SS` → compare against `time_full()` (threadtime date+time)
enum TimeFilter {
    /// HH:MM:SS — compare against time_hms()
    TimeOnly(String),
    /// Anything longer — compare against time_full()
    Full(String),
}

impl TimeFilter {
    fn parse(input: &str) -> Result<Self, String> {
        let s = input.trim();
        if s.is_empty() {
            return Err("empty time value".to_string());
        }
        // HH:MM:SS (8 chars, e.g. "10:30:00")
        if s.len() == 8 && s.as_bytes()[2] == b':' && s.as_bytes()[5] == b':' {
            Ok(TimeFilter::TimeOnly(s.to_string()))
        } else {
            // YYYY-MM-DD HH:MM:SS (19 chars) or MM-DD HH:MM:SS (14 chars) or other
            Ok(TimeFilter::Full(s.to_string()))
        }
    }

    /// Check if entry timestamp >= this filter value.
    fn entry_gte(&self, entry: &LogEntry) -> bool {
        match self {
            TimeFilter::TimeOnly(t) => entry.time_hms().map_or(true, |hms| hms >= t.as_str()),
            TimeFilter::Full(t) => entry.time_full().map_or(true, |full| full >= t.as_str()),
        }
    }

    /// Check if entry timestamp <= this filter value.
    fn entry_lte(&self, entry: &LogEntry) -> bool {
        match self {
            TimeFilter::TimeOnly(t) => entry.time_hms().map_or(true, |hms| hms <= t.as_str()),
            TimeFilter::Full(t) => entry.time_full().map_or(true, |full| full <= t.as_str()),
        }
    }
}

pub struct FilterChain {
    tag_filters: Vec<Regex>,
    msg_filters: Vec<Regex>,
    package_filters: Vec<Regex>,
    pid_filters: Vec<Regex>,
    tid_filters: Vec<Regex>,
    min_level: Option<Level>,
    since: Option<TimeFilter>,
    until: Option<TimeFilter>,
    use_and: bool,
    exprs: Vec<Expr>,
}

fn compile_patterns(
    patterns: &[String],
    case_flag: &str,
    label: &str,
) -> Result<Vec<Regex>, String> {
    patterns
        .iter()
        .map(|p| {
            Regex::new(&format!("{case_flag}{p}"))
                .map_err(|e| format!("bad --{label} regex '{p}': {e}"))
        })
        .collect()
}

impl FilterChain {
    pub fn from_cli(cli: &Cli) -> Result<Self, String> {
        let case_flag = if cli.ignore_case { "(?i)" } else { "" };

        let tag_filters = compile_patterns(&cli.tag, case_flag, "tag")?;
        let msg_filters = compile_patterns(&cli.msg, case_flag, "msg")?;
        let pid_filters = compile_patterns(&cli.pid, case_flag, "pid")?;
        let tid_filters = compile_patterns(&cli.tid, case_flag, "tid")?;

        let package_filters = cli
            .package
            .iter()
            .map(|p| {
                let escaped = regex::escape(p);
                Regex::new(&format!("{case_flag}{escaped}"))
                    .map_err(|e| format!("bad --package pattern '{p}': {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let min_level = cli
            .level
            .as_ref()
            .map(|l| {
                Level::from_str(l)
                    .ok_or_else(|| format!("unknown level '{}', expected V/D/I/W/E/F", l))
            })
            .transpose()?;

        let exprs = cli
            .expr
            .iter()
            .map(|e| Expr::parse(e, cli.ignore_case))
            .collect::<Result<Vec<_>, _>>()?;

        let since = cli
            .since
            .as_ref()
            .map(|s| TimeFilter::parse(s))
            .transpose()?;
        let until = cli
            .until
            .as_ref()
            .map(|s| TimeFilter::parse(s))
            .transpose()?;

        Ok(Self {
            tag_filters,
            msg_filters,
            package_filters,
            pid_filters,
            tid_filters,
            min_level,
            since,
            until,
            use_and: cli.and,
            exprs,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.tag_filters.is_empty()
            && self.msg_filters.is_empty()
            && self.package_filters.is_empty()
            && self.pid_filters.is_empty()
            && self.tid_filters.is_empty()
            && self.min_level.is_none()
            && self.since.is_none()
            && self.until.is_none()
            && self.exprs.is_empty()
    }

    pub fn matches(&self, entry: &LogEntry) -> bool {
        self.match_level(entry)
            && self.match_time(entry)
            && self.match_group(&self.tag_filters, entry.tag)
            && self.match_group(&self.msg_filters, entry.msg)
            && self.match_group(&self.pid_filters, entry.pid)
            && self.match_group(&self.tid_filters, entry.tid)
            && self.match_package(entry)
            && self.match_exprs(entry)
    }

    fn match_group(&self, filters: &[Regex], value: &str) -> bool {
        if filters.is_empty() {
            return true;
        }
        if self.use_and {
            filters.iter().all(|re| re.is_match(value))
        } else {
            filters.iter().any(|re| re.is_match(value))
        }
    }

    fn match_level(&self, entry: &LogEntry) -> bool {
        self.min_level.map_or(true, |min| entry.level >= min)
    }

    fn match_time(&self, entry: &LogEntry) -> bool {
        if let Some(ref since) = self.since {
            if !since.entry_gte(entry) {
                return false;
            }
        }
        if let Some(ref until) = self.until {
            if !until.entry_lte(entry) {
                return false;
            }
        }
        true
    }

    fn match_package(&self, entry: &LogEntry) -> bool {
        if self.package_filters.is_empty() {
            return true;
        }
        if !entry.pkg.is_empty() {
            self.package_filters.iter().any(|re| re.is_match(entry.pkg))
        } else {
            self.package_filters
                .iter()
                .any(|re| re.is_match(entry.tag) || re.is_match(entry.msg))
        }
    }

    fn match_exprs(&self, entry: &LogEntry) -> bool {
        if self.exprs.is_empty() {
            return true;
        }
        // Multiple -e are OR'd (like grep -e)
        self.exprs.iter().any(|expr| expr.matches(entry))
    }

    pub fn highlight_patterns(&self) -> Vec<&Regex> {
        let mut pats: Vec<&Regex> = self
            .tag_filters
            .iter()
            .chain(self.msg_filters.iter())
            .collect();
        for expr in &self.exprs {
            expr.collect_patterns(&mut pats);
        }
        pats
    }
}
