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
            TimeFilter::TimeOnly(t) => {
                entry.time_hms().map_or(true, |hms| hms >= t.as_str())
            }
            TimeFilter::Full(t) => {
                entry.time_full().map_or(true, |full| full >= t.as_str())
            }
        }
    }

    /// Check if entry timestamp <= this filter value.
    fn entry_lte(&self, entry: &LogEntry) -> bool {
        match self {
            TimeFilter::TimeOnly(t) => {
                entry.time_hms().map_or(true, |hms| hms <= t.as_str())
            }
            TimeFilter::Full(t) => {
                entry.time_full().map_or(true, |full| full <= t.as_str())
            }
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

fn compile_patterns(patterns: &[String], case_flag: &str, label: &str) -> Result<Vec<Regex>, String> {
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
            .map(|l| Level::from_str(l).ok_or_else(|| format!("unknown level '{}', expected V/D/I/W/E/F", l)))
            .transpose()?;

        let exprs = cli
            .expr
            .iter()
            .map(|e| Expr::parse(e, cli.ignore_case))
            .collect::<Result<Vec<_>, _>>()?;

        let since = cli.since.as_ref().map(|s| TimeFilter::parse(s)).transpose()?;
        let until = cli.until.as_ref().map(|s| TimeFilter::parse(s)).transpose()?;

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
        self.package_filters.iter().any(|re| re.is_match(entry.tag) || re.is_match(entry.msg))
    }

    fn match_exprs(&self, entry: &LogEntry) -> bool {
        if self.exprs.is_empty() {
            return true;
        }
        // Multiple -e are OR'd (like grep -e)
        self.exprs.iter().any(|expr| expr.matches(entry))
    }

    pub fn highlight_patterns(&self) -> Vec<&Regex> {
        let mut pats: Vec<&Regex> = self.tag_filters.iter().chain(self.msg_filters.iter()).collect();
        for expr in &self.exprs {
            expr.collect_patterns(&mut pats);
        }
        pats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OutputFormat;

    fn make_entry(level: Level, tag: &str, msg: &str) -> String {
        format!("04-02 12:34:56.789  1234  5678 {} {:<8}: {}", level.as_char(), tag, msg)
    }

    fn build_chain_with(
        tags: &[&str],
        msgs: &[&str],
        level: Option<&str>,
        packages: &[&str],
        and: bool,
    ) -> FilterChain {
        let cli = Cli {
            tag: tags.iter().map(|s| s.to_string()).collect(),
            msg: msgs.iter().map(|s| s.to_string()).collect(),
            level: level.map(|s| s.to_string()),
            package: packages.iter().map(|s| s.to_string()).collect(),
            file: vec![],
            format: OutputFormat::Text,
            limit: 0,
            count: false,
            summary: false,
            since: None,
            until: None,
            no_color: false,
            ignore_case: false,
            invert: false,
            and,
            expr: vec![],
            context: None,
            after_context: None,
            before_context: None,
            dedupe: false,
            multiline: false,
            crashes: false,
            tail: 0,
            sample: 0,
            pid: vec![],
            tid: vec![],
            histogram: None,
            fields: None,
            sort_time: false,
            time_context: None,
        };
        FilterChain::from_cli(&cli).unwrap()
    }

    fn build_chain(tags: &[&str], msgs: &[&str], level: Option<&str>, packages: &[&str]) -> FilterChain {
        build_chain_with(tags, msgs, level, packages, false)
    }

    #[test]
    fn test_empty_chain_matches_all() {
        let chain = build_chain(&[], &[], None, &[]);
        let line = make_entry(Level::D, "Tag", "hello");
        assert!(chain.matches(&LogEntry::parse(&line).unwrap()));
    }

    #[test]
    fn test_tag_filter() {
        let chain = build_chain(&["OkHttp"], &[], None, &[]);
        let hit = make_entry(Level::D, "OkHttp", "request");
        let miss = make_entry(Level::D, "MyApp", "request");
        assert!(chain.matches(&LogEntry::parse(&hit).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(&miss).unwrap()));
    }

    #[test]
    fn test_tag_or_logic() {
        let chain = build_chain(&["OkHttp|Retrofit"], &[], None, &[]);
        assert!(chain.matches(&LogEntry::parse(&make_entry(Level::D, "OkHttp", "m")).unwrap()));
        assert!(chain.matches(&LogEntry::parse(&make_entry(Level::D, "Retrofit", "m")).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::D, "Other", "m")).unwrap()));
    }

    #[test]
    fn test_cross_type_and_logic() {
        let chain = build_chain(&["OkHttp"], &["error"], None, &[]);
        assert!(chain.matches(&LogEntry::parse(&make_entry(Level::D, "OkHttp", "error occurred")).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::D, "OkHttp", "success")).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::D, "MyApp", "error occurred")).unwrap()));
    }

    #[test]
    fn test_level_filter() {
        let chain = build_chain(&[], &[], Some("W"), &[]);
        assert!(chain.matches(&LogEntry::parse(&make_entry(Level::W, "T", "m")).unwrap()));
        assert!(chain.matches(&LogEntry::parse(&make_entry(Level::E, "T", "m")).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::D, "T", "m")).unwrap()));
    }

    #[test]
    fn test_msg_and_logic() {
        let chain = build_chain_with(&[], &["mobile_msf", "0x9293"], None, &[], true);
        assert!(chain.matches(&LogEntry::parse(&make_entry(Level::I, "NT", "mobile_msf cmd:0x9293 done")).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::I, "NT", "mobile_msf cmd:0xfe1 done")).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(&make_entry(Level::I, "NT", "other cmd:0x9293 done")).unwrap()));
    }

    fn build_chain_time(since: Option<&str>, until: Option<&str>) -> FilterChain {
        let cli = Cli {
            tag: vec![], msg: vec![], level: None, package: vec![],
            file: vec![], format: OutputFormat::Text, limit: 0, count: false,
            summary: false, since: since.map(|s| s.to_string()),
            until: until.map(|s| s.to_string()), no_color: false,
            ignore_case: false, invert: false, and: false, expr: vec![],
            context: None, after_context: None, before_context: None,
            dedupe: false, multiline: false, crashes: false, tail: 0, sample: 0,
            pid: vec![], tid: vec![], histogram: None, fields: None,
            sort_time: false, time_context: None,
        };
        FilterChain::from_cli(&cli).unwrap()
    }

    #[test]
    fn test_time_hms_since_until() {
        let chain = build_chain_time(Some("12:00:00"), Some("13:00:00"));
        let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
        let before = "04-02 11:59:59.999  1234  5678 D Tag     : msg";
        let after = "04-02 13:00:01.000  1234  5678 D Tag     : msg";
        assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(before).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(after).unwrap()));
    }

    #[test]
    fn test_time_full_datetime_xlog() {
        let chain = build_chain_time(
            Some("2026-03-04 10:30:00"),
            Some("2026-03-04 10:35:00"),
        );
        let hit = "2026-03-04 10:32:00.000|1[3542]3831|3542|I|Tag|msg";
        let before = "2026-03-04 10:29:59.000|1[3542]3831|3542|I|Tag|msg";
        let after = "2026-03-04 10:35:01.000|1[3542]3831|3542|I|Tag|msg";
        assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(before).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(after).unwrap()));
    }

    #[test]
    fn test_time_full_date_threadtime() {
        // MM-DD HH:MM:SS format for threadtime
        let chain = build_chain_time(
            Some("04-02 12:00:00"),
            Some("04-02 13:00:00"),
        );
        let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
        let before = "04-01 23:59:59.999  1234  5678 D Tag     : msg";
        let after = "04-03 00:00:01.000  1234  5678 D Tag     : msg";
        assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(before).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(after).unwrap()));
    }

    #[test]
    fn test_time_full_xlog_cross_day() {
        // Can filter across days with full datetime
        let chain = build_chain_time(
            Some("2026-03-03 23:00:00"),
            Some("2026-03-04 01:00:00"),
        );
        let hit = "2026-03-03 23:30:00.000|1[3542]3831|3542|I|Tag|msg";
        let miss = "2026-03-04 02:00:00.000|1[3542]3831|3542|I|Tag|msg";
        assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
    }

    #[test]
    fn test_pid_filter() {
        let cli = Cli {
            tag: vec![], msg: vec![], level: None, package: vec![],
            file: vec![], format: OutputFormat::Text, limit: 0, count: false,
            summary: false, since: None, until: None, no_color: false,
            ignore_case: false, invert: false, and: false, expr: vec![],
            context: None, after_context: None, before_context: None,
            dedupe: false, multiline: false, crashes: false, tail: 0, sample: 0,
            pid: vec!["1234".to_string()], tid: vec![],
            histogram: None, fields: None, sort_time: false, time_context: None,
        };
        let chain = FilterChain::from_cli(&cli).unwrap();
        let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
        let miss = "04-02 12:34:56.789  9999  5678 D Tag     : msg";
        assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
    }

    #[test]
    fn test_tid_filter() {
        let cli = Cli {
            tag: vec![], msg: vec![], level: None, package: vec![],
            file: vec![], format: OutputFormat::Text, limit: 0, count: false,
            summary: false, since: None, until: None, no_color: false,
            ignore_case: false, invert: false, and: false, expr: vec![],
            context: None, after_context: None, before_context: None,
            dedupe: false, multiline: false, crashes: false, tail: 0, sample: 0,
            pid: vec![], tid: vec!["5678".to_string()],
            histogram: None, fields: None, sort_time: false, time_context: None,
        };
        let chain = FilterChain::from_cli(&cli).unwrap();
        let hit = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
        let miss = "04-02 12:34:56.789  1234  9999 D Tag     : msg";
        assert!(chain.matches(&LogEntry::parse(hit).unwrap()));
        assert!(!chain.matches(&LogEntry::parse(miss).unwrap()));
    }
}
