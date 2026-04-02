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

pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    TagMatch(Regex),
    MsgMatch(Regex),
    PkgMatch(Regex),
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
        Self { tokens, pos: 0, case_insensitive }
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
            Err(format!("expected {:?} at position {}, found {:?}", expected, pos, tok))
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
                            format!("unknown level '{}' at position {}, expected V/D/I/W/E/F", v, pos)
                        })?;
                        Ok(Expr::LevelGte(lvl))
                    } else {
                        Err(format!("expected level value at position {}, found {:?}", pos, tok))
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
                            FieldKind::Level => unreachable!(),
                        }
                    } else {
                        Err(format!("expected value after '~' at position {}, found {:?}", pos, tok))
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
            Expr::PkgMatch(re) => re.is_match(entry.tag) || re.is_match(entry.msg),
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
            Expr::TagMatch(re) | Expr::MsgMatch(re) | Expr::PkgMatch(re) => out.push(re),
            Expr::LevelGte(_) => {}
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::LogEntry;

    fn entry(level: Level, tag: &str, msg: &str) -> String {
        format!(
            "04-02 12:34:56.789  1234  5678 {} {:<8}: {}",
            level.as_char(),
            tag,
            msg
        )
    }

    fn parse_and_match(expr_str: &str, line: &str) -> bool {
        let expr = Expr::parse(expr_str, false).unwrap();
        let e = LogEntry::parse(line).unwrap();
        expr.matches(&e)
    }

    #[test]
    fn test_tag_match() {
        let line = entry(Level::D, "OkHttp", "request ok");
        assert!(parse_and_match("tag ~ OkHttp", &line));
        assert!(!parse_and_match("tag ~ Retrofit", &line));
    }

    #[test]
    fn test_msg_match() {
        let line = entry(Level::I, "App", "timeout error occurred");
        assert!(parse_and_match("msg ~ timeout", &line));
        assert!(!parse_and_match("msg ~ success", &line));
    }

    #[test]
    fn test_pkg_match() {
        let line = entry(Level::I, "com.app", "starting service");
        assert!(parse_and_match("pkg ~ com.app", &line));
        assert!(parse_and_match("pkg ~ service", &line));
        assert!(!parse_and_match("pkg ~ missing", &line));
    }

    #[test]
    fn test_level_gte() {
        let warn = entry(Level::W, "T", "m");
        let debug = entry(Level::D, "T", "m");
        assert!(parse_and_match("level >= W", &warn));
        assert!(!parse_and_match("level >= W", &debug));
    }

    #[test]
    fn test_and() {
        let line = entry(Level::W, "OkHttp", "timeout error");
        assert!(parse_and_match("tag ~ OkHttp and msg ~ timeout", &line));
        assert!(!parse_and_match("tag ~ OkHttp and msg ~ success", &line));
    }

    #[test]
    fn test_or() {
        let line = entry(Level::D, "Retrofit", "ok");
        assert!(parse_and_match("tag ~ OkHttp or tag ~ Retrofit", &line));
        assert!(!parse_and_match("tag ~ OkHttp or tag ~ Volley", &line));
    }

    #[test]
    fn test_not() {
        let line = entry(Level::D, "Debug", "trace");
        assert!(parse_and_match("not tag ~ OkHttp", &line));
        assert!(!parse_and_match("not tag ~ Debug", &line));
    }

    #[test]
    fn test_parens_and_precedence() {
        // (tag ~ OkHttp or tag ~ MyApp) and level >= W
        let ok_warn = entry(Level::W, "OkHttp", "m");
        let ok_debug = entry(Level::D, "OkHttp", "m");
        let other_warn = entry(Level::W, "Other", "m");

        let e = "(tag ~ OkHttp or tag ~ MyApp) and level >= W";
        assert!(parse_and_match(e, &ok_warn));
        assert!(!parse_and_match(e, &ok_debug));
        assert!(!parse_and_match(e, &other_warn));
    }

    #[test]
    fn test_case_insensitive() {
        let line = entry(Level::D, "OkHttp", "hello");
        let expr = Expr::parse("tag ~ okhttp", true).unwrap();
        let e = LogEntry::parse(&line).unwrap();
        assert!(expr.matches(&e));
    }

    #[test]
    fn test_quoted_value() {
        let line = entry(Level::I, "App", "hello world");
        assert!(parse_and_match("msg ~ \"hello world\"", &line));
    }

    #[test]
    fn test_collect_patterns() {
        let expr = Expr::parse("tag ~ OkHttp and msg ~ timeout", false).unwrap();
        let mut pats = Vec::new();
        expr.collect_patterns(&mut pats);
        assert_eq!(pats.len(), 2);
    }

    #[test]
    fn test_error_unterminated_string() {
        assert!(Expr::parse("msg ~ \"hello", false).is_err());
    }

    #[test]
    fn test_error_unexpected_eof() {
        assert!(Expr::parse("tag ~", false).is_err());
    }

    #[test]
    fn test_error_bad_level() {
        assert!(Expr::parse("level >= X", false).is_err());
    }

    #[test]
    fn test_error_missing_paren() {
        assert!(Expr::parse("(tag ~ OkHttp", false).is_err());
    }

    #[test]
    fn test_complex_expr() {
        let line = entry(Level::E, "OkHttp", "mobile_msf cmd:0x9293 done");
        let e = "msg ~ mobile_msf and msg ~ 0x9293 and level >= W";
        assert!(parse_and_match(e, &line));
    }
}
