//! H1 draft preview: sample rows under temporary filter/search conditions
//! without mutating `App.visible` / `following`.

use crate::app::App;
use crate::filter_model::{compile_predicate, GroupList};
use crate::highlight_model::HighlightGroup;
use crate::input::{build_group_from_chips, Chip, ChipField, InputBox};
use crate::model::EntryRow;
use crate::store::RowStore;
use alnav::expr::Expr;

/// Hard cap on rows scanned while building a preview (anti-stall).
pub const PREVIEW_SCAN_CAP: usize = 4000;

/// One preview hit: owned row plus optional highlight pattern for paint.
#[derive(Debug, Clone)]
pub struct PreviewHit {
    pub row: EntryRow,
    /// When set, Preview paints this pattern with LogList highlight colors.
    pub pattern: Option<String>,
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

/// The App's predicate plus the draft: a draft include group joins the OR list,
/// draft excludes AND-NOT on top. `None` = nothing filters.
fn draft_predicate(app: &App, input: &InputBox) -> Option<Expr> {
    let chips = input_estimated_chips(input);
    let mut rules: GroupList = app.groups.clone();
    if input.exclude_mode {
        for chip in chips {
            let _ = rules.push_exclude(chip);
        }
    } else if let Ok(Some(group)) = build_group_from_chips(chips, true) {
        rules.groups.push(group);
    }
    compile_predicate(
        &rules,
        app.lock_pid.as_deref(),
        app.lock_tid.as_deref(),
        app.time_bound.as_ref(),
    )
}

fn row_passes_preview(app: &App, row: &EntryRow, predicate: Option<&Expr>) -> bool {
    // Draft or committed filters both count: unparsed never survives (CLI-aligned).
    let filtering = app.filter_active() || predicate.is_some();
    App::row_passes_filter_parts(
        row,
        predicate,
        app.view_focus,
        &app.highlight_groups,
        filtering,
    )
}

fn preview_source_len(app: &App) -> usize {
    match &app.index.store {
        RowStore::Stream(s) => s.rows.len(),
        RowStore::File(f) => f.line_count(),
    }
}

fn preview_row_at(app: &App, i: usize) -> Option<EntryRow> {
    match &app.index.store {
        RowStore::Stream(s) => s.rows.get(i).cloned(),
        RowStore::File(f) => f.row_at(i),
    }
}

/// Pick up to `limit` matching row indices near `anchor_row`, scanning at most
/// `scan_cap` rows total.
fn sample_near_anchor(
    app: &App,
    anchor_row: usize,
    scan_cap: usize,
    limit: usize,
    mut passes: impl FnMut(&EntryRow) -> bool,
) -> Vec<usize> {
    let n = preview_source_len(app);
    if n == 0 || limit == 0 {
        return Vec::new();
    }
    let anchor = anchor_row.min(n - 1);
    let mut hits: Vec<usize> = Vec::new();
    let mut scanned = 0usize;

    // Prefer scanning outward from the anchor so the preview stays local.
    let mut lo = anchor as isize;
    let mut hi = anchor as isize;
    while scanned < scan_cap && (lo >= 0 || hi < n as isize) && hits.len() < limit * 3 {
        if hi < n as isize {
            let i = hi as usize;
            if let Some(row) = preview_row_at(app, i) {
                if passes(&row) {
                    hits.push(i);
                }
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
            if let Some(row) = preview_row_at(app, i) {
                if passes(&row) {
                    hits.push(i);
                }
            }
            scanned += 1;
        }
    }

    hits.sort_unstable();
    hits.dedup();
    // Prefer hits closest to the anchor.
    hits.sort_by_key(|&i| i.abs_diff(anchor));
    hits.truncate(limit);
    hits.sort_unstable();
    hits
}

/// Filter/Exclude Input preview hits (final visible rows under draft).
/// `limit` is the number of content rows the Preview pane can show.
pub fn preview_filter_lines(app: &App, input: &InputBox, limit: usize) -> Vec<PreviewHit> {
    if limit == 0 {
        return Vec::new();
    }
    let predicate = draft_predicate(app, input);

    let n = preview_source_len(app);
    let anchor = if app.filter_active() {
        n.saturating_sub(1)
    } else {
        app.source_idx_for_visible(app.cursor)
            .unwrap_or(n.saturating_sub(1))
    };

    let indices = sample_near_anchor(app, anchor, PREVIEW_SCAN_CAP, limit, |row| {
        row_passes_preview(app, row, predicate.as_ref())
    });
    indices
        .into_iter()
        .filter_map(|i| {
            let row = preview_row_at(app, i)?;
            Some(PreviewHit { row, pattern: None })
        })
        .collect()
}

/// Search draft preview: matching rows with highlight pattern for paint.
/// Empty draft → empty vec (caller shows placeholder / folds).
pub fn preview_search_lines(app: &App, limit: usize) -> Result<Vec<PreviewHit>, ()> {
    preview_highlight_pattern_lines(app, app.highlight_box.draft.as_str(), limit)
}

/// Search preview for a caller-owned draft (for example the unified picker).
pub fn preview_highlight_pattern_lines(
    app: &App,
    pattern: &str,
    limit: usize,
) -> Result<Vec<PreviewHit>, ()> {
    let draft = pattern.trim();
    if draft.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    let group = HighlightGroup::from_pattern(draft).ok_or(())?;
    let n = preview_source_len(app);
    let anchor = if app.filter_active() {
        n.saturating_sub(1)
    } else {
        app.source_idx_for_visible(app.cursor)
            .unwrap_or(n.saturating_sub(1))
    };

    let indices = sample_near_anchor(app, anchor, PREVIEW_SCAN_CAP, limit, |row| {
        app.row_passes_filters(row) && group.matches_entry(row)
    });

    let mut out = Vec::with_capacity(indices.len());
    for i in indices {
        let Some(row) = preview_row_at(app, i) else {
            continue;
        };
        out.push(PreviewHit {
            row,
            pattern: Some(group.pattern.clone()),
        });
    }
    Ok(out)
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
        let lines = preview_filter_lines(&app, &input, 10);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].row.tag, "Keep");
        assert!(lines[0].pattern.is_none());
    }

    #[test]
    fn search_preview_empty_draft_is_empty() {
        let app = App::new(100);
        assert!(preview_search_lines(&app, 10).unwrap().is_empty());
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
        let lines = preview_search_lines(&app, 10).unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].row.msg.contains("world"));
        assert_eq!(lines[0].pattern.as_deref(), Some("wor"));
    }

    #[test]
    fn filter_preview_respects_limit() {
        let mut app = App::new(100);
        for i in 0..20 {
            drain_line(
                &mut app,
                &format!("04-02 10:00:{i:02}.000  1  1 I Keep    : msg{i}"),
            );
        }
        let mut input = InputBox::default();
        input.set_field(ChipField::Tag);
        for c in "Keep".chars() {
            input.push_char(c);
        }
        let lines = preview_filter_lines(&app, &input, 5);
        assert_eq!(lines.len(), 5);
    }
}
