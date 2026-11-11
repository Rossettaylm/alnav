//! Help zone pages and Home document: `HelpPage`/`HelpView`, page blurbs/entries, doc lines.

use super::{
    context_entries, context_kind, detail_line, key_of, key_style, label_style, push_agg,
    push_literal, push_short, push_single, HintEntry,
};
use crate::app::{App, Focus};
use crate::keymap::ActionId;
use crate::theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

/// Max Active key rows on Help Home (full [`context_entries`] stay elsewhere).
pub const HOME_ACTIVE_LIMIT: usize = 4;

/// Pinned Home/page footer (this panel's chrome; not a zone page).
pub const HOME_CHROME: &str = "/ search    n/N next    h back    Esc close";

/// One of the seven Help zone pages (`1`–`7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HelpPage {
    Filter,
    Exclude,
    Highlight,
    Log,
    Session,
    Picker,
    Overlays,
}

impl HelpPage {
    pub const ALL: [HelpPage; 7] = [
        Self::Filter,
        Self::Exclude,
        Self::Highlight,
        Self::Log,
        Self::Session,
        Self::Picker,
        Self::Overlays,
    ];

    pub fn index(self) -> u8 {
        match self {
            Self::Filter => 0,
            Self::Exclude => 1,
            Self::Highlight => 2,
            Self::Log => 3,
            Self::Session => 4,
            Self::Picker => 5,
            Self::Overlays => 6,
        }
    }

    pub fn from_index(i: u8) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }

    /// Digit `1`–`7` → page. Other chars → `None`.
    pub fn from_digit(c: char) -> Option<Self> {
        let n = c.to_digit(10)?;
        if !(1..=7).contains(&n) {
            return None;
        }
        Self::from_index((n - 1) as u8)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Filter => "Filter",
            Self::Exclude => "Exclude",
            Self::Highlight => "Highlight",
            Self::Log => "Log",
            Self::Session => "Session",
            Self::Picker => "Picker",
            Self::Overlays => "Overlays",
        }
    }
}

/// Help panel view: Home TOC or a zone page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpView {
    Home { toc: u8, toc_off: usize },
    Page { id: HelpPage, scroll: usize },
}

impl Default for HelpView {
    fn default() -> Self {
        Self::Home { toc: 3, toc_off: 0 }
    }
}

/// Styled Help line plus the plain haystack used for search/highlight.
#[derive(Debug, Clone)]
pub struct HelpLine {
    pub text: String,
    pub line: Line<'static>,
}

/// TOC index from the focus that opened Help.
pub fn preselect_toc(focus: Focus) -> u8 {
    match focus {
        Focus::ChipStrip => 0,
        Focus::ExcludeStrip => 1,
        Focus::HighlightStrip => 2,
        Focus::LogList => 3,
        Focus::Input => 0,
    }
}

/// Design-contract copy for a zone page (at most 5 lines).
pub fn page_blurb(page: HelpPage) -> &'static [&'static str] {
    match page {
        HelpPage::Filter => &[
            "Filter chips live in groups.",
            "Inside a group every chip is AND; across enabled groups the result is OR.",
            "If every group is disabled, that is the same as an empty list: every row stays visible.",
            "New filters go through the Picker, not the old Input strip.",
            "Startup CLI flags become group 0 and can be deleted or disabled like any other group.",
        ],
        HelpPage::Exclude => &[
            "Exclude groups apply as global AND NOT after Filter (then lock and the time window).",
            "They are not an inverted Filter page: a row must pass Filter and then match no enabled Exclude.",
            "Empty Exclude strip is folded.",
            "`C` plus a field letter pushes an exclude from the current row.",
        ],
        HelpPage::Highlight => &[
            "Highlight groups paint matching text; they do not hide rows.",
            "Enabled patterns are OR and walk the 8-slot color ramp in order.",
            "`/` on LogList finds or creates a highlight and jumps to the first hit; existing hits stay listed, then a create row for a non-exact query. `/` inside this Help panel is search and does not create a highlight.",
            "Command palette Add Highlight always opens New. Unified Enter still toggles enable without jumping.",
        ],
        HelpPage::Log => &[
            "The log list is the action origin: most cancels return here.",
            "Leaving the last visible row pauses following; landing on the last row resumes it; Esc still resumes explicitly.",
            "Yank, wrap, visual, and chip-from-row start on the current line.",
            "File mode can browse the whole file; live mode is a dropping ring.",
        ],
        HelpPage::Session => &[
            "Lock PID and lock TID are mutually exclusive and AND after chips.",
            "The global time window is file-only and orthogonal to Filter groups.",
            "`th` opens a time histogram over the current visible rows; Enter jumps, s sets the window, J/K/g/G move, Tab cycles 10s/1m/5m.",
            "Bookmarks are session-only snapshot pins: alive origins stay on the LogList when rules hide them; evicted live rows stay tray-only. `mm` opens the compare panel.",
            "Follow and device/file state live in the status bar, not in a chip group.",
        ],
        HelpPage::Picker => &[
            "Space is Leader; Space Space opens unified Manage.",
            "Bare `;` and backtick force New for Filter / Exclude. `/` finds or creates a Highlight.",
            "Unified Manage stays in Manage when nothing matches. Highlight finder appends a create row when the query is not an exact existing pattern, and auto-opens New on zero hits.",
            "`C-t` in unified Manage flips every target on or off: Tab-checked rows when any are checked, else the query-visible rows.",
            "The compare tray (`mm`) is not this unified picker.",
        ],
        HelpPage::Overlays => &[
            "Fields (`p`) and Pretty (`P`) are a top modal on the current row; Esc closes the overlay only and does not resume following.",
            "Pretty pretty-prints JSON in msg (then raw).",
            "The command palette (`C-p`) is not a Picker: an empty query shows no list.",
            "File-mode `th` opens a time histogram over visible rows (Enter jumps, s sets the window, Tab cycles 10s/1m/5m).",
            "This Help panel's own keys are on the Home footer, not in this list.",
        ],
    }
}

fn push_strip_group_ops(out: &mut Vec<HintEntry>, app: &App) {
    push_agg(
        out,
        app,
        &[ActionId::StripPrevGroup, ActionId::StripNextGroup],
        "group",
        "prev / next group on focused strip",
    );
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::StripPendingD),
        key_of(app, ActionId::StripDDelete),
    ) {
        out.push(HintEntry::new(
            format!("{a}{b}"),
            "delete",
            "delete selected strip group",
        ));
    }
    if let (Some(a), Some(b)) = (
        key_of(app, ActionId::StripPendingD),
        key_of(app, ActionId::StripDDisable),
    ) {
        out.push(HintEntry::new(
            format!("{a}{b}"),
            "disable",
            "toggle disable selected strip group",
        ));
    }
}

pub(super) fn page_entries(app: &App, page: HelpPage) -> Vec<HintEntry> {
    let live = app.export_source.is_live();
    match page {
        HelpPage::Filter => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::GlobalFilterNew,
                "filter new",
                "open filter picker in new mode",
            );
            push_strip_group_ops(&mut out, app);
            push_single(
                &mut out,
                app,
                ActionId::LogListChip,
                "chip",
                "filter/highlight from row (msg → tokens → Filter|Highlight)",
            );
            out
        }
        HelpPage::Exclude => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::GlobalExcludeNew,
                "exclude new",
                "open exclude picker in new mode",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListExcludeChip,
                "exclude",
                "exclude chip from current row field",
            );
            push_strip_group_ops(&mut out, app);
            out
        }
        HelpPage::Highlight => {
            let mut out = Vec::new();
            push_single(
                &mut out,
                app,
                ActionId::GlobalHighlightNew,
                "find highlight",
                "find or create a highlight and jump to the first hit",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListNextMatch, ActionId::LogListPrevMatch],
                "next hit",
                "next / previous highlight match",
            );
            push_strip_group_ops(&mut out, app);
            out
        }
        HelpPage::Log => {
            let mut out = Vec::new();
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListMoveDown, ActionId::LogListMoveUp],
                "move",
                "move cursor one line",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListJumpDown, ActionId::LogListJumpUp],
                "jump",
                "move 7 lines",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListJumpTop, ActionId::LogListJumpBottom],
                "top/bottom",
                "jump top or bottom (G resumes follow)",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListResumeFollow,
                "follow",
                "resume following and pin to bottom",
            );
            push_agg(
                &mut out,
                app,
                &[ActionId::LogListNextSevere, ActionId::LogListPrevSevere],
                "error",
                "next / previous severe line",
            );
            push_literal(
                &mut out,
                "1-5",
                "focus",
                "focus filter / exclude / highlight / log / input",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListWrapToggle,
                "wrap",
                "toggle multi-line wrap / single-line collapsed view",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListVisualLine,
                "visual",
                "visual line mode",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListYankMsgLine,
                "yank msg",
                "yank message of current line",
            );
            push_single(
                &mut out,
                app,
                ActionId::LogListChip,
                "chip",
                "filter/highlight from row (msg → tokens → Filter|Highlight)",
            );
            if let (Some(y), Some(c)) = (
                key_of(app, ActionId::LogListYank),
                key_of(app, ActionId::YankCli),
            ) {
                out.push(HintEntry::new(
                    format!("{y} {c}"),
                    "export",
                    "yank filters as alnav grep CLI (literal approx)",
                ));
            }
            if let Some(y) = key_of(app, ActionId::LogListYank) {
                out.push(HintEntry::new(
                    format!("{y} …"),
                    "yank field",
                    "yank tag/msg(token picker)/pkg/pid/tid/level/raw/line/time",
                ));
            }
            out
        }
        HelpPage::Session => {
            let mut session = Vec::new();
            push_single(
                &mut session,
                app,
                ActionId::LeaderPresetSave,
                "save preset",
                "save Filter/Exclude/Highlight preset",
            );
            push_single(
                &mut session,
                app,
                ActionId::LeaderPresetOpen,
                "open preset",
                "search and apply named preset",
            );
            push_agg(
                &mut session,
                app,
                &[ActionId::OpenFile, ActionId::OpenStream],
                "source",
                "open or switch file / stream source",
            );
            if let (Some(p), Some(fp), Some(ft), Some(fu)) = (
                key_of(app, ActionId::LogListLock),
                key_of(app, ActionId::LockPid),
                key_of(app, ActionId::LockTid),
                key_of(app, ActionId::LockClear),
            ) {
                session.push(HintEntry::new(
                    format!("{p} {fp}/{ft}/{fu}"),
                    "lock",
                    "lock pid / tid / clear",
                ));
            }
            if let (Some(p), Some(h), Some(e)) = (
                key_of(app, ActionId::LogListLock),
                key_of(app, ActionId::LockViewHighlight),
                key_of(app, ActionId::LockViewSevere),
            ) {
                session.push(HintEntry::new(
                    format!("{p} {h}/{e}"),
                    "view",
                    "highlight-only / severe-only (independent toggles; both = AND)",
                ));
            }
            if !live {
                if let (Some(t), Some(tt), Some(tu)) = (
                    key_of(app, ActionId::LogListTime),
                    key_of(app, ActionId::TimeSet),
                    key_of(app, ActionId::TimeClear),
                ) {
                    session.push(HintEntry::new(
                        format!("{t} {tt}/{tu}"),
                        "time",
                        "set / clear global time window (file only)",
                    ));
                }
                if let (Some(t), Some(th)) = (
                    key_of(app, ActionId::LogListTime),
                    key_of(app, ActionId::TimeHistogram),
                ) {
                    session.push(HintEntry::new(
                        format!("{t} {th}"),
                        "hist",
                        "time histogram: jump or set window (file only)",
                    ));
                }
            }
            if let (Some(m), Some(a), Some(d)) = (
                key_of(app, ActionId::LogListBookmark),
                key_of(app, ActionId::BookmarkAdd),
                key_of(app, ActionId::BookmarkRemove),
            ) {
                session.push(HintEntry::new(
                    format!("{m}{a}/{m}{d}"),
                    "bookmark",
                    "add / remove bookmark on current row",
                ));
            }
            if let (Some(m), Some(b)) = (
                key_of(app, ActionId::LogListBookmark),
                key_of(app, ActionId::BookmarkManage),
            ) {
                session.push(HintEntry::new(
                    format!("{m}{b}"),
                    "bookmarks",
                    "open bookmark compare panel",
                ));
            }
            if live {
                push_single(
                    &mut session,
                    app,
                    ActionId::LogListClearLive,
                    "clear",
                    "clear buffered live logs",
                );
            }
            session
        }
        HelpPage::Picker => {
            let mut leader = Vec::new();
            if let (Some(a), Some(b)) = (
                key_of(app, ActionId::LogListLeader),
                key_of(app, ActionId::LeaderManage),
            ) {
                leader.push(HintEntry::new(
                    format!("{a} {b}"),
                    "manage",
                    "unified manage picker",
                ));
            }
            push_single(
                &mut leader,
                app,
                ActionId::GlobalFilterNew,
                "filter new",
                "open filter picker in new mode",
            );
            push_single(
                &mut leader,
                app,
                ActionId::GlobalHighlightNew,
                "find highlight",
                "find or create a highlight and jump to the first hit",
            );
            push_single(
                &mut leader,
                app,
                ActionId::GlobalExcludeNew,
                "exclude new",
                "open exclude picker in new mode",
            );
            push_literal(
                &mut leader,
                "type",
                "filter",
                "type to fuzzy-filter; Enter toggle; ^X edit; Del delete",
            );
            push_single(
                &mut leader,
                app,
                ActionId::PickerEdit,
                "edit",
                "edit selected",
            );
            push_agg(
                &mut leader,
                app,
                &[ActionId::PickerDelete, ActionId::PickerDeleteAlt],
                "delete",
                "delete with confirm",
            );
            leader
        }
        HelpPage::Overlays => {
            let mut overlays = Vec::new();
            push_agg(
                &mut overlays,
                app,
                &[ActionId::LogListDetailFields, ActionId::LogListDetailPretty],
                "detail",
                "toggle fields / pretty overlay",
            );
            push_single(
                &mut overlays,
                app,
                ActionId::GlobalCommandPalette,
                "palette",
                "open command palette",
            );
            if let (Some(a), Some(b)) = (
                key_of(app, ActionId::LogListLeader),
                key_of(app, ActionId::LeaderSummary),
            ) {
                overlays.push(HintEntry::new(
                    format!("{a} {b}"),
                    "stats",
                    "open summary panel (level / tags / errors)",
                ));
            }
            if !live {
                push_agg(
                    &mut overlays,
                    app,
                    &[ActionId::TimePanelNext, ActionId::TimePanelSubmit],
                    "next",
                    "time panel next field",
                );
                push_agg(
                    &mut overlays,
                    app,
                    &[ActionId::TimePanelDateUp, ActionId::TimePanelDateDown],
                    "date",
                    "time panel date",
                );
                push_short(&mut overlays, app, ActionId::TimePanelCancel, "cancel");
            }
            overlays
        }
    }
}

fn styled_help_line(line: Line<'static>) -> HelpLine {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    HelpLine { text, line }
}

fn plain_help_line(text: impl Into<String>, style: Style) -> HelpLine {
    let text = text.into();
    HelpLine {
        line: Line::from(Span::styled(text.clone(), style)),
        text,
    }
}

/// Home Active block: title + at most [`HOME_ACTIVE_LIMIT`] context entries.
pub fn home_active_lines(app: &App) -> Vec<HelpLine> {
    let kind = context_kind(app);
    let title = Line::from(vec![
        Span::styled(
            "Active  ",
            Style::default()
                .fg(theme::accent())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            kind.title().to_string(),
            Style::default().fg(theme::accent()),
        ),
    ]);
    let mut lines = vec![styled_help_line(title)];
    for entry in context_entries(app).into_iter().take(HOME_ACTIVE_LIMIT) {
        lines.push(styled_help_line(detail_line(&entry)));
    }
    lines
}

/// Numbered TOC rows (`1`–`7`). `selected` is highlighted when `Some`.
pub fn home_toc_lines(selected: Option<u8>) -> Vec<HelpLine> {
    HelpPage::ALL
        .iter()
        .enumerate()
        .map(|(i, page)| {
            let text = format!("{}  {}", i + 1, page.title());
            let selected = selected == Some(i as u8);
            let style = if selected {
                theme::candidate_selected_style()
            } else {
                theme::candidate_unselected_style()
            };
            HelpLine {
                line: Line::from(Span::styled(text.clone(), style)),
                text,
            }
        })
        .collect()
}

pub fn chrome_help_line() -> HelpLine {
    let mut spans = Vec::new();
    for (i, (key, label)) in [
        ("/", "search"),
        ("n/N", "next"),
        ("h", "back"),
        ("Esc", "close"),
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            spans.push(Span::raw("    "));
        }
        spans.push(Span::styled(key.to_string(), key_style()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(label.to_string(), label_style()));
    }
    HelpLine {
        text: HOME_CHROME.to_string(),
        line: Line::from(spans),
    }
}

/// Home document for search: Active + TOC (unselected) + chrome.
pub fn home_doc_lines(app: &App) -> Vec<HelpLine> {
    let mut lines = home_active_lines(app);
    lines.extend(home_toc_lines(None));
    lines.push(chrome_help_line());
    lines
}

/// Zone page document: title, blurb, key table.
pub fn page_doc_lines(app: &App, page: HelpPage) -> Vec<HelpLine> {
    let mut lines = Vec::new();
    lines.push(plain_help_line(
        page.title().to_string(),
        Style::default()
            .fg(theme::accent())
            .add_modifier(Modifier::BOLD),
    ));
    for blurb in page_blurb(page) {
        lines.push(plain_help_line((*blurb).to_string(), label_style()));
    }
    lines.push(HelpLine {
        text: String::new(),
        line: Line::from(""),
    });
    for entry in page_entries(app, page) {
        lines.push(styled_help_line(detail_line(&entry)));
    }
    lines
}

/// Max page `scroll` so the last line sits at the bottom of the viewport
/// instead of the top (which would leave a blank remainder).
pub fn page_max_scroll(line_count: usize, viewport_rows: usize) -> usize {
    line_count.saturating_sub(viewport_rows.max(1))
}

/// Lines for the current Help view (unwindowed; tests + modal height).
pub fn help_body_lines(app: &App) -> Vec<Line<'static>> {
    match app.help_view {
        HelpView::Home { toc, .. } => {
            let mut lines: Vec<Line<'static>> =
                home_active_lines(app).into_iter().map(|l| l.line).collect();
            lines.extend(home_toc_lines(Some(toc)).into_iter().map(|l| l.line));
            lines.push(chrome_help_line().line);
            lines
        }
        HelpView::Page { id, .. } => {
            let mut lines: Vec<Line<'static>> = page_doc_lines(app, id)
                .into_iter()
                .map(|l| l.line)
                .collect();
            lines.push(chrome_help_line().line);
            lines
        }
    }
}

pub fn home_active_len(app: &App) -> usize {
    home_active_lines(app).len()
}

pub fn decode_home_hit_line(app: &App, line: usize) -> HomeHitKind {
    let active = home_active_len(app);
    if line < active {
        HomeHitKind::Active
    } else if line < active + HelpPage::ALL.len() {
        HomeHitKind::Toc((line - active) as u8)
    } else {
        HomeHitKind::Chrome
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeHitKind {
    Active,
    Toc(u8),
    Chrome,
}
