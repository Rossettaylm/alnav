//! Help `/` search: `HelpSearch`/`HelpHit`, hit collection and line overlay.

use super::{home_doc_lines, page_doc_lines, HelpPage};
use crate::app::App;
use crate::text_field::TextField;
use crate::theme;
use ratatui::text::{Line, Span};

/// One ignore-case substring hit in the Help corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpHit {
    /// `None` = Home document; `Some` = that zone page.
    pub page: Option<HelpPage>,
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// Optional Help `/` search session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpSearch {
    pub query: TextField,
    pub prompt: bool,
    pub hits: Vec<HelpHit>,
    pub current: usize,
}

impl HelpSearch {
    pub fn new() -> Self {
        Self {
            query: TextField::new(),
            prompt: true,
            hits: Vec::new(),
            current: 0,
        }
    }

    pub fn has_hits(&self) -> bool {
        !self.hits.is_empty()
    }
}

fn substring_spans(haystack: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let h = haystack.to_ascii_lowercase();
    let n = needle.to_ascii_lowercase();
    h.match_indices(&n).map(|(i, m)| (i, i + m.len())).collect()
}

/// Ignore-case substring hits across Home + all seven pages.
pub fn search_help_hits(app: &App, query: &str) -> Vec<HelpHit> {
    let needle = query.trim();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (line_idx, row) in home_doc_lines(app).iter().enumerate() {
        for (start, end) in substring_spans(&row.text, needle) {
            hits.push(HelpHit {
                page: None,
                line: line_idx,
                start,
                end,
            });
        }
    }
    for page in HelpPage::ALL {
        for (line_idx, row) in page_doc_lines(app, page).iter().enumerate() {
            for (start, end) in substring_spans(&row.text, needle) {
                hits.push(HelpHit {
                    page: Some(page),
                    line: line_idx,
                    start,
                    end,
                });
            }
        }
    }
    hits
}

pub fn hits_on_line(
    search: Option<&HelpSearch>,
    page: Option<HelpPage>,
    line: usize,
) -> Vec<(usize, usize, bool)> {
    let Some(search) = search else {
        return Vec::new();
    };
    if search.hits.is_empty() {
        return Vec::new();
    }
    search
        .hits
        .iter()
        .enumerate()
        .filter(|(_, hit)| hit.page == page && hit.line == line)
        .map(|(i, hit)| (hit.start, hit.end, i == search.current))
        .collect()
}

/// Overlay substring hits onto an already-styled line.
pub fn overlay_search_hits(line: Line<'static>, hits: &[(usize, usize, bool)]) -> Line<'static> {
    if hits.is_empty() {
        return line;
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut byte = 0usize;
    for span in line.spans {
        let content = span.content.into_owned();
        let style = span.style;
        let end = byte + content.len();
        let mut cursor = 0usize;
        let mut marks: Vec<(usize, usize, bool)> = hits
            .iter()
            .filter_map(|&(s, e, cur)| {
                let a = s.max(byte);
                let b = e.min(end);
                if a < b {
                    Some((a - byte, b - byte, cur))
                } else {
                    None
                }
            })
            .collect();
        marks.sort_by_key(|m| (m.0, !m.2, m.1));
        for (s, e, current) in marks {
            let s = s.max(cursor);
            if s > cursor {
                out.push(Span::styled(content[cursor..s].to_string(), style));
            }
            if e > s {
                let hit_style = if current {
                    theme::help_search_current_style()
                } else {
                    theme::help_search_hit_style()
                };
                out.push(Span::styled(content[s..e].to_string(), hit_style));
                cursor = e;
            }
        }
        if cursor < content.len() {
            out.push(Span::styled(content[cursor..].to_string(), style));
        }
        byte = end;
    }
    Line::from(out)
}
