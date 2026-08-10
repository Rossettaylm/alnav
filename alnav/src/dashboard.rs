//! One-shot startup Dashboard (unbound source).

use crate::recent::RecentFiles;
use crate::theme;

pub const QUICK_ACTION_COUNT: usize = 3;
pub const MAX_VISIBLE_RECENTS: usize = 9;
pub const FULL_PRESENTATION_ROWS: u16 = 24;

/// Responsive Dashboard presentation tier, selected from terminal display
/// dimensions before the renderer allocates individual rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardDensity {
    Full,
    Compact,
    Minimal,
}

impl DashboardDensity {
    pub fn for_size(content_width: u16, height: u16) -> Self {
        if height >= FULL_PRESENTATION_ROWS && content_width >= theme::DASHBOARD_LOGO_WIDTH {
            Self::Full
        } else if height >= 9 {
            Self::Compact
        } else {
            Self::Minimal
        }
    }

    pub fn fixed_rows(self, show_minimal_header: bool) -> u16 {
        match self {
            Self::Full => 15,
            Self::Compact => 8,
            Self::Minimal => 4 + u16::from(show_minimal_header),
        }
    }

    pub fn show_descriptions(self) -> bool {
        !matches!(self, Self::Minimal)
    }
}

/// Flat list item on the Dashboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardItem {
    Hdc,
    Adb,
    OpenFile,
    Recent { path: String, index: usize },
}

impl DashboardItem {
    pub fn glyph(&self) -> &'static str {
        match self {
            Self::Hdc => theme::GLYPH_SOURCE_HDC,
            Self::Adb => theme::GLYPH_SOURCE_ADB,
            Self::OpenFile => theme::GLYPH_SOURCE_OPEN_FILE,
            Self::Recent { .. } => theme::GLYPH_SOURCE_RECENT,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Hdc => "HDC".into(),
            Self::Adb => "ADB".into(),
            Self::OpenFile => "Open file".into(),
            Self::Recent { path, .. } => path.clone(),
        }
    }

    pub fn description(&self) -> Option<&'static str> {
        match self {
            Self::Hdc => Some("HarmonyOS hilog"),
            Self::Adb => Some("Android logcat"),
            Self::OpenFile => Some("Browse recent or local logs"),
            Self::Recent { .. } => None,
        }
    }

    pub fn hotkey(&self) -> Option<String> {
        match self {
            Self::Hdc => Some("h".into()),
            Self::Adb => Some("a".into()),
            Self::OpenFile => Some("o".into()),
            Self::Recent { index, .. } if *index < MAX_VISIBLE_RECENTS => {
                Some((index + 1).to_string())
            }
            Self::Recent { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DashboardState {
    pub cursor: usize,
    pub recent: RecentFiles,
}

impl DashboardState {
    pub fn new(recent: RecentFiles) -> Self {
        Self { cursor: 0, recent }
    }

    pub fn items(&self) -> Vec<DashboardItem> {
        let mut items = vec![
            DashboardItem::Hdc,
            DashboardItem::Adb,
            DashboardItem::OpenFile,
        ];
        for (i, path) in self.recent.paths.iter().enumerate() {
            items.push(DashboardItem::Recent {
                path: path.clone(),
                index: i,
            });
        }
        items
    }

    pub fn len(&self) -> usize {
        QUICK_ACTION_COUNT + self.recent.paths.len()
    }

    pub fn selected(&self) -> Option<DashboardItem> {
        self.items().into_iter().nth(self.cursor)
    }

    pub fn move_by(&mut self, delta: isize) {
        let n = self.len();
        if n == 0 {
            return;
        }
        let cur = self.cursor as isize;
        let next = (cur + delta).rem_euclid(n as isize) as usize;
        self.cursor = next;
    }

    pub fn jump_first(&mut self) {
        self.cursor = 0;
    }

    pub fn jump_last(&mut self) {
        let n = self.len();
        if n > 0 {
            self.cursor = n - 1;
        }
    }

    /// Jump to recent file index `n` (0-based) and return its path.
    pub fn select_recent(&mut self, n: usize) -> Option<String> {
        let path = self.recent.paths.get(n)?.clone();
        self.cursor = QUICK_ACTION_COUNT + n;
        Some(path)
    }

    /// Keep selection valid after the recent list shrinks (for example when an
    /// unreadable history entry is removed while the Dashboard stays open).
    pub fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.len().saturating_sub(1));
    }

    /// Visible newest-first recent slice. The window advances only as needed
    /// to keep the selected recent row on screen.
    pub fn visible_recent_range(&self, capacity: usize) -> std::ops::Range<usize> {
        let total = self.recent.paths.len();
        let capacity = capacity.min(MAX_VISIBLE_RECENTS).min(total);
        if capacity == 0 {
            return 0..0;
        }
        if total <= capacity {
            return 0..total;
        }

        let selected_recent = self
            .cursor
            .checked_sub(QUICK_ACTION_COUNT)
            .filter(|idx| *idx < total);
        let start = selected_recent
            .map(|idx| idx.saturating_sub(capacity - 1).min(total - capacity))
            .unwrap_or(0);
        start..start + capacity
    }
}

/// Outcome of a Dashboard key that should leave the Dashboard or open a panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardAction {
    Quit,
    BindHdc,
    BindAdb,
    OpenFilePicker,
    OpenRecent(String),
}

/// Handle a Dashboard key. Returns `Some` when an action should run.
pub fn handle_key(
    state: &mut DashboardState,
    code: crossterm::event::KeyCode,
) -> Option<DashboardAction> {
    use crossterm::event::KeyCode::*;
    match code {
        Char('q') => Some(DashboardAction::Quit),
        Char('h') => Some(DashboardAction::BindHdc),
        Char('a') => Some(DashboardAction::BindAdb),
        Char('o') => Some(DashboardAction::OpenFilePicker),
        Char(d) if d.is_ascii_digit() && d != '0' => {
            let n = (d as u8 - b'1') as usize;
            state.select_recent(n).map(DashboardAction::OpenRecent)
        }
        Char('j') | Down => {
            state.move_by(1);
            None
        }
        Char('k') | Up => {
            state.move_by(-1);
            None
        }
        Char('g') => {
            state.jump_first();
            None
        }
        Char('G') => {
            state.jump_last();
            None
        }
        Enter => match state.selected()? {
            DashboardItem::Hdc => Some(DashboardAction::BindHdc),
            DashboardItem::Adb => Some(DashboardAction::BindAdb),
            DashboardItem::OpenFile => Some(DashboardAction::OpenFilePicker),
            DashboardItem::Recent { path, .. } => Some(DashboardAction::OpenRecent(path)),
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_order_and_hotkeys() {
        let mut st = DashboardState::new(RecentFiles {
            paths: vec!["/a.log".into(), "/b.log".into()],
        });
        assert_eq!(st.len(), 5);
        assert!(matches!(
            handle_key(&mut st, crossterm::event::KeyCode::Char('h')),
            Some(DashboardAction::BindHdc)
        ));
        assert!(matches!(
            handle_key(&mut st, crossterm::event::KeyCode::Char('1')),
            Some(DashboardAction::OpenRecent(p)) if p == "/a.log"
        ));
        assert_eq!(st.cursor, 3); // Open file…=2, first recent=3
        st.cursor = 0;
        handle_key(&mut st, crossterm::event::KeyCode::Char('j'));
        assert_eq!(st.cursor, 1);
    }

    #[test]
    fn recent_window_keeps_selected_row_visible_and_caps_at_nine() {
        let mut st = DashboardState::new(RecentFiles {
            paths: (0..20).map(|i| format!("/{i}.log")).collect(),
        });
        assert_eq!(st.visible_recent_range(20), 0..9);

        st.cursor = QUICK_ACTION_COUNT + 19;
        assert_eq!(st.visible_recent_range(9), 11..20);
        assert!(st.visible_recent_range(4).contains(&19));
    }

    #[test]
    fn recent_window_handles_boundary_and_large_histories() {
        for (total, expected) in [(0, 0..0), (1, 0..1), (9, 0..9), (20, 0..9), (200, 0..9)] {
            let st = DashboardState::new(RecentFiles {
                paths: (0..total).map(|i| format!("/{i}.log")).collect(),
            });
            assert_eq!(st.visible_recent_range(MAX_VISIBLE_RECENTS), expected);
        }
    }

    #[test]
    fn presentation_density_reserves_full_frame_for_nine_recents() {
        assert_eq!(
            DashboardDensity::for_size(72, FULL_PRESENTATION_ROWS),
            DashboardDensity::Full
        );
        assert_eq!(
            DashboardDensity::for_size(72, FULL_PRESENTATION_ROWS - 1),
            DashboardDensity::Compact
        );
        assert_eq!(
            DashboardDensity::for_size(theme::DASHBOARD_LOGO_WIDTH - 1, FULL_PRESENTATION_ROWS),
            DashboardDensity::Compact
        );
        assert_eq!(
            DashboardDensity::for_size(theme::DASHBOARD_LOGO_WIDTH, FULL_PRESENTATION_ROWS),
            DashboardDensity::Full
        );
        assert_eq!(DashboardDensity::for_size(72, 8), DashboardDensity::Minimal);
    }

    #[test]
    fn navigation_and_activation_keys_keep_existing_contract() {
        use crossterm::event::KeyCode;

        let mut st = DashboardState::new(RecentFiles {
            paths: (0..12).map(|i| format!("/{i}.log")).collect(),
        });
        assert_eq!(
            handle_key(&mut st, KeyCode::Char('q')),
            Some(DashboardAction::Quit)
        );
        assert_eq!(
            handle_key(&mut st, KeyCode::Char('a')),
            Some(DashboardAction::BindAdb)
        );
        assert_eq!(
            handle_key(&mut st, KeyCode::Char('o')),
            Some(DashboardAction::OpenFilePicker)
        );

        handle_key(&mut st, KeyCode::Char('G'));
        assert_eq!(st.cursor, st.len() - 1);
        handle_key(&mut st, KeyCode::Up);
        assert_eq!(st.cursor, st.len() - 2);
        handle_key(&mut st, KeyCode::Char('g'));
        assert_eq!(st.cursor, 0);
        handle_key(&mut st, KeyCode::Down);
        assert_eq!(st.cursor, 1);
        assert_eq!(
            handle_key(&mut st, KeyCode::Enter),
            Some(DashboardAction::BindAdb)
        );
        assert_eq!(
            handle_key(&mut st, KeyCode::Char('9')),
            Some(DashboardAction::OpenRecent("/8.log".into()))
        );
        assert_eq!(st.cursor, QUICK_ACTION_COUNT + 8);
    }

    #[test]
    fn clamp_cursor_after_recent_removal_selects_last_valid_item() {
        let mut st = DashboardState::new(RecentFiles {
            paths: vec!["/a.log".into(), "/b.log".into()],
        });
        st.cursor = 4;
        st.recent.paths.pop();
        st.clamp_cursor();
        assert_eq!(st.cursor, 3);
        assert!(matches!(
            st.selected(),
            Some(DashboardItem::Recent { path, .. }) if path == "/a.log"
        ));
    }
}
