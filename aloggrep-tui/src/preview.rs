//! H1 draft preview: sample rows under temporary filter/search conditions
//! without mutating `App.visible` / `following`.

use crate::app::App;
use crate::filter_model::{ExcludeEntry, Group, GroupList};
use crate::highlight_model::HighlightGroup;
use crate::input::{build_group_from_chips, Chip, ChipField, InputBox};
use crate::model::EntryRow;

/// Max rows shown in the Preview window.
pub const PREVIEW_LIMIT: usize = 10;
/// Hard cap on rows scanned while building a preview (anti-stall).
pub const PREVIEW_SCAN_CAP: usize = 4000;

/// One preview line for Filter (plain) or Search (optional match byte range in msg/tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewLine {
    pub text: String,
    /// Byte range into `text` for faint search highlight, if any.
    pub highlight: Option<(usize, usize)>,
}

/// Estimate chips that would be committed from the Input modal (pills + draft).
pub fn input_estimated_chips(input: &InputBox) -> Vec<Chip> {
    let mut chips = input.chips.clone();
    if !input.draft.is_empty() {
        let field = input.draft_field.unwrap_or(ChipField::Msg);
        chips.push(Chip {
            field,
            value: input.draft.to_string(),
        });
    }
    chips
}

fn lock_ok(app: &App, row: &EntryRow) -> bool {
    if let Some(pid) = &app.lock_pid {
        return row.pid == *pid;
    }
    if let Some(tid) = &app.lock_tid {
        return row.tid == *tid;
    }
    true
}

fn excludes_allow(excludes: &[ExcludeEntry], row: &EntryRow) -> bool {
    let le = row.as_log_entry();
    for e in excludes {
        if e.enabled && e.expr.matches(&le) {
            return false;
        }
    }
    true
}

fn include_matches(groups: &[Group], temp: Option<&Group>, row: &EntryRow) -> bool {
    let mut any = false;
    for g in groups {
        if !g.enabled {
            continue;
        }
        any = true;
        if g.matches(row) {
            return true;
        }
    }
    if let Some(t) = temp {
        any = true;
        if t.matches(row) {
            return true;
        }
    }
    !any
}

fn row_passes_preview(
    app: &App,
    row: &EntryRow,
    temp_include: Option<&Group>,
    extra_excludes: &[ExcludeEntry],
) -> bool {
    if !include_matches(&app.groups.groups, temp_include, row) {
        return false;
    }
    if !excludes_allow(&app.groups.excludes, row) {
        return false;
    }
    if !excludes_allow(extra_excludes, row) {
        return false;
    }
    lock_ok(app, row)
}

fn format_preview_line(row: &EntryRow) -> String {
    let tag = if row.tag.is_empty() { "-" } else { &row.tag };
    let msg = if row.msg.is_empty() {
        &row.raw
    } else {
        &row.msg
    };
    let line = format!("{} {} {}", row.level.as_char(), tag, msg);
    // Keep preview rows short for the narrow modal width.
    if line.chars().count() > 72 {
        let truncated: String = line.chars().take(69).collect();
        format!("{truncated}…")
    } else {
        line
    }
}

/// Pick up to `limit` matching row indices near `anchor_row`, scanning at most
/// `scan_cap` rows total.
fn sample_near_anchor(
    rows: &std::collections::VecDeque<EntryRow>,
    anchor_row: usize,
    scan_cap: usize,
    limit: usize,
    mut passes: impl FnMut(&EntryRow) -> bool,
) -> Vec<usize> {
    if rows.is_empty() || limit == 0 {
        return Vec::new();
    }
    let n = rows.len();
    let anchor = anchor_row.min(n - 1);
    let mut hits: Vec<usize> = Vec::new();
    let mut scanned = 0usize;

    // Prefer scanning outward from the anchor so the preview stays local.
    let mut lo = anchor as isize;
    let mut hi = anchor as isize;
    while scanned < scan_cap && (lo >= 0 || hi < n as isize) && hits.len() < limit * 3 {
        if hi < n as isize {
            let i = hi as usize;
            if passes(&rows[i]) {
                hits.push(i);
            }
            scanned += 1;
            hi += 1;
        }
        if scanned >= scan_cap || hits.len() >= limit * 3 {
            break;
        }
        lo -= 1;
        if lo >= 0 {
            let i = lo as usize;
            if passes(&rows[i]) {
                hits.push(i);
            }
            scanned += 1;
        }
    }
    hits.sort_unstable();
    hits.dedup();
    if hits.is_empty() {
        return Vec::new();
    }
    // Window of `limit` centered on the hit closest to anchor.
    let best = hits
        .iter()
        .enumerate()
        .min_by_key(|(_, &i)| i.abs_diff(anchor))
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let start = best.saturating_sub(limit / 2);
    let end = (start + limit).min(hits.len());
    let start = end.saturating_sub(limit);
    hits[start..end].to_vec()
}

fn compile_temp_include(chips: Vec<Chip>) -> Result<Option<Group>, String> {
    build_group_from_chips(chips, true)
}

fn compile_temp_excludes(chips: Vec<Chip>) -> Result<Vec<ExcludeEntry>, String> {
    let mut out = Vec::new();
    let mut tmp = GroupList::default();
    for chip in chips {
        match tmp.push_exclude(chip) {
            Ok(true) => {
                if let Some(e) = tmp.excludes.pop() {
                    out.push(e);
                }
            }
            Ok(false) => {} // duplicate of earlier temp chip
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// Filter/Exclude Input preview lines (final visible rows under draft).
pub fn preview_filter_lines(app: &App, input: &InputBox) -> Vec<PreviewLine> {
    let chips = input_estimated_chips(input);
    let (temp_include, extra_excludes) = if input.exclude_mode {
        (None, compile_temp_excludes(chips).unwrap_or_default())
    } else if chips.is_empty() {
        (None, Vec::new())
    } else {
        match compile_temp_include(chips) {
            Ok(g) => (g, Vec::new()),
            Err(_) => (None, Vec::new()),
        }
    };

    let anchor = if app.filter_active() {
        app.rows.len().saturating_sub(1)
    } else {
        app.visible
            .get(app.cursor)
            .copied()
            .unwrap_or(app.rows.len().saturating_sub(1))
    };

    let indices = sample_near_anchor(&app.rows, anchor, PREVIEW_SCAN_CAP, PREVIEW_LIMIT, |row| {
        row_passes_preview(app, row, temp_include.as_ref(), &extra_excludes)
    });
    indices
        .into_iter()
        .map(|i| PreviewLine {
            text: format_preview_line(&app.rows[i]),
            highlight: None,
        })
        .collect()
}

/// Search draft preview: matching rows with faint highlight ranges.
/// Empty draft → empty vec (caller shows placeholder / folds).
pub fn preview_search_lines(app: &App) -> Result<Vec<PreviewLine>, ()> {
    preview_highlight_pattern_lines(app, app.highlight_box.draft.as_str())
}

/// Search preview for a caller-owned draft (for example the unified picker).
pub fn preview_highlight_pattern_lines(app: &App, pattern: &str) -> Result<Vec<PreviewLine>, ()> {
    let draft = pattern.trim();
    if draft.is_empty() {
        return Ok(Vec::new());
    }
    let group = HighlightGroup::from_pattern(draft).ok_or(())?;
    let anchor = if app.filter_active() {
        app.rows.len().saturating_sub(1)
    } else {
        app.visible
            .get(app.cursor)
            .copied()
            .unwrap_or(app.rows.len().saturating_sub(1))
    };

    let indices = sample_near_anchor(&app.rows, anchor, PREVIEW_SCAN_CAP, PREVIEW_LIMIT, |row| {
        app.row_passes_filters(row) && group.matches_row(&row.tag, &row.msg)
    });

    let mut out = Vec::with_capacity(indices.len());
    for i in indices {
        let row = &app.rows[i];
        let text = format_preview_line(row);
        let highlight = find_highlight_in_preview(&text, &group.re);
        out.push(PreviewLine { text, highlight });
    }
    Ok(out)
}

fn find_highlight_in_preview(text: &str, re: &regex::Regex) -> Option<(usize, usize)> {
    re.find(text).map(|m| (m.start(), m.end()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryRow;
    use std::sync::mpsc;

    fn drain_line(app: &mut App, line: &str) {
        let (tx, rx) = mpsc::channel();
        tx.send(EntryRow::from_line(line).unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
    }

    #[test]
    fn filter_preview_respects_draft_chip() {
        let mut app = App::new(100);
        drain_line(&mut app, "04-02 10:00:00.000  1  1 I Keep    : a");
        drain_line(&mut app, "04-02 10:00:01.000  1  1 I Drop    : b");
        let mut input = InputBox::default();
        input.set_field(ChipField::Tag);
        for c in "Keep".chars() {
            input.push_char(c);
        }
        let lines = preview_filter_lines(&app, &input);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.contains("Keep"));
    }

    #[test]
    fn search_preview_empty_draft_is_empty() {
        let app = App::new(100);
        assert!(preview_search_lines(&app).unwrap().is_empty());
    }

    #[test]
    fn search_preview_finds_draft_hits() {
        let mut app = App::new(100);
        drain_line(&mut app, "04-02 10:00:00.000  1  1 I Tag     : hello");
        drain_line(&mut app, "04-02 10:00:01.000  1  1 I Tag     : world");
        app.highlight_box.begin_editing();
        for c in "wor".chars() {
            app.highlight_box.push_char(c);
        }
        let lines = preview_search_lines(&app).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].text.contains("world"));
        assert!(lines[0].highlight.is_some());
    }
}
