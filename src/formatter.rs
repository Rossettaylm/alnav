use std::io::Write;

use colored::Colorize;

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
}
