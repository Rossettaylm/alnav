use std::io::Write;

use colored::Colorize;

use crate::dedupe::DedupGroup;
use crate::filter::FilterChain;
use crate::parser::{Level, LogEntry};
use crate::OutputFormat;

pub struct Formatter {
    format: OutputFormat,
    use_color: bool,
    highlight_patterns: Vec<regex::Regex>,
}

impl Formatter {
    pub fn new(format: OutputFormat, use_color: bool, chain: &FilterChain) -> Self {
        let highlight_patterns = if use_color {
            chain.highlight_patterns().into_iter().cloned().collect()
        } else {
            vec![]
        };
        Self { format, use_color, highlight_patterns }
    }

    pub fn write_entry<W: Write>(&self, entry: &LogEntry, raw_line: &str, out: &mut W) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Text => self.write_text(entry, raw_line, out),
            OutputFormat::Json => self.write_json(entry, out),
            OutputFormat::Csv => self.write_csv(entry, out),
        }
    }

    fn write_text<W: Write>(&self, entry: &LogEntry, raw_line: &str, out: &mut W) -> std::io::Result<()> {
        if !self.use_color {
            return writeln!(out, "{raw_line}");
        }

        // Per-field coloring for better readability
        let ts_pid = if !entry.timestamp.is_empty() {
            if !entry.tid.is_empty() {
                format!("{} {} {} ", entry.timestamp, entry.pid, entry.tid)
            } else {
                format!("{} {} ", entry.timestamp, entry.pid)
            }
        } else if !entry.pid.is_empty() {
            format!("({}) ", entry.pid)
        } else {
            String::new()
        };

        let level_badge = match entry.level {
            Level::V => format!(" {} ", entry.level.as_char()).white().on_truecolor(100, 100, 100).to_string(),
            Level::D => format!(" {} ", entry.level.as_char()).black().on_blue().to_string(),
            Level::I => format!(" {} ", entry.level.as_char()).black().on_green().to_string(),
            Level::W => format!(" {} ", entry.level.as_char()).black().on_yellow().to_string(),
            Level::E => format!(" {} ", entry.level.as_char()).white().on_red().to_string(),
            Level::F => format!(" {} ", entry.level.as_char()).white().on_red().bold().to_string(),
        };

        let msg = if self.highlight_patterns.is_empty() {
            entry.msg.to_string()
        } else {
            self.highlight_keywords(entry.msg)
        };

        writeln!(
            out,
            "{}{} {}{} {}",
            ts_pid.truecolor(140, 140, 140),
            level_badge,
            entry.tag.bold().cyan(),
            ":".truecolor(140, 140, 140),
            msg,
        )
    }

    fn write_json<W: Write>(&self, entry: &LogEntry, out: &mut W) -> std::io::Result<()> {
        let json = serde_json::to_string(entry).unwrap_or_default();
        writeln!(out, "{json}")
    }

    fn write_csv<W: Write>(&self, entry: &LogEntry, out: &mut W) -> std::io::Result<()> {
        // Write fields directly, only escape msg if it contains quotes
        write!(out, "{},{},{},{},{},\"", entry.timestamp.trim(), entry.pid, entry.tid, entry.level.as_char(), entry.tag)?;
        if entry.msg.contains('"') {
            write!(out, "{}", entry.msg.replace('"', "\"\""))?;
        } else {
            write!(out, "{}", entry.msg)?;
        }
        writeln!(out, "\"")
    }

    fn highlight_keywords(&self, text: &str) -> String {
        let mut result = text.to_string();
        for re in &self.highlight_patterns {
            let replaced = re.replace_all(&result, |caps: &regex::Captures| {
                caps[0].bold().on_truecolor(180, 140, 50).to_string()
            });
            if let std::borrow::Cow::Owned(s) = replaced {
                result = s;
            }
        }
        result
    }

    /// Write a context (non-matching) line — dimmed if color is on.
    pub fn write_context_line<W: Write>(&self, raw_line: &str, out: &mut W) -> std::io::Result<()> {
        if self.use_color {
            writeln!(out, "{}", raw_line.truecolor(110, 110, 110))
        } else {
            writeln!(out, "{raw_line}")
        }
    }

    /// Write a deduplicated group.
    pub fn write_dedupe_group<W: Write>(&self, group: &DedupGroup, out: &mut W) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Text => self.write_dedupe_text(group, out),
            OutputFormat::Json => self.write_dedupe_json(group, out),
            OutputFormat::Csv => self.write_dedupe_csv(group, out),
        }
    }

    fn write_dedupe_text<W: Write>(&self, g: &DedupGroup, out: &mut W) -> std::io::Result<()> {
        let count_str = format!("[{:>5}x]", g.count);
        let level_char = g.level.as_char();
        let time_range = if g.first_ts == g.last_ts || g.last_ts.is_empty() {
            format!("({})", g.first_ts)
        } else {
            format!("({} ~ {})", g.first_ts, g.last_ts)
        };

        if self.use_color {
            let count_colored = if g.count >= 100 {
                count_str.bold().red().to_string()
            } else if g.count >= 10 {
                count_str.bold().yellow().to_string()
            } else {
                count_str.bold().to_string()
            };
            let level_badge = match g.level {
                Level::V => format!(" {level_char} ").white().on_truecolor(100, 100, 100).to_string(),
                Level::D => format!(" {level_char} ").black().on_blue().to_string(),
                Level::I => format!(" {level_char} ").black().on_green().to_string(),
                Level::W => format!(" {level_char} ").black().on_yellow().to_string(),
                Level::E => format!(" {level_char} ").white().on_red().to_string(),
                Level::F => format!(" {level_char} ").white().on_red().bold().to_string(),
            };
            writeln!(
                out,
                "{} {} {}{} {}  {}",
                count_colored,
                level_badge,
                g.tag.bold().cyan(),
                ":".truecolor(140, 140, 140),
                g.pattern,
                time_range.truecolor(110, 110, 110),
            )
        } else {
            writeln!(
                out,
                "{} {} {}: {}  {}",
                count_str, level_char, g.tag, g.pattern, time_range,
            )
        }
    }

    fn write_dedupe_json<W: Write>(&self, g: &DedupGroup, out: &mut W) -> std::io::Result<()> {
        let json = serde_json::to_string(g).unwrap_or_default();
        writeln!(out, "{json}")
    }

    fn write_dedupe_csv<W: Write>(&self, g: &DedupGroup, out: &mut W) -> std::io::Result<()> {
        write!(
            out,
            "{},{},{},\"{}\",\"{}\",{},{}",
            g.count,
            g.level.as_char(),
            g.tag,
            g.pattern.replace('"', "\"\""),
            g.sample_msg.replace('"', "\"\""),
            g.first_ts,
            g.last_ts,
        )?;
        writeln!(out)
    }
}
