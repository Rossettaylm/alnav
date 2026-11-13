//! `-e` boolean filter language: lexer, recursive-descent parser, AST evaluator, canonical printer.
//!
//! `Display` prints a canonical form that parses back to an equal `Expr`
//! (given the same case flag), so programmatically built predicates can be
//! exported as `alnav grep -e '…'`.

use std::fmt;

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
    Lte,
    EqEq,
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
    Time,
}

/// Two-byte comparison operator starting at `i`, if any.
fn op_at(bytes: &[u8], i: usize) -> Option<Token> {
    if bytes.get(i + 1) != Some(&b'=') {
        return None;
    }
    match bytes[i] {
        b'>' => Some(Token::Gte),
        b'<' => Some(Token::Lte),
        b'=' => Some(Token::EqEq),
        _ => None,
    }
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

        if let Some(op) = op_at(bytes, i) {
            tokens.push((op, pos));
            i += 2;
            continue;
        }

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
            b'"' | b'\'' => {
                // `\\` and `\<quote>` are escapes; any other backslash is kept
                // verbatim so regex escapes like `\d` need no doubling.
                let quote = bytes[i];
                i += 1;
                let mut val = String::new();
                loop {
                    if i >= len {
                        return Err(format!("unterminated string at position {pos}"));
                    }
                    let b = bytes[i];
                    if b == quote {
                        i += 1;
                        break;
                    }
                    if b == b'\\'
                        && matches!(bytes.get(i + 1), Some(&n) if n == b'\\' || n == quote)
                    {
                        val.push(bytes[i + 1] as char);
                        i += 2;
                        continue;
                    }
                    let ch = input[i..].chars().next().expect("char boundary");
                    val.push(ch);
                    i += ch.len_utf8();
                }
                tokens.push((Token::Value(val), pos));
            }
            _ => {
                let start = i;
                while i < len {
                    let b = bytes[i];
                    if b.is_ascii_whitespace()
                        || b == b'('
                        || b == b')'
                        || b == b'~'
                        || op_at(bytes, i).is_some()
                    {
                        break;
                    }
                    i += 1;
                }
                let word = &input[start..i];
                let tok = match word.to_ascii_lowercase().as_str() {
                    "and" => Token::And,
                    "or" => Token::Or,
                    "not" => Token::Not,
                    "tag" => Token::Field(FieldKind::Tag),
                    "msg" => Token::Field(FieldKind::Msg),
                    "pkg" => Token::Field(FieldKind::Pkg),
                    "pid" => Token::Field(FieldKind::Pid),
                    "tid" => Token::Field(FieldKind::Tid),
                    "level" => Token::Field(FieldKind::Level),
                    "time" => Token::Field(FieldKind::Time),
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

/// Field matched by a `~ REGEX` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextField {
    Tag,
    Msg,
    /// Package (hilog / xlog). Empty package never satisfies a literal pattern.
    Pkg,
    Pid,
    Tid,
}

impl TextField {
    fn keyword(self) -> &'static str {
        match self {
            Self::Tag => "tag",
            Self::Msg => "msg",
            Self::Pkg => "pkg",
            Self::Pid => "pid",
            Self::Tid => "tid",
        }
    }

    fn value<'a>(self, e: &LogEntry<'a>) -> &'a str {
        match self {
            Self::Tag => e.tag,
            Self::Msg => e.msg,
            Self::Pkg => e.pkg,
            Self::Pid => e.pid,
            Self::Tid => e.tid,
        }
    }
}

/// Field compared by an exact `== VALUE` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdField {
    Pid,
    Tid,
}

impl IdField {
    fn keyword(self) -> &'static str {
        match self {
            Self::Pid => "pid",
            Self::Tid => "tid",
        }
    }

    fn value<'a>(self, e: &LogEntry<'a>) -> &'a str {
        match self {
            Self::Pid => e.pid,
            Self::Tid => e.tid,
        }
    }
}

/// Compiled regex plus the source text it was written as (for [`Display`]).
#[derive(Debug, Clone)]
pub struct Pattern {
    src: String,
    re: Regex,
}

impl Pattern {
    fn new(src: String, case_insensitive: bool) -> Result<Self, regex::Error> {
        let re = if case_insensitive {
            Regex::new(&format!("(?i){src}"))?
        } else {
            Regex::new(&src)?
        };
        Ok(Self { src, re })
    }

    pub fn regex(&self) -> &Regex {
        &self.re
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Self) -> bool {
        self.re.as_str() == other.re.as_str()
    }
}

/// `HH:MM:SS` compares against [`LogEntry::time_hms`]; anything else
/// (`MM-DD HH:MM:SS`, `YYYY-MM-DD HH:MM:SS`, …) against [`LogEntry::time_full`].
/// Rows without a timestamp always pass a time comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimePoint {
    value: String,
    time_only: bool,
}

/// True for an 8-char `HH:MM:SS` value.
pub fn is_time_only(s: &str) -> bool {
    s.len() == 8 && s.as_bytes()[2] == b':' && s.as_bytes()[5] == b':'
}

impl TimePoint {
    pub fn parse(input: &str) -> Result<Self, String> {
        let value = input.trim();
        if value.is_empty() {
            return Err("empty time value".to_string());
        }
        Ok(Self {
            time_only: is_time_only(value),
            value: value.to_string(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }

    fn key<'e>(&self, e: &'e LogEntry) -> Option<&'e str> {
        if self.time_only {
            e.time_hms()
        } else {
            e.time_full()
        }
    }

    /// Entry timestamp `>=` this point (or entry has no timestamp).
    pub fn entry_ge(&self, e: &LogEntry) -> bool {
        self.key(e).map_or(true, |k| k >= self.value.as_str())
    }

    /// Entry timestamp `<=` this point (or entry has no timestamp).
    pub fn entry_le(&self, e: &LogEntry) -> bool {
        self.key(e).map_or(true, |k| k <= self.value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Match(TextField, Pattern),
    Eq(IdField, String),
    LevelGte(Level),
    TimeGe(TimePoint),
    TimeLe(TimePoint),
}

// ── Parser ───────────────────────────────────────────────────────────

struct ExprParser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    case_insensitive: bool,
}

impl ExprParser {
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

    fn value(&mut self, what: &str) -> Result<(String, usize), String> {
        match self.advance() {
            (Token::Value(v), pos) => Ok((v, pos)),
            (tok, pos) => Err(format!("expected {what} at position {pos}, found {tok:?}")),
        }
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
        let (tok, pos) = self.advance();
        let kind = match tok {
            Token::LParen => {
                let expr = self.parse_or()?;
                self.expect(&Token::RParen)?;
                return Ok(expr);
            }
            Token::Field(kind) => kind,
            Token::Eof => return Err("unexpected end of expression".to_string()),
            t => return Err(format!("unexpected token {t:?} at position {pos}")),
        };
        let (op, op_pos) = self.advance();
        match (kind, op) {
            (FieldKind::Level, Token::Gte) => {
                let (v, pos) = self.value("level value")?;
                let lvl = Level::from_str(&v).ok_or_else(|| {
                    format!("unknown level '{v}' at position {pos}, expected V/D/I/W/E/F")
                })?;
                Ok(Expr::LevelGte(lvl))
            }
            (FieldKind::Time, op @ (Token::Gte | Token::Lte)) => {
                let (v, pos) = self.value("time value")?;
                let tp = TimePoint::parse(&v).map_err(|e| format!("{e} at position {pos}"))?;
                Ok(if op == Token::Gte {
                    Expr::TimeGe(tp)
                } else {
                    Expr::TimeLe(tp)
                })
            }
            (FieldKind::Pid | FieldKind::Tid, Token::EqEq) => {
                let (v, _) = self.value("value after '=='")?;
                let field = if kind == FieldKind::Pid {
                    IdField::Pid
                } else {
                    IdField::Tid
                };
                Ok(Expr::Eq(field, v))
            }
            (
                FieldKind::Tag | FieldKind::Msg | FieldKind::Pkg | FieldKind::Pid | FieldKind::Tid,
                Token::Tilde,
            ) => {
                let (v, pos) = self.value("value after '~'")?;
                let field = match kind {
                    FieldKind::Tag => TextField::Tag,
                    FieldKind::Msg => TextField::Msg,
                    FieldKind::Pkg => TextField::Pkg,
                    FieldKind::Pid => TextField::Pid,
                    _ => TextField::Tid,
                };
                let pat = Pattern::new(v.clone(), self.case_insensitive)
                    .map_err(|e| format!("bad regex '{v}' at position {pos}: {e}"))?;
                Ok(Expr::Match(field, pat))
            }
            (_, op) => Err(format!(
                "unsupported operator {op:?} at position {op_pos} (tag/msg/pkg/pid/tid ~ REGEX, pid/tid == VALUE, level >= L, time >=|<= T)"
            )),
        }
    }
}

// ── Public API ───────────────────────────────────────────────────────

impl Expr {
    /// Parse a boolean expression string into an Expr AST.
    pub fn parse(input: &str, case_insensitive: bool) -> Result<Expr, String> {
        let tokens = tokenize(input)?;
        let mut parser = ExprParser {
            tokens,
            pos: 0,
            case_insensitive,
        };
        let expr = parser.parse_or()?;
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

    /// `field ~ <escaped literal>`: contiguous substring match of `literal`.
    pub fn literal(field: TextField, literal: &str, case_insensitive: bool) -> Self {
        let pat = Pattern::new(regex::escape(literal), case_insensitive)
            .expect("escaped literal is a valid regex");
        Expr::Match(field, pat)
    }

    pub fn and(self, other: Expr) -> Self {
        Expr::And(Box::new(self), Box::new(other))
    }

    pub fn or(self, other: Expr) -> Self {
        Expr::Or(Box::new(self), Box::new(other))
    }

    pub fn negate(self) -> Self {
        Expr::Not(Box::new(self))
    }

    /// Left-fold with AND; `None` for an empty iterator.
    pub fn all(items: impl IntoIterator<Item = Expr>) -> Option<Self> {
        items.into_iter().reduce(Expr::and)
    }

    /// Left-fold with OR; `None` for an empty iterator.
    pub fn any(items: impl IntoIterator<Item = Expr>) -> Option<Self> {
        items.into_iter().reduce(Expr::or)
    }

    pub fn matches(&self, entry: &LogEntry) -> bool {
        match self {
            Expr::And(a, b) => a.matches(entry) && b.matches(entry),
            Expr::Or(a, b) => a.matches(entry) || b.matches(entry),
            Expr::Not(inner) => !inner.matches(entry),
            Expr::Match(field, pat) => pat.re.is_match(field.value(entry)),
            Expr::Eq(field, v) => field.value(entry) == v,
            Expr::LevelGte(min) => entry.level >= *min,
            Expr::TimeGe(t) => t.entry_ge(entry),
            Expr::TimeLe(t) => t.entry_le(entry),
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
            Expr::Match(_, pat) => out.push(&pat.re),
            Expr::Eq(..) | Expr::LevelGte(_) | Expr::TimeGe(_) | Expr::TimeLe(_) => {}
        }
    }

    /// Binding strength: or < and < not/atom.
    fn precedence(&self) -> u8 {
        match self {
            Expr::Or(..) => 0,
            Expr::And(..) => 1,
            _ => 2,
        }
    }
}

// ── Display ──────────────────────────────────────────────────────────

/// Double-quoted value that the lexer reads back verbatim.
fn write_quoted(f: &mut fmt::Formatter<'_>, s: &str) -> fmt::Result {
    f.write_str("\"")?;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => f.write_str("\\\"")?,
            '\\' if matches!(chars.peek(), None | Some('\\') | Some('"')) => f.write_str("\\\\")?,
            c => write!(f, "{c}")?,
        }
    }
    f.write_str("\"")
}

fn write_child(f: &mut fmt::Formatter<'_>, child: &Expr, min_prec: u8) -> fmt::Result {
    if child.precedence() < min_prec {
        write!(f, "({child})")
    } else {
        write!(f, "{child}")
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Or(a, b) => {
                write_child(f, a, 0)?;
                f.write_str(" or ")?;
                write_child(f, b, 1)
            }
            Expr::And(a, b) => {
                write_child(f, a, 1)?;
                f.write_str(" and ")?;
                write_child(f, b, 2)
            }
            Expr::Not(inner) => {
                f.write_str("not ")?;
                match inner.as_ref() {
                    Expr::And(..) | Expr::Or(..) | Expr::Not(_) => write!(f, "({inner})"),
                    _ => write!(f, "{inner}"),
                }
            }
            Expr::Match(field, pat) => {
                write!(f, "{} ~ ", field.keyword())?;
                write_quoted(f, &pat.src)
            }
            Expr::Eq(field, v) => {
                write!(f, "{} == ", field.keyword())?;
                write_quoted(f, v)
            }
            Expr::LevelGte(l) => write!(f, "level >= {}", l.as_char()),
            Expr::TimeGe(t) => {
                f.write_str("time >= ")?;
                write_quoted(f, t.as_str())
            }
            Expr::TimeLe(t) => {
                f.write_str("time <= ")?;
                write_quoted(f, t.as_str())
            }
        }
    }
}
