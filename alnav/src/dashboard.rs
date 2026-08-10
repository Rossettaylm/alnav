//! One-shot startup Dashboard (unbound source).

use crate::recent::RecentFiles;
use crate::theme;

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
            Self::Hdc => "HDC  (hilog)".into(),
            Self::Adb => "ADB  (logcat)".into(),
            Self::OpenFile => "Open file…".into(),
            Self::Recent { path, .. } => path.clone(),
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
        3 + self.recent.paths.len()
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
        self.cursor = 3 + n;
        Some(path)
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
}
