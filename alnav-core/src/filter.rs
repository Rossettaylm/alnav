use regex::Regex;

use crate::expr::{Expr, TimePoint};
use crate::parser::{Level, LogEntry};
use crate::Cli;

pub struct FilterChain {
    tag_filters: Vec<Regex>,
    msg_filters: Vec<Regex>,
    package_filters: Vec<Regex>,
    pid_filters: Vec<String>,
    tid_filters: Vec<String>,
    min_level: Option<Level>,
    since: Option<TimePoint>,
    until: Option<TimePoint>,
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
        let pid_filters = cli.pid.iter().map(|p| p.trim().to_string()).collect();
        let tid_filters = cli.tid.iter().map(|t| t.trim().to_string()).collect();

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

        let since = cli.since.as_deref().map(TimePoint::parse).transpose()?;
        let until = cli.until.as_deref().map(TimePoint::parse).transpose()?;

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
            && self.match_ids(&self.pid_filters, entry.pid)
            && self.match_ids(&self.tid_filters, entry.tid)
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

    /// `--pid` / `--tid`: exact id equality.
    fn match_ids(&self, ids: &[String], value: &str) -> bool {
        if ids.is_empty() {
            return true;
        }
        if self.use_and {
            ids.iter().all(|id| id == value)
        } else {
            ids.iter().any(|id| id == value)
        }
    }

    fn match_level(&self, entry: &LogEntry) -> bool {
        self.min_level.map_or(true, |min| entry.level >= min)
    }

    fn match_time(&self, entry: &LogEntry) -> bool {
        self.since.as_ref().map_or(true, |t| t.entry_ge(entry))
            && self.until.as_ref().map_or(true, |t| t.entry_le(entry))
    }

    /// `--package`: literal substring of the entry's package; entries without
    /// a package (threadtime / brief) never match.
    fn match_package(&self, entry: &LogEntry) -> bool {
        if self.package_filters.is_empty() {
            return true;
        }
        if entry.pkg.is_empty() {
            return false;
        }
        self.package_filters.iter().any(|re| re.is_match(entry.pkg))
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
