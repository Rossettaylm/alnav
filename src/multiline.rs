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

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_lines(lines: &[&str]) -> Vec<io::Result<String>> {
        lines.iter().map(|s| Ok(s.to_string())).collect()
    }

    #[test]
    fn test_no_continuation() {
        let input = ok_lines(&[
            "04-02 12:34:56.789  1234  5678 E Tag     : msg1",
            "04-02 12:34:57.000  1234  5678 W Tag     : msg2",
        ]);
        let merged: Vec<String> = MultilineMerger::new(input.into_iter())
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(merged.len(), 2);
        assert!(!merged[0].contains('\n'));
    }

    #[test]
    fn test_merge_stack_trace() {
        let input = ok_lines(&[
            "04-02 12:34:56.789  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main",
            "    at com.app.Foo.bar(Foo.java:12)",
            "    at com.app.Baz.qux(Baz.java:34)",
            "04-02 12:34:57.000  1234  5678 W Tag     : next entry",
        ]);
        let merged: Vec<String> = MultilineMerger::new(input.into_iter())
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(merged.len(), 2);
        assert!(merged[0].contains("FATAL EXCEPTION"));
        assert!(merged[0].contains("at com.app.Foo.bar"));
        assert!(merged[0].contains("at com.app.Baz.qux"));
        assert_eq!(merged[0].lines().count(), 3);
    }

    #[test]
    fn test_orphan_continuation() {
        let input = ok_lines(&[
            "    orphan line with no preceding entry",
            "04-02 12:34:56.789  1234  5678 E Tag     : msg",
        ]);
        let merged: Vec<String> = MultilineMerger::new(input.into_iter())
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], "    orphan line with no preceding entry");
    }

    #[test]
    fn test_trailing_continuation() {
        let input = ok_lines(&[
            "04-02 12:34:56.789  1234  5678 E Tag     : first",
            "  continuation 1",
            "  continuation 2",
        ]);
        let merged: Vec<String> = MultilineMerger::new(input.into_iter())
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].lines().count(), 3);
    }
}
