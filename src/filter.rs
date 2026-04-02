use regex::Regex;

use crate::expr::Expr;
use crate::parser::{Level, LogEntry};
use crate::Cli;

pub struct FilterChain {
    tag_filters: Vec<Regex>,
    msg_filters: Vec<Regex>,
    package_filters: Vec<Regex>,
    min_level: Option<Level>,
    since: Option<String>,
    until: Option<String>,
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

        Ok(Self {
            tag_filters,
            msg_filters,
            package_filters,
            min_level,
            since: cli.since.clone(),
            until: cli.until.clone(),
            use_and: cli.and,
            exprs,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.tag_filters.is_empty()
            && self.msg_filters.is_empty()
            && self.package_filters.is_empty()
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
        let hms = match entry.time_hms() {
            Some(t) => t,
            None => return true,
        };
        if let Some(ref since) = self.since {
            if hms < since.as_str() {
                return false;
            }
        }
        if let Some(ref until) = self.until {
            if hms > until.as_str() {
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
}
