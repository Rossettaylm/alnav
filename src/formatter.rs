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

        // Highlight keywords on raw text first, then apply level color
        let base = if self.highlight_patterns.is_empty() {
            raw_line.to_string()
        } else {
            self.highlight_keywords(raw_line)
        };

        let colored = match entry.level {
            Level::V => base.white().to_string(),
            Level::D => base.blue().to_string(),
            Level::I => base.green().to_string(),
            Level::W => base.yellow().to_string(),
            Level::E => base.red().to_string(),
            Level::F => base.white().on_red().bold().to_string(),
        };

        writeln!(out, "{colored}")
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
                caps[0].bold().underline().to_string()
            });
            if let std::borrow::Cow::Owned(s) = replaced {
                result = s;
            }
        }
        result
    }
}
