use regex::Regex;

use crate::parser::{Level, LogEntry};

// ── Token ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    And,
    Or,
    Not,
    LParen,
    RParen,
    Tilde,
    Gte,
    Field(FieldKind),
    Value(String),
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FieldKind {
    Tag,
    Msg,
    Pkg,
    Pid,
    Tid,
    Level,
}

// ── Lexer ────────────────────────────────────────────────────────────

fn tokenize(input: &str) -> Result<Vec<(Token, usize)>, String> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        let pos = i;

        match bytes[i] {
            b'(' => {
                tokens.push((Token::LParen, pos));
                i += 1;
            }
            b')' => {
                tokens.push((Token::RParen, pos));
                i += 1;
            }
            b'~' => {
                tokens.push((Token::Tilde, pos));
                i += 1;
            }
            b'>' if i + 1 < len && bytes[i + 1] == b'=' => {
                tokens.push((Token::Gte, pos));
                i += 2;
            }
            b'"' | b'\'' => {
                let quote = bytes[i];
                i += 1;
                let start = i;
                while i < len && bytes[i] != quote {
                    i += 1;
                }
                if i >= len {
                    return Err(format!("unterminated string at position {pos}"));
                }
                let val = input[start..i].to_string();
                tokens.push((Token::Value(val), pos));
                i += 1;
            }
            _ => {
                let start = i;
                while i < len
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'('
                    && bytes[i] != b')'
                    && bytes[i] != b'~'
                    && bytes[i] != b'>'
                {
                    i += 1;
                }
                let word = &input[start..i];
                let lower = word.to_ascii_lowercase();
                let tok = match lower.as_str() {
                    "and" => Token::And,
                    "or" => Token::Or,
                    "not" => Token::Not,
                    "tag" => Token::Field(FieldKind::Tag),
                    "msg" => Token::Field(FieldKind::Msg),
                    "pkg" => Token::Field(FieldKind::Pkg),
                    "pid" => Token::Field(FieldKind::Pid),
                    "tid" => Token::Field(FieldKind::Tid),
                    "level" => Token::Field(FieldKind::Level),
                    _ => Token::Value(word.to_string()),
                };
                tokens.push((tok, pos));
            }
        }
    }

    tokens.push((Token::Eof, len));
    Ok(tokens)
}

// ── AST ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    TagMatch(Regex),
    MsgMatch(Regex),
    PkgMatch(Regex),
    PidMatch(Regex),
    TidMatch(Regex),
    LevelGte(Level),
}

// ── Parser ───────────────────────────────────────────────────────────

struct ExprParser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    case_insensitive: bool,
}

impl ExprParser {
    fn new(tokens: Vec<(Token, usize)>, case_insensitive: bool) -> Self {
        Self {
            tokens,
            pos: 0,
            case_insensitive,
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].0
    }

    fn advance(&mut self) -> (Token, usize) {
        let tok = self.tokens[self.pos].clone();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<usize, String> {
        let (tok, pos) = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(pos)
        } else {
            Err(format!(
                "expected {:?} at position {}, found {:?}",
                expected, pos, tok
            ))
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        while *self.peek() == Token::Or {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_not()?;
        while *self.peek() == Token::And {
            self.advance();
            let right = self.parse_not()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, String> {
        if *self.peek() == Token::Not {
            self.advance();
            let inner = self.parse_atom()?;
            Ok(Expr::Not(Box::new(inner)))
        } else {
            self.parse_atom()
        }
    }

    fn parse_atom(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(expr)
            }
            Token::Field(kind) => {
                self.advance();
                if kind == FieldKind::Level {
                    self.expect(&Token::Gte)?;
                    let (tok, pos) = self.advance();
                    if let Token::Value(ref v) = tok {
                        let lvl = Level::from_str(v).ok_or_else(|| {
                            format!(
                                "unknown level '{}' at position {}, expected V/D/I/W/E/F",
                                v, pos
                            )
                        })?;
                        Ok(Expr::LevelGte(lvl))
                    } else {
                        Err(format!(
                            "expected level value at position {}, found {:?}",
                            pos, tok
                        ))
                    }
                } else {
                    self.expect(&Token::Tilde)?;
                    let (tok, pos) = self.advance();
                    if let Token::Value(ref v) = tok {
                        let re = self.compile_regex(v, pos)?;
                        match kind {
                            FieldKind::Tag => Ok(Expr::TagMatch(re)),
                            FieldKind::Msg => Ok(Expr::MsgMatch(re)),
                            FieldKind::Pkg => Ok(Expr::PkgMatch(re)),
                            FieldKind::Pid => Ok(Expr::PidMatch(re)),
                            FieldKind::Tid => Ok(Expr::TidMatch(re)),
                            FieldKind::Level => unreachable!(),
                        }
                    } else {
                        Err(format!(
                            "expected value after '~' at position {}, found {:?}",
                            pos, tok
                        ))
                    }
                }
            }
            Token::Eof => Err("unexpected end of expression".to_string()),
            ref t => {
                let pos = self.tokens[self.pos].1;
                Err(format!("unexpected token {:?} at position {}", t, pos))
            }
        }
    }

    fn compile_regex(&self, pattern: &str, pos: usize) -> Result<Regex, String> {
        let pat;
        let regex_input = if self.case_insensitive {
            pat = format!("(?i){pattern}");
            pat.as_str()
        } else {
            pattern
        };
        Regex::new(regex_input)
            .map_err(|e| format!("bad regex '{}' at position {}: {}", pattern, pos, e))
    }
}

// ── Public API ───────────────────────────────────────────────────────

impl Expr {
    /// Parse a boolean expression string into an Expr AST.
    pub fn parse(input: &str, case_insensitive: bool) -> Result<Expr, String> {
        let tokens = tokenize(input)?;
        let mut parser = ExprParser::new(tokens, case_insensitive);
        let expr = parser.parse_expr()?;

        if *parser.peek() != Token::Eof {
            let pos = parser.tokens[parser.pos].1;
            return Err(format!(
                "unexpected token {:?} at position {}",
                parser.peek(),
                pos
            ));
        }

        Ok(expr)
    }

    pub fn matches(&self, entry: &LogEntry) -> bool {
        match self {
            Expr::And(a, b) => a.matches(entry) && b.matches(entry),
            Expr::Or(a, b) => a.matches(entry) || b.matches(entry),
            Expr::Not(inner) => !inner.matches(entry),
            Expr::TagMatch(re) => re.is_match(entry.tag),
            Expr::MsgMatch(re) => re.is_match(entry.msg),
            Expr::PkgMatch(re) => {
                if !entry.pkg.is_empty() {
                    re.is_match(entry.pkg)
                } else {
                    re.is_match(entry.tag) || re.is_match(entry.msg)
                }
            }
            Expr::PidMatch(re) => re.is_match(entry.pid),
            Expr::TidMatch(re) => re.is_match(entry.tid),
            Expr::LevelGte(min) => entry.level >= *min,
        }
    }

    /// Collect regex patterns from all match nodes for keyword highlighting.
    pub fn collect_patterns<'a>(&'a self, out: &mut Vec<&'a Regex>) {
        match self {
            Expr::And(a, b) | Expr::Or(a, b) => {
                a.collect_patterns(out);
                b.collect_patterns(out);
            }
            Expr::Not(inner) => inner.collect_patterns(out),
            Expr::TagMatch(re)
            | Expr::MsgMatch(re)
            | Expr::PkgMatch(re)
            | Expr::PidMatch(re)
            | Expr::TidMatch(re) => out.push(re),
            Expr::LevelGte(_) => {}
        }
    }

    /// Combine scalar field filters into one expression.
    ///
    /// Each field value is treated as a **literal** substring (regex
    /// metacharacters are escaped). Different fields (and `level`) are always
    /// AND'd. Multiple values for the *same* field follow `same_field`:
    /// - [`SameFieldOp::Or`] — alternation of escaped literals (`a|b`), CLI default
    /// - [`SameFieldOp::And`] — separate match nodes AND'd (TUI chip groups)
    ///
    /// Returns `Ok(None)` if every input is empty (no filter to apply).
    pub fn from_filters(
        tag: &[String],
        msg: &[String],
        pkg: &[String],
        pid: &[String],
        tid: &[String],
        level: Option<&str>,
        case_insensitive: bool,
        same_field: SameFieldOp,
    ) -> Result<Option<Expr>, String> {
        let mut nodes: Vec<Expr> = Vec::new();

        Self::push_field_matches(
            &mut nodes,
            tag,
            case_insensitive,
            "tag",
            same_field,
            Expr::TagMatch,
        )?;
        Self::push_field_matches(
            &mut nodes,
            msg,
            case_insensitive,
            "msg",
            same_field,
            Expr::MsgMatch,
        )?;
        Self::push_field_matches(
            &mut nodes,
            pkg,
            case_insensitive,
            "pkg",
            same_field,
            Expr::PkgMatch,
        )?;
        Self::push_field_matches(
            &mut nodes,
            pid,
            case_insensitive,
            "pid",
            same_field,
            Expr::PidMatch,
        )?;
        Self::push_field_matches(
            &mut nodes,
            tid,
            case_insensitive,
            "tid",
            same_field,
            Expr::TidMatch,
        )?;

        if let Some(l) = level {
            let lvl = Level::from_str(l)
                .ok_or_else(|| format!("unknown level '{}', expected V/D/I/W/E/F", l))?;
            nodes.push(Expr::LevelGte(lvl));
        }

        let mut iter = nodes.into_iter();
        let Some(first) = iter.next() else {
            return Ok(None);
        };
        Ok(Some(iter.fold(first, |acc, next| {
            Expr::And(Box::new(acc), Box::new(next))
        })))
    }

    fn push_field_matches(
        nodes: &mut Vec<Expr>,
        values: &[String],
        case_insensitive: bool,
        label: &str,
        same_field: SameFieldOp,
        wrap: fn(Regex) -> Expr,
    ) -> Result<(), String> {
        if values.is_empty() {
            return Ok(());
        }
        match same_field {
            SameFieldOp::Or => {
                if let Some(re) = Self::compile_joined(values, case_insensitive, label)? {
                    nodes.push(wrap(re));
                }
            }
            SameFieldOp::And => {
                for v in values {
                    nodes.push(wrap(Self::compile_one(v, case_insensitive, label)?));
                }
            }
        }
        Ok(())
    }

    fn compile_joined(
        values: &[String],
        case_insensitive: bool,
        label: &str,
    ) -> Result<Option<Regex>, String> {
        if values.is_empty() {
            return Ok(None);
        }
        // Escape each value so user input is a literal substring; `|` between
        // values remains alternation (SameFieldOp::Or).
        let joined = values
            .iter()
            .map(|v| regex::escape(v))
            .collect::<Vec<_>>()
            .join("|");
        let pattern = if case_insensitive {
            format!("(?i){joined}")
        } else {
            joined.clone()
        };
        Regex::new(&pattern)
            .map(Some)
            .map_err(|e| format!("bad {label} pattern '{}': {}", joined, e))
    }

    fn compile_one(value: &str, case_insensitive: bool, label: &str) -> Result<Regex, String> {
        // Literal substring match: metacharacters like `(`, `<`, `|` need no
        // manual escaping from the caller (TUI chips / startup filters).
        let escaped = regex::escape(value);
        let pattern = if case_insensitive {
            format!("(?i){escaped}")
        } else {
            escaped
        };
        Regex::new(&pattern).map_err(|e| format!("bad {label} pattern '{}': {}", value, e))
    }
}

/// How multiple values for the same filter field combine inside [`Expr::from_filters`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SameFieldOp {
    /// `a|b` alternation (CLI / startup multi-flag default).
    Or,
    /// Separate matches AND'd together (TUI chip group within one Enter).
    And,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(tag: &'a str, msg: &'a str, level: Level) -> LogEntry<'a> {
        LogEntry {
            timestamp: "",
            pid: "",
            tid: "",
            level,
            tag,
            pkg: "",
            msg,
        }
    }

    #[test]
    fn test_from_filters_empty_returns_none() {
        let result =
            Expr::from_filters(&[], &[], &[], &[], &[], None, false, SameFieldOp::Or).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_from_filters_single_field() {
        let expr = Expr::from_filters(
            &["MyTag".to_string()],
            &[],
            &[],
            &[],
            &[],
            None,
            false,
            SameFieldOp::Or,
        )
        .unwrap()
        .unwrap();
        assert!(expr.matches(&entry("MyTag", "hello", Level::I)));
        assert!(!expr.matches(&entry("Other", "hello", Level::I)));
    }

    #[test]
    fn test_from_filters_multi_value_same_field_is_or() {
        let expr = Expr::from_filters(
            &["A".to_string(), "B".to_string()],
            &[],
            &[],
            &[],
            &[],
            None,
            false,
            SameFieldOp::Or,
        )
        .unwrap()
        .unwrap();
        assert!(expr.matches(&entry("A", "m", Level::I)));
        assert!(expr.matches(&entry("B", "m", Level::I)));
        assert!(!expr.matches(&entry("C", "m", Level::I)));
    }

    #[test]
    fn test_from_filters_multi_value_same_field_is_and() {
        let expr = Expr::from_filters(
            &[],
            &["trace=".to_string(), "0x1100".to_string()],
            &[],
            &[],
            &[],
            None,
            true,
            SameFieldOp::And,
        )
        .unwrap()
        .unwrap();
        assert!(expr.matches(&entry("T", "foo trace=0x1100 bar", Level::I)));
        assert!(!expr.matches(&entry("T", "foo trace=999 bar", Level::I)));
        assert!(!expr.matches(&entry("T", "foo code=0x1100 bar", Level::I)));
        assert!(!expr.matches(&entry("T", "hello world", Level::I)));
    }

    #[test]
    fn test_from_filters_cross_field_is_and() {
        let expr = Expr::from_filters(
            &["MyTag".to_string()],
            &["timeout".to_string()],
            &[],
            &[],
            &[],
            Some("W"),
            false,
            SameFieldOp::Or,
        )
        .unwrap()
        .unwrap();
        assert!(expr.matches(&entry("MyTag", "timeout occurred", Level::E)));
        assert!(!expr.matches(&entry("MyTag", "all good", Level::E)));
        assert!(!expr.matches(&entry("Other", "timeout occurred", Level::E)));
        assert!(!expr.matches(&entry("MyTag", "timeout occurred", Level::D)));
    }

    #[test]
    fn test_from_filters_unknown_level_errors() {
        let err = Expr::from_filters(
            &[],
            &[],
            &[],
            &[],
            &[],
            Some("bogus"),
            false,
            SameFieldOp::Or,
        )
        .unwrap_err();
        assert!(err.contains("bogus"));
    }

    #[test]
    fn test_from_filters_literal_metacharacters() {
        let expr = Expr::from_filters(
            &[],
            &["(0)".to_string()],
            &[],
            &[],
            &[],
            None,
            true,
            SameFieldOp::And,
        )
        .unwrap()
        .unwrap();
        assert!(expr.matches(&entry("T", "code=(0) ok", Level::I)));
        assert!(!expr.matches(&entry("T", "code=0 ok", Level::I)));

        let expr = Expr::from_filters(
            &[],
            &["foo <bar>".to_string()],
            &[],
            &[],
            &[],
            None,
            false,
            SameFieldOp::And,
        )
        .unwrap()
        .unwrap();
        assert!(expr.matches(&entry("T", "see foo <bar> here", Level::I)));

        // Literal `|` inside one value — not alternation.
        let expr = Expr::from_filters(
            &[],
            &["a|b".to_string()],
            &[],
            &[],
            &[],
            None,
            false,
            SameFieldOp::And,
        )
        .unwrap()
        .unwrap();
        assert!(expr.matches(&entry("T", "x a|b y", Level::I)));
        assert!(!expr.matches(&entry("T", "x a y", Level::I)));
        assert!(!expr.matches(&entry("T", "x b y", Level::I)));
    }

    #[test]
    fn test_from_filters_or_still_alternates_literal_values() {
        let expr = Expr::from_filters(
            &[],
            &["(0)".to_string(), "<tag>".to_string()],
            &[],
            &[],
            &[],
            None,
            false,
            SameFieldOp::Or,
        )
        .unwrap()
        .unwrap();
        assert!(expr.matches(&entry("T", "val=(0)", Level::I)));
        assert!(expr.matches(&entry("T", "see <tag>", Level::I)));
        assert!(!expr.matches(&entry("T", "val=0", Level::I)));
    }
}
