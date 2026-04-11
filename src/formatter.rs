use std::io::Write;

use colored::Colorize;

use crate::dedupe::DedupGroup;
use crate::filter::FilterChain;
use crate::parser::{Level, LogEntry};
use crate::OutputFormat;

/// Which fields to include in output.
#[derive(Clone, Copy)]
pub struct FieldSet {
    pub timestamp: bool,
    pub pid: bool,
    pub tid: bool,
    pub level: bool,
    pub tag: bool,
    pub msg: bool,
}

impl FieldSet {
    /// All fields selected (default).
    pub fn all() -> Self {
        Self { timestamp: true, pid: true, tid: true, level: true, tag: true, msg: true }
    }

    /// Parse a comma-separated field list.
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut fs = Self { timestamp: false, pid: false, tid: false, level: false, tag: false, msg: false };
        let mut any = false;
        for field in input.split(',') {
            let f = field.trim().to_ascii_lowercase();
            match f.as_str() {
                "timestamp" | "ts" | "time" => fs.timestamp = true,
                "pid" => fs.pid = true,
                "tid" => fs.tid = true,
                "level" | "lvl" => fs.level = true,
                "tag" => fs.tag = true,
                "msg" | "message" => fs.msg = true,
                "" => continue,
                _ => return Err(format!("unknown field '{}', expected: timestamp,pid,tid,level,tag,msg", f)),
            }
            any = true;
        }
        if !any {
            return Err("--fields requires at least one field".to_string());
        }
        Ok(fs)
    }
}

/// Colored level badge string (e.g. " E " on red background).
fn level_badge(level: Level) -> String {
    let ch = level.as_char();
    match level {
        Level::V => format!(" {ch} ").white().on_truecolor(100, 100, 100).to_string(),
        Level::D => format!(" {ch} ").black().on_blue().to_string(),
        Level::I => format!(" {ch} ").black().on_green().to_string(),
        Level::W => format!(" {ch} ").black().on_yellow().to_string(),
        Level::E => format!(" {ch} ").white().on_red().to_string(),
        Level::F => format!(" {ch} ").white().on_red().bold().to_string(),
    }
}

pub struct Formatter {
    format: OutputFormat,
    use_color: bool,
    highlight_patterns: Vec<regex::Regex>,
    fields: FieldSet,
}

impl Formatter {
    pub fn new(format: OutputFormat, use_color: bool, chain: &FilterChain, fields: FieldSet) -> Self {
        let highlight_patterns = if use_color {
            chain.highlight_patterns().into_iter().cloned().collect()
        } else {
            vec![]
        };
        Self { format, use_color, highlight_patterns, fields }
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
            // If fields are narrowed, build custom output
            if !self.fields.timestamp || !self.fields.pid || !self.fields.tid
                || !self.fields.level || !self.fields.tag || !self.fields.msg {
                return self.write_text_fields(entry, out);
            }
            return writeln!(out, "{raw_line}");
        }

        // If fields are narrowed, build custom colored output
        if !self.fields.timestamp || !self.fields.pid || !self.fields.tid
            || !self.fields.level || !self.fields.tag || !self.fields.msg {
            return self.write_text_fields_color(entry, out);
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

        let level_badge = level_badge(entry.level);

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
        let f = &self.fields;
        write!(out, "{{")?;
        let mut first = true;
        macro_rules! json_field {
            ($name:expr, $val:expr) => {{
                if !first { write!(out, ",")?; }
                first = false;
                write!(out, "\"{}\":\"{}\"", $name, $val)?;
            }};
        }
        macro_rules! json_field_escaped {
            ($name:expr, $val:expr) => {{
                if !first { write!(out, ",")?; }
                first = false;
                write!(out, "\"{}\":\"", $name)?;
                for ch in $val.chars() {
                    match ch {
                        '"' => write!(out, "\\\"")?,
                        '\\' => write!(out, "\\\\")?,
                        _ => write!(out, "{ch}")?,
                    }
                }
                write!(out, "\"")?;
            }};
        }
        if f.timestamp { json_field!("timestamp", entry.timestamp.trim()); }
        if f.pid { json_field!("pid", entry.pid); }
        if f.tid { json_field!("tid", entry.tid); }
        if f.level { json_field!("level", entry.level.as_char()); }
        if f.tag { json_field_escaped!("tag", entry.tag); }
        if f.msg { json_field_escaped!("msg", entry.msg); }
        let _ = first;
        writeln!(out, "}}")
    }

    fn write_csv<W: Write>(&self, entry: &LogEntry, out: &mut W) -> std::io::Result<()> {
        let f = &self.fields;
        let mut parts: Vec<String> = Vec::new();
        if f.timestamp { parts.push(entry.timestamp.trim().to_string()); }
        if f.pid { parts.push(entry.pid.to_string()); }
        if f.tid { parts.push(entry.tid.to_string()); }
        if f.level { parts.push(entry.level.as_char().to_string()); }
        if f.tag { parts.push(entry.tag.to_string()); }
        if f.msg {
            if entry.msg.contains('"') {
                parts.push(format!("\"{}\"", entry.msg.replace('"', "\"\"")));
            } else {
                parts.push(format!("\"{}\"", entry.msg));
            }
        }
        writeln!(out, "{}", parts.join(","))
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

    /// Write selected fields as plain text (no color).
    fn write_text_fields<W: Write>(&self, entry: &LogEntry, out: &mut W) -> std::io::Result<()> {
        let f = &self.fields;
        let mut parts: Vec<&str> = Vec::new();
        let level_str;
        if f.timestamp && !entry.timestamp.is_empty() { parts.push(entry.timestamp.trim()); }
        if f.pid && !entry.pid.is_empty() { parts.push(entry.pid); }
        if f.tid && !entry.tid.is_empty() { parts.push(entry.tid); }
        if f.level { level_str = entry.level.as_char().to_string(); parts.push(&level_str); }
        if f.tag { parts.push(entry.tag); }
        if f.msg { parts.push(entry.msg); }
        writeln!(out, "{}", parts.join(" "))
    }

    /// Write selected fields with color.
    fn write_text_fields_color<W: Write>(&self, entry: &LogEntry, out: &mut W) -> std::io::Result<()> {
        let f = &self.fields;
        let mut buf = String::new();
        if f.timestamp && !entry.timestamp.is_empty() {
            buf.push_str(&entry.timestamp.trim().truecolor(140, 140, 140).to_string());
            buf.push(' ');
        }
        if f.pid && !entry.pid.is_empty() {
            buf.push_str(&entry.pid.truecolor(140, 140, 140).to_string());
            buf.push(' ');
        }
        if f.tid && !entry.tid.is_empty() {
            buf.push_str(&entry.tid.truecolor(140, 140, 140).to_string());
            buf.push(' ');
        }
        if f.level {
            buf.push_str(&level_badge(entry.level));
            buf.push(' ');
        }
        if f.tag {
            buf.push_str(&entry.tag.bold().cyan().to_string());
            buf.push_str(&":".truecolor(140, 140, 140).to_string());
            buf.push(' ');
        }
        if f.msg {
            if self.highlight_patterns.is_empty() {
                buf.push_str(entry.msg);
            } else {
                buf.push_str(&self.highlight_keywords(entry.msg));
            }
        }
        writeln!(out, "{buf}")
    }

    /// Write a context (non-matching) line — format-aware.
    pub fn write_context_line<W: Write>(&self, raw_line: &str, out: &mut W) -> std::io::Result<()> {
        match self.format {
            OutputFormat::Json => self.write_context_json(raw_line, out),
            OutputFormat::Text => {
                if self.use_color {
                    writeln!(out, "{}", raw_line.truecolor(110, 110, 110))
                } else {
                    writeln!(out, "{raw_line}")
                }
            }
            OutputFormat::Csv => writeln!(out, "{raw_line}"),
        }
    }

    fn write_context_json<W: Write>(&self, raw_line: &str, out: &mut W) -> std::io::Result<()> {
        if let Some(entry) = LogEntry::parse(raw_line) {
            let f = &self.fields;
            write!(out, "{{\"context\":true")?;
            if f.timestamp {
                write!(out, ",\"timestamp\":\"{}\"", entry.timestamp.trim())?;
            }
            if f.pid {
                write!(out, ",\"pid\":\"{}\"", entry.pid)?;
            }
            if f.tid {
                write!(out, ",\"tid\":\"{}\"", entry.tid)?;
            }
            if f.level {
                write!(out, ",\"level\":\"{}\"", entry.level.as_char())?;
            }
            if f.tag {
                write!(out, ",\"tag\":\"")?;
                for ch in entry.tag.chars() {
                    match ch {
                        '"' => write!(out, "\\\"")?,
                        '\\' => write!(out, "\\\\")?,
                        _ => write!(out, "{ch}")?,
                    }
                }
                write!(out, "\"")?;
            }
            if f.msg {
                write!(out, ",\"msg\":\"")?;
                for ch in entry.msg.chars() {
                    match ch {
                        '"' => write!(out, "\\\"")?,
                        '\\' => write!(out, "\\\\")?,
                        _ => write!(out, "{ch}")?,
                    }
                }
                write!(out, "\"")?;
            }
            writeln!(out, "}}")
        } else {
            write!(out, "{{\"context\":true,\"raw\":\"")?;
            for ch in raw_line.chars() {
                match ch {
                    '"' => write!(out, "\\\"")?,
                    '\\' => write!(out, "\\\\")?,
                    _ => write!(out, "{ch}")?,
                }
            }
            writeln!(out, "\"}}")
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
            let badge = level_badge(g.level);
            writeln!(
                out,
                "{} {} {}{} {}  {}",
                count_colored,
                badge,
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
