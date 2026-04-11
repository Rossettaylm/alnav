use std::io;
use std::iter::Peekable;

use crate::parser::LogEntry;

/// Iterator adapter that merges continuation lines into the preceding log entry.
///
/// A continuation line is any line that does not parse as a valid `LogEntry`.
/// Such lines are appended (with `\n`) to the previous entry, producing a
/// single string whose `LogEntry::parse()` yields the original metadata while
/// `msg` spans all merged lines.
pub struct MultilineMerger<I: Iterator<Item = io::Result<String>>> {
    lines: Peekable<I>,
}

impl<I: Iterator<Item = io::Result<String>>> MultilineMerger<I> {
    pub fn new(lines: I) -> Self {
        Self { lines: lines.peekable() }
    }
}

impl<I: Iterator<Item = io::Result<String>>> Iterator for MultilineMerger<I> {
    type Item = io::Result<String>;

    fn next(&mut self) -> Option<Self::Item> {
        let first = self.lines.next()?;
        let mut merged = match first {
            Ok(line) => line,
            Err(e) => return Some(Err(e)),
        };

        // If the first line isn't a parseable entry, return as-is
        if LogEntry::parse(&merged).is_none() {
            return Some(Ok(merged));
        }

        // Consume following continuation lines
        loop {
            match self.lines.peek() {
                Some(Ok(next_line)) if LogEntry::parse(next_line).is_none() => {
                    merged.push('\n');
                    merged.push_str(next_line);
                }
                _ => break,
            }
            self.lines.next();
        }

        Some(Ok(merged))
    }
}
