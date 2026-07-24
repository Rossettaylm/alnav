//! Global session time-window editor (`ts` panel, file mode only).

use std::collections::{BTreeMap, VecDeque};

use crossterm::event::KeyCode;

use crate::filter_model::TimeBound;
use crate::model::EntryRow;
use crate::text_field::TextField;

/// Per-date min/max HMS seen in the current `rows` buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateStats {
    pub date: String,
    pub min_hms: String,
    pub max_hms: String,
}

/// Deduped, sorted date catalog extracted from buffered rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DateCatalog {
    pub dates: Vec<DateStats>,
}

impl DateCatalog {
    pub fn from_rows<'a, I>(rows: I) -> Self
    where
        I: IntoIterator<Item = &'a EntryRow>,
    {
        let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
        for row in rows {
            let entry = row.as_log_entry();
            let Some(full) = entry.time_full() else {
                continue;
            };
            let Some((date, hms)) = split_date_hms(full) else {
                continue;
            };
            map.entry(date.to_string())
                .and_modify(|(min, max)| {
                    if hms < min.as_str() {
                        *min = hms.to_string();
                    }
                    if hms > max.as_str() {
                        *max = hms.to_string();
                    }
                })
                .or_insert((hms.to_string(), hms.to_string()));
        }
        let dates = map
            .into_iter()
            .map(|(date, (min_hms, max_hms))| DateStats {
                date,
                min_hms,
                max_hms,
            })
            .collect();
        Self { dates }
    }

    pub fn is_empty(&self) -> bool {
        self.dates.is_empty()
    }

    pub fn stats_for(&self, date: &str) -> Option<&DateStats> {
        self.dates.iter().find(|d| d.date == date)
    }
}

/// Which of the four fields currently owns the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeField {
    SinceDate,
    SinceTime,
    UntilDate,
    UntilTime,
}

impl TimeField {
    pub fn next(self) -> Option<Self> {
        match self {
            Self::SinceDate => Some(Self::SinceTime),
            Self::SinceTime => Some(Self::UntilDate),
            Self::UntilDate => Some(Self::UntilTime),
            Self::UntilTime => None,
        }
    }

    pub fn is_date(self) -> bool {
        matches!(self, Self::SinceDate | Self::UntilDate)
    }

    pub fn is_since_side(self) -> bool {
        matches!(self, Self::SinceDate | Self::SinceTime)
    }
}

#[derive(Debug, Clone)]
struct SideDraft {
    selected_date: Option<String>,
    date_query: TextField,
    /// Highlight into the currently filtered candidate list.
    date_highlight: Option<usize>,
    time: TextField,
}

impl Default for SideDraft {
    fn default() -> Self {
        Self {
            selected_date: None,
            date_query: TextField::new(),
            date_highlight: None,
            time: TextField::new(),
        }
    }
}

/// Open `ts` editor state.
#[derive(Debug, Clone)]
pub struct TimePanel {
    pub catalog: DateCatalog,
    pub focus: TimeField,
    since: SideDraft,
    until: SideDraft,
}

/// Result of handling a key while the panel is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimePanelOutcome {
    /// Keep editing.
    Continue,
    /// Esc: close without applying.
    Cancel,
    /// Successful submit.
    Submit(TimeBound),
    /// Validation failed; flash this message and keep open.
    Flash(&'static str),
}

impl TimePanel {
    /// Build panel from buffered rows and optional current bound (best-effort prefill).
    /// Returns `None` when there are no date candidates.
    pub fn open(rows: &VecDeque<EntryRow>, bound: Option<&TimeBound>) -> Option<Self> {
        Self::open_from_iter(rows.iter(), bound)
    }

    /// Same as [`Self::open`] but accepts any `EntryRow` iterator (file sample).
    pub fn open_from_iter<'a, I>(rows: I, bound: Option<&TimeBound>) -> Option<Self>
    where
        I: IntoIterator<Item = &'a EntryRow>,
    {
        let catalog = DateCatalog::from_rows(rows);
        if catalog.is_empty() {
            return None;
        }
        let mut panel = Self {
            catalog,
            focus: TimeField::SinceDate,
            since: SideDraft::default(),
            until: SideDraft::default(),
        };
        if let Some(b) = bound {
            panel.prefill_side(true, b.since.as_deref());
            panel.prefill_side(false, b.until.as_deref());
        }
        panel.sync_date_highlight(true);
        panel.sync_date_highlight(false);
        Some(panel)
    }

    fn prefill_side(&mut self, since_side: bool, raw: Option<&str>) {
        let Some(raw) = raw else {
            return;
        };
        let side = if since_side {
            &mut self.since
        } else {
            &mut self.until
        };
        if let Some((date, hms)) = split_bound_prefills(raw) {
            if self.catalog.stats_for(date).is_some() {
                side.selected_date = Some(date.to_string());
                side.date_query.set_text(date);
            } else if !date.is_empty() {
                // Date not in buffer: still show query for visibility; leave unselected.
                side.date_query.set_text(date);
            }
            if !hms.is_empty() {
                side.time.set_text(hms);
            }
        } else if TimeBound::is_time_only(raw) {
            side.time.set_text(raw);
        }
    }

    fn side(&self, since_side: bool) -> &SideDraft {
        if since_side {
            &self.since
        } else {
            &self.until
        }
    }

    fn side_mut(&mut self, since_side: bool) -> &mut SideDraft {
        if since_side {
            &mut self.since
        } else {
            &mut self.until
        }
    }

    pub fn since_selected_date(&self) -> Option<&str> {
        self.since.selected_date.as_deref()
    }

    pub fn since_date_query(&self) -> &str {
        self.since.date_query.as_str()
    }

    pub fn until_date_query(&self) -> &str {
        self.until.date_query.as_str()
    }

    pub fn since_time(&self) -> &str {
        self.since.time.as_str()
    }

    pub fn until_time(&self) -> &str {
        self.until.time.as_str()
    }

    pub fn since_time_cursor(&self) -> usize {
        self.since.time.cursor()
    }

    pub fn until_time_cursor(&self) -> usize {
        self.until.time.cursor()
    }

    pub fn since_date_cursor(&self) -> usize {
        self.since.date_query.cursor()
    }

    pub fn until_date_cursor(&self) -> usize {
        self.until.date_query.cursor()
    }

    pub fn since_date_highlight(&self) -> Option<usize> {
        self.since.date_highlight
    }

    pub fn until_date_highlight(&self) -> Option<usize> {
        self.until.date_highlight
    }

    /// Filtered date candidates for the given side (substring match on query).
    pub fn filtered_dates(&self, since_side: bool) -> Vec<&DateStats> {
        let q = self.side(since_side).date_query.as_str();
        self.catalog
            .dates
            .iter()
            .filter(|d| q.is_empty() || d.date.contains(q))
            .collect()
    }

    fn sync_date_highlight(&mut self, since_side: bool) {
        let q = self.side(since_side).date_query.as_str().to_string();
        let selected = self.side(since_side).selected_date.clone();
        let cur_hl = self.side(since_side).date_highlight;
        let filtered: Vec<String> = self
            .catalog
            .dates
            .iter()
            .filter(|d| q.is_empty() || d.date.contains(&q))
            .map(|d| d.date.clone())
            .collect();
        let new_hl = if filtered.is_empty() {
            None
        } else if let Some(sel) = selected.as_ref() {
            Some(
                filtered
                    .iter()
                    .position(|d| d == sel)
                    .unwrap_or_else(|| cur_hl.unwrap_or(0).min(filtered.len() - 1)),
            )
        } else {
            Some(cur_hl.unwrap_or(0).min(filtered.len() - 1))
        };
        self.side_mut(since_side).date_highlight = new_hl;
    }

    pub fn handle_key(&mut self, code: KeyCode) -> TimePanelOutcome {
        match code {
            KeyCode::Esc => TimePanelOutcome::Cancel,
            KeyCode::Tab => self.advance(false),
            KeyCode::Enter => self.advance(true),
            KeyCode::Up if self.focus.is_date() => {
                self.move_date_hl(-1);
                TimePanelOutcome::Continue
            }
            KeyCode::Down if self.focus.is_date() => {
                self.move_date_hl(1);
                TimePanelOutcome::Continue
            }
            KeyCode::Left => {
                self.edit_field().move_left();
                TimePanelOutcome::Continue
            }
            KeyCode::Right => {
                self.edit_field().move_right();
                TimePanelOutcome::Continue
            }
            KeyCode::Home => {
                self.edit_field().home();
                TimePanelOutcome::Continue
            }
            KeyCode::End => {
                self.edit_field().end();
                TimePanelOutcome::Continue
            }
            KeyCode::Backspace => {
                self.edit_field().backspace();
                if self.focus.is_date() {
                    let since_side = self.focus.is_since_side();
                    self.side_mut(since_side).selected_date = None;
                    self.sync_date_highlight(since_side);
                    self.try_auto_accept_unique(since_side);
                }
                TimePanelOutcome::Continue
            }
            KeyCode::Char(c) => {
                if self.focus.is_date() {
                    self.edit_field().insert(c);
                    let since_side = self.focus.is_since_side();
                    self.side_mut(since_side).selected_date = None;
                    self.sync_date_highlight(since_side);
                    self.try_auto_accept_unique(since_side);
                } else {
                    // Time: allow digits and ':'
                    if c.is_ascii_digit() || c == ':' {
                        self.edit_field().insert(c);
                    }
                }
                TimePanelOutcome::Continue
            }
            _ => TimePanelOutcome::Continue,
        }
    }

    fn edit_field(&mut self) -> &mut TextField {
        match self.focus {
            TimeField::SinceDate => &mut self.since.date_query,
            TimeField::SinceTime => &mut self.since.time,
            TimeField::UntilDate => &mut self.until.date_query,
            TimeField::UntilTime => &mut self.until.time,
        }
    }

    fn move_date_hl(&mut self, delta: isize) {
        let since_side = self.focus.is_since_side();
        let len = self.filtered_dates(since_side).len();
        if len == 0 {
            self.side_mut(since_side).date_highlight = None;
            return;
        }
        let cur = self.side(since_side).date_highlight.unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, (len as isize) - 1) as usize;
        self.side_mut(since_side).date_highlight = Some(next);
    }

    /// If filtered list has exactly one date, select it and advance to time.
    fn try_auto_accept_unique(&mut self, since_side: bool) {
        let filtered: Vec<String> = self
            .filtered_dates(since_side)
            .iter()
            .map(|d| d.date.clone())
            .collect();
        if filtered.len() == 1 {
            let date = filtered.into_iter().next().unwrap();
            {
                let side = self.side_mut(since_side);
                side.selected_date = Some(date.clone());
                side.date_query.set_text(date);
                side.date_highlight = Some(0);
            }
            // Auto-advance only when currently on that side's date field.
            let on_date = matches!(
                (since_side, self.focus),
                (true, TimeField::SinceDate) | (false, TimeField::UntilDate)
            );
            if on_date {
                let _ = self.commit_date_and_maybe_advance(false);
            }
        }
    }

    fn advance(&mut self, is_enter: bool) -> TimePanelOutcome {
        if self.focus.is_date() {
            return self.commit_date_and_maybe_advance(is_enter);
        }
        // Time field: normalize + clamp, then advance or submit.
        let since_side = self.focus.is_since_side();
        self.commit_time_field(since_side);
        self.focus_next_or_submit()
    }

    fn commit_date_and_maybe_advance(&mut self, is_enter: bool) -> TimePanelOutcome {
        let since_side = self.focus.is_since_side();
        // Tab on an untouched date field skips without selecting so one-sided
        // windows can reach submit. Enter still selects the highlighted date.
        let untouched = self.side(since_side).selected_date.is_none()
            && self.side(since_side).date_query.is_empty()
            && self.side(since_side).time.is_empty();
        if untouched && !is_enter {
            return self.focus_next_or_submit();
        }

        let filtered: Vec<String> = self
            .filtered_dates(since_side)
            .iter()
            .map(|d| d.date.clone())
            .collect();
        let hl = self.side(since_side).date_highlight;
        let Some(i) = hl else {
            // No highlight → do not advance.
            return TimePanelOutcome::Continue;
        };
        if i >= filtered.len() {
            return TimePanelOutcome::Continue;
        }
        let date = filtered[i].clone();
        {
            let side = self.side_mut(since_side);
            side.selected_date = Some(date.clone());
            side.date_query.set_text(date);
        }
        self.focus_next_or_submit()
    }

    fn focus_next_or_submit(&mut self) -> TimePanelOutcome {
        match self.focus.next() {
            Some(next) => {
                self.focus = next;
                TimePanelOutcome::Continue
            }
            None => self.try_submit(),
        }
    }

    fn commit_time_field(&mut self, since_side: bool) {
        let draft = self.side(since_side).time.as_str().to_string();
        if draft.trim().is_empty() {
            return;
        }
        let mut hms = normalize_hms(&draft);
        if let Some(date) = self.side(since_side).selected_date.clone() {
            if let Some(stats) = self.catalog.stats_for(&date) {
                hms = clamp_hms(&hms, &stats.min_hms, &stats.max_hms);
            }
        }
        self.side_mut(since_side).time.set_text(&hms);

        // If both sides complete, clamp current side so since ≤ until.
        if let (Some(s), Some(u)) = (self.compose_side(true), self.compose_side(false)) {
            if s > u {
                if since_side {
                    // Clamp since down to until.
                    if let Some((_, uh)) = split_date_hms(&u) {
                        self.since.time.set_text(uh);
                    }
                } else if let Some((_, sh)) = split_date_hms(&s) {
                    self.until.time.set_text(sh);
                }
            }
        }
    }

    fn compose_side(&self, since_side: bool) -> Option<String> {
        let side = self.side(since_side);
        let date = side.selected_date.as_ref()?;
        let hms = side.time.as_str().trim();
        if hms.is_empty() {
            return None;
        }
        let hms = normalize_hms(hms);
        Some(format!("{date} {hms}"))
    }

    fn side_status(&self, since_side: bool) -> SideStatus {
        let side = self.side(since_side);
        let has_date = side.selected_date.is_some();
        let has_time = !side.time.as_str().trim().is_empty();
        match (has_date, has_time) {
            (false, false) => SideStatus::Empty,
            (true, true) => SideStatus::Complete,
            _ => SideStatus::Partial,
        }
    }

    fn try_submit(&mut self) -> TimePanelOutcome {
        // Finalize both time fields before validating.
        self.commit_time_field(true);
        self.commit_time_field(false);

        let s = self.side_status(true);
        let u = self.side_status(false);
        if matches!(s, SideStatus::Partial) || matches!(u, SideStatus::Partial) {
            return TimePanelOutcome::Flash("端内需日期+时间");
        }
        if matches!(s, SideStatus::Empty) && matches!(u, SideStatus::Empty) {
            return TimePanelOutcome::Flash("未设置时间窗");
        }

        let since = self.compose_side(true);
        let until = self.compose_side(false);
        // Re-clamp order after both composed.
        let (since, until) = match (since, until) {
            (Some(s), Some(u)) if s > u => {
                // Last field was until time; clamp until to since (current side).
                (Some(s.clone()), Some(s))
            }
            pair => pair,
        };
        TimePanelOutcome::Submit(TimeBound { since, until })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideStatus {
    Empty,
    Partial,
    Complete,
}

/// Split `time_full` (`MM-DD HH:MM:SS` or `YYYY-MM-DD HH:MM:SS`) into date + HMS.
pub fn split_date_hms(full: &str) -> Option<(&str, &str)> {
    let (date, rest) = full.split_once(' ')?;
    if rest.len() < 8 {
        return None;
    }
    Some((date, &rest[..8]))
}

/// Best-effort split of a stored bound string into date + HMS for prefill.
fn split_bound_prefills(raw: &str) -> Option<(&str, &str)> {
    let raw = raw.trim();
    if let Some((date, rest)) = raw.split_once(' ') {
        let hms = if rest.len() >= 8 { &rest[..8] } else { rest };
        return Some((date, hms));
    }
    // Bare date (no time)?
    if (raw.len() == 5 && raw.as_bytes().get(2) == Some(&b'-'))
        || (raw.len() == 10 && raw.as_bytes().get(4) == Some(&b'-'))
    {
        return Some((raw, ""));
    }
    None
}

/// Normalize free-typed time into `HH:MM:SS`, clamping components to valid ranges.
pub fn normalize_hms(raw: &str) -> String {
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    let mut chars: Vec<char> = digits.chars().collect();
    // Pad / truncate to 6 digits HHMMSS
    while chars.len() < 6 {
        chars.push('0');
    }
    chars.truncate(6);
    let parse2 = |a: char, b: char| -> u32 {
        let s: String = [a, b].iter().collect();
        s.parse().unwrap_or(0)
    };
    let mut hh = parse2(chars[0], chars[1]).min(23);
    let mut mm = parse2(chars[2], chars[3]).min(59);
    let mut ss = parse2(chars[4], chars[5]).min(59);
    // Also accept already-colon form by re-parsing segments when present.
    if raw.contains(':') {
        let parts: Vec<&str> = raw.split(':').collect();
        if !parts.is_empty() {
            hh = parts[0].parse::<u32>().unwrap_or(hh).min(23);
        }
        if parts.len() > 1 {
            mm = parts[1].parse::<u32>().unwrap_or(mm).min(59);
        }
        if parts.len() > 2 {
            // Take first 2 digits of seconds if longer
            let sec = parts[2].chars().take(2).collect::<String>();
            ss = sec.parse::<u32>().unwrap_or(ss).min(59);
        }
    }
    format!("{hh:02}:{mm:02}:{ss:02}")
}

/// Clamp HMS into `[min, max]` (lexicographic on `HH:MM:SS`).
pub fn clamp_hms(hms: &str, min: &str, max: &str) -> String {
    if hms < min {
        return min.to_string();
    }
    if hms > max {
        return max.to_string();
    }
    hms.to_string()
}

/// Convenience: extract catalog from a slice (tests / callers without VecDeque).
pub fn extract_date_catalog(rows: &[EntryRow]) -> DateCatalog {
    DateCatalog::from_rows(rows.iter())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntryRow;

    fn row_at(ts_prefix: &str) -> EntryRow {
        // threadtime: "MM-DD HH:MM:SS.mmm ..."
        EntryRow::from_line(&format!("{ts_prefix}.000  1234  5678 I Tag   : msg")).unwrap()
    }

    #[test]
    fn extract_dates_dedup_sorted_with_minmax() {
        let rows = vec![
            row_at("04-02 10:00:00"),
            row_at("04-02 12:30:00"),
            row_at("04-01 09:00:00"),
            row_at("04-02 11:00:00"),
        ];
        let cat = extract_date_catalog(&rows);
        assert_eq!(cat.dates.len(), 2);
        assert_eq!(cat.dates[0].date, "04-01");
        assert_eq!(cat.dates[0].min_hms, "09:00:00");
        assert_eq!(cat.dates[1].date, "04-02");
        assert_eq!(cat.dates[1].min_hms, "10:00:00");
        assert_eq!(cat.dates[1].max_hms, "12:30:00");
    }

    #[test]
    fn normalize_and_clamp_hms() {
        assert_eq!(normalize_hms("9:5:1"), "09:05:01");
        assert_eq!(normalize_hms("25:99:99"), "23:59:59");
        assert_eq!(clamp_hms("08:00:00", "10:00:00", "12:00:00"), "10:00:00");
        assert_eq!(clamp_hms("13:00:00", "10:00:00", "12:00:00"), "12:00:00");
        assert_eq!(clamp_hms("11:00:00", "10:00:00", "12:00:00"), "11:00:00");
    }

    #[test]
    fn submit_one_sided_since() {
        let rows: VecDeque<_> = vec![row_at("04-02 10:00:00"), row_at("04-02 12:00:00")].into();
        let mut panel = TimePanel::open(&rows, None).unwrap();
        // Select date
        panel.focus = TimeField::SinceDate;
        panel.since.date_highlight = Some(0);
        // Pre-set query so untouched-skip does not fire; select via highlight.
        panel.since.date_query.set_text("04");
        panel.sync_date_highlight(true);
        assert!(matches!(
            panel.handle_key(KeyCode::Enter),
            TimePanelOutcome::Continue
        ));
        assert_eq!(panel.focus, TimeField::SinceTime);
        panel.since.time.set_text("11:00:00");
        assert!(matches!(
            panel.handle_key(KeyCode::Tab),
            TimePanelOutcome::Continue
        ));
        assert_eq!(panel.focus, TimeField::UntilDate);
        // Skip empty until date + time → submit
        assert!(matches!(
            panel.handle_key(KeyCode::Tab),
            TimePanelOutcome::Continue
        ));
        assert_eq!(panel.focus, TimeField::UntilTime);
        let out = panel.handle_key(KeyCode::Enter);
        match out {
            TimePanelOutcome::Submit(b) => {
                assert_eq!(b.since.as_deref(), Some("04-02 11:00:00"));
                assert!(b.until.is_none());
            }
            other => panic!("expected Submit, got {other:?}"),
        }
    }

    #[test]
    fn submit_partial_side_flashes() {
        let rows: VecDeque<_> = vec![row_at("04-02 10:00:00")].into();
        let mut panel = TimePanel::open(&rows, None).unwrap();
        panel.since.selected_date = Some("04-02".into());
        // time empty → partial
        panel.focus = TimeField::UntilTime;
        let out = panel.handle_key(KeyCode::Enter);
        assert_eq!(out, TimePanelOutcome::Flash("端内需日期+时间"));
    }

    #[test]
    fn clamp_since_le_until_on_current_side() {
        let rows: VecDeque<_> = vec![row_at("04-02 10:00:00"), row_at("04-02 15:00:00")].into();
        let mut panel = TimePanel::open(&rows, None).unwrap();
        panel.since.selected_date = Some("04-02".into());
        panel.since.time.set_text("14:00:00");
        panel.until.selected_date = Some("04-02".into());
        panel.until.time.set_text("12:00:00");
        panel.focus = TimeField::UntilTime;
        panel.commit_time_field(false);
        // Current side is until → clamp until up to since
        assert_eq!(panel.until.time.as_str(), "14:00:00");
    }

    #[test]
    fn prefill_from_bound() {
        let rows: VecDeque<_> = vec![row_at("04-02 10:00:00"), row_at("04-02 12:00:00")].into();
        let bound = TimeBound {
            since: Some("04-02 10:30:00".into()),
            until: Some("04-02 11:00:00".into()),
        };
        let panel = TimePanel::open(&rows, Some(&bound)).unwrap();
        assert_eq!(panel.since_selected_date(), Some("04-02"));
        assert_eq!(panel.since_time(), "10:30:00");
        assert_eq!(panel.until_time(), "11:00:00");
    }
}
