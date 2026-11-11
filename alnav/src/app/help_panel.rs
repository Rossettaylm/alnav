//! Help panel navigation and `/` search.

use super::App;
use crate::help::{HelpPage, HelpSearch, HelpView};

impl App {
    /// Open the read-only Help panel. Does not change `following`.
    pub fn open_help(&mut self) {
        self.help_open = true;
        self.help_search = None;
        self.help_view = HelpView::Home {
            toc: crate::help::preselect_toc(self.focus),
            toc_off: 0,
        };
    }

    /// Close Help without resuming follow (same as Detail Esc).
    pub fn close_help(&mut self) {
        self.help_open = false;
        self.help_search = None;
        self.help_view = HelpView::default();
    }

    /// Sub-page → Home, restoring TOC highlight on the page just left.
    pub fn help_pop_to_home(&mut self) {
        if let HelpView::Page { id, .. } = self.help_view {
            self.help_view = HelpView::Home {
                toc: id.index(),
                toc_off: 0,
            };
        }
    }

    pub fn help_open_page(&mut self, page: HelpPage) {
        self.help_view = HelpView::Page {
            id: page,
            scroll: 0,
        };
    }

    pub fn help_search_prompting(&self) -> bool {
        self.help_search.as_ref().is_some_and(|s| s.prompt)
    }

    pub fn help_clear_search(&mut self) {
        self.help_search = None;
    }

    pub fn help_begin_search(&mut self) {
        match self.help_search.as_mut() {
            Some(search) => search.prompt = true,
            None => self.help_search = Some(HelpSearch::new()),
        }
    }

    pub fn help_rebuild_search(&mut self) {
        let query = self
            .help_search
            .as_ref()
            .map(|s| s.query.as_str().to_string())
            .unwrap_or_default();
        let hits = crate::help::search_help_hits(self, &query);
        let Some(search) = self.help_search.as_mut() else {
            return;
        };
        search.hits = hits;
        if search.hits.is_empty() {
            search.current = 0;
        } else {
            search.current = search.current.min(search.hits.len() - 1);
        }
    }

    /// After a query edit: rebuild hits, live-jump, or flash `NO MATCH`.
    pub fn help_on_query_edit(&mut self) {
        self.help_rebuild_search();
        let (empty_query, no_hits) = match self.help_search.as_ref() {
            Some(s) => (s.query.as_str().is_empty(), s.hits.is_empty()),
            None => return,
        };
        if empty_query {
            return;
        }
        if no_hits {
            self.set_flash("NO MATCH");
            return;
        }
        self.help_jump_current_hit();
    }

    pub fn help_commit_search(&mut self) {
        let empty = self
            .help_search
            .as_ref()
            .is_none_or(|s| s.query.as_str().is_empty());
        if empty {
            self.help_search = None;
            return;
        }
        self.help_rebuild_search();
        let no_hits = self.help_search.as_ref().is_none_or(|s| s.hits.is_empty());
        if no_hits {
            self.set_flash("NO MATCH");
            if let Some(s) = self.help_search.as_mut() {
                s.prompt = true;
            }
            return;
        }
        if let Some(s) = self.help_search.as_mut() {
            s.prompt = false;
        }
        self.help_jump_current_hit();
    }

    pub fn help_jump_current_hit(&mut self) {
        let Some(hit) = self
            .help_search
            .as_ref()
            .and_then(|s| s.hits.get(s.current))
            .cloned()
        else {
            return;
        };
        match hit.page {
            None => match crate::help::decode_home_hit_line(self, hit.line) {
                crate::help::HomeHitKind::Toc(i) => {
                    self.help_view = HelpView::Home { toc: i, toc_off: 0 };
                }
                crate::help::HomeHitKind::Active | crate::help::HomeHitKind::Chrome => {
                    let toc = match self.help_view {
                        HelpView::Home { toc, .. } => toc,
                        HelpView::Page { id, .. } => id.index(),
                    };
                    self.help_view = HelpView::Home { toc, toc_off: 0 };
                }
            },
            Some(id) => {
                let n = crate::help::page_doc_lines(self, id).len();
                let scroll = crate::help::page_max_scroll(n, self.help_body_view_h).min(hit.line);
                self.help_view = HelpView::Page { id, scroll };
            }
        }
    }

    pub fn help_search_step(&mut self, dir: isize) {
        let Some(search) = self.help_search.as_mut() else {
            return;
        };
        if search.prompt || search.hits.is_empty() {
            return;
        }
        let n = search.hits.len() as isize;
        let next = (search.current as isize + dir).rem_euclid(n) as usize;
        search.current = next;
        self.help_jump_current_hit();
    }

    pub fn help_search_step_prompt(&mut self, dir: isize) {
        let Some(search) = self.help_search.as_mut() else {
            return;
        };
        if !search.prompt || search.hits.is_empty() {
            return;
        }
        let n = search.hits.len() as isize;
        let next = (search.current as isize + dir).rem_euclid(n) as usize;
        search.current = next;
        self.help_jump_current_hit();
    }

    pub fn help_move_home_toc(&mut self, delta: isize) {
        let HelpView::Home { toc, toc_off } = &mut self.help_view else {
            return;
        };
        let max = (HelpPage::ALL.len() - 1) as isize;
        let next = (*toc as isize + delta).clamp(0, max) as u8;
        *toc = next;
        let sel = next as usize;
        if sel < *toc_off {
            *toc_off = sel;
        }
    }

    pub fn help_scroll_top(&mut self) {
        match &mut self.help_view {
            HelpView::Home { toc, toc_off } => {
                *toc = 0;
                *toc_off = 0;
            }
            HelpView::Page { scroll, .. } => *scroll = 0,
        }
    }

    pub fn help_scroll_bottom(&mut self) {
        match self.help_view {
            HelpView::Home { .. } => {
                self.help_view = HelpView::Home {
                    toc: (HelpPage::ALL.len() - 1) as u8,
                    toc_off: 0,
                };
            }
            HelpView::Page { id, .. } => {
                let n = crate::help::page_doc_lines(self, id).len();
                self.help_view = HelpView::Page {
                    id,
                    scroll: crate::help::page_max_scroll(n, self.help_body_view_h),
                };
            }
        }
    }

    pub fn scroll_help(&mut self, delta: isize) {
        if !self.help_open {
            return;
        }
        match self.help_view {
            HelpView::Home { .. } => self.help_move_home_toc(delta),
            HelpView::Page { id, scroll } => {
                let n = crate::help::page_doc_lines(self, id).len();
                let max = crate::help::page_max_scroll(n, self.help_body_view_h);
                let next = if delta < 0 {
                    scroll.saturating_sub((-delta) as usize)
                } else {
                    (scroll + delta as usize).min(max)
                };
                self.help_view = HelpView::Page { id, scroll: next };
            }
        }
    }
}
