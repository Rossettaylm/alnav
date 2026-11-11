//! Normal-mode key handling: focus/strip/loglist keys, leader and strip `d` chords, chip helpers.

use crate::app::{App, Pending};
use crate::handlers::km_code;
use crate::keymap::ActionId;
use crate::{app, input};
use crossterm::event::KeyCode;

fn handle_strip_d_chord(app: &mut App, kind: app::StripKind, code: KeyCode) -> bool {
    use app::StripKind;
    if !matches!(
        (kind, app.focus),
        (StripKind::Filter, app::Focus::ChipStrip)
            | (StripKind::Exclude, app::Focus::ExcludeStrip)
            | (StripKind::Highlight, app::Focus::HighlightStrip)
    ) {
        return false;
    }
    if app.pending == Pending::StripD {
        if km_code(app, ActionId::StripDDelete, code) {
            crate::action::dispatch(app, ActionId::StripDDelete);
            return true;
        }
        if km_code(app, ActionId::StripDDisable, code) {
            crate::action::dispatch(app, ActionId::StripDDisable);
            return true;
        }
        app.pending.cancel(Pending::StripD);
        return false;
    }
    if km_code(app, ActionId::StripPendingD, code) {
        crate::action::dispatch(app, ActionId::StripPendingD);
        return true;
    }
    false
}

fn handle_leader_key(app: &mut App, code: KeyCode) -> bool {
    if app.pending == Pending::Leader {
        app.pending.cancel(Pending::Leader);
        if km_code(app, ActionId::LeaderManage, code) {
            crate::action::dispatch(app, ActionId::LeaderManage);
        } else if km_code(app, ActionId::LeaderSummary, code) {
            crate::action::dispatch(app, ActionId::LeaderSummary);
        } else if km_code(app, ActionId::LeaderCancel, code) {
            crate::action::dispatch(app, ActionId::LeaderCancel);
        } else {
            app.set_flash("UNKNOWN LEADER");
        }
        return true;
    }

    if km_code(app, ActionId::LogListLeader, code) {
        crate::action::dispatch(app, ActionId::LogListLeader);
        return true;
    }
    false
}

pub(super) fn handle_normal_key(app: &mut App, _input: &mut input::InputBox, code: KeyCode) {
    use app::{Focus, StripKind};

    // Visual-line: handle selection motions / yank before anything else.
    if app.visual_anchor.is_some() && app.focus == Focus::LogList {
        if let Some(id) = bound_action(app, VISUAL_KEYS, code) {
            crate::action::dispatch(app, id);
            return;
        }
        app.clear_visual();
        // fall through so the key still does its Normal action
    }

    if handle_leader_key(app, code) {
        return;
    }

    match app.pending {
        Pending::Yank | Pending::Chip | Pending::Exclude => {
            handle_field_operator(app, code);
            return;
        }
        Pending::Lock | Pending::Time | Pending::Mark => {
            let ids: &[ActionId] = match app.pending {
                Pending::Lock => LOCK_CHORD,
                Pending::Time => TIME_CHORD,
                _ => MARK_CHORD,
            };
            app.pending = Pending::None;
            if app.focus == Focus::LogList {
                match bound_action(app, ids, code) {
                    Some(id) => crate::action::dispatch(app, id),
                    None => app.set_flash("UNKNOWN"),
                }
            }
            return;
        }
        Pending::None | Pending::Leader | Pending::StripD => {}
    }

    // Filter / Exclude / Highlight strip: dd delete, di toggle disable.
    if handle_strip_d_chord(app, StripKind::Filter, code) {
        return;
    }
    if handle_strip_d_chord(app, StripKind::Exclude, code) {
        return;
    }
    if handle_strip_d_chord(app, StripKind::Highlight, code) {
        return;
    }
    if !km_code(app, ActionId::StripPendingD, code) {
        app.pending.cancel(Pending::StripD);
    }

    if km_code(app, ActionId::GlobalQuit, code) {
        crate::action::dispatch(app, ActionId::GlobalQuit);
        return;
    }
    if km_code(app, ActionId::GlobalFocusNext, code) {
        crate::action::dispatch(app, ActionId::GlobalFocusNext);
        return;
    }
    if km_code(app, ActionId::GlobalFocusPrev, code) {
        crate::action::dispatch(app, ActionId::GlobalFocusPrev);
        return;
    }
    if km_code(app, ActionId::GlobalFocusFilter, code) {
        crate::action::dispatch(app, ActionId::GlobalFocusFilter);
        return;
    }
    if km_code(app, ActionId::GlobalFocusExclude, code) {
        crate::action::dispatch(app, ActionId::GlobalFocusExclude);
        return;
    }
    if km_code(app, ActionId::GlobalFocusHighlight, code) {
        crate::action::dispatch(app, ActionId::GlobalFocusHighlight);
        return;
    }
    if km_code(app, ActionId::GlobalFocusLog, code) {
        crate::action::dispatch(app, ActionId::GlobalFocusLog);
        return;
    }
    if km_code(app, ActionId::GlobalFocusInput, code) {
        crate::action::dispatch(app, ActionId::GlobalFocusInput);
        return;
    }

    // Esc / resume: detail close, follow, or return to log list.
    if km_code(app, ActionId::LogListResumeFollow, code)
        || km_code(app, ActionId::StripResumeFollow, code)
        || km_code(app, ActionId::DetailClose, code)
    {
        crate::action::dispatch(app, ActionId::LogListResumeFollow);
        return;
    }

    if app.focus == Focus::LogList {
        for id in [
            ActionId::LogListMoveDown,
            ActionId::LogListMoveUp,
            ActionId::LogListJumpDown,
            ActionId::LogListJumpUp,
            ActionId::LogListJumpTop,
            ActionId::LogListJumpBottom,
            ActionId::LogListDetailFields,
            ActionId::LogListDetailPretty,
            ActionId::LogListWrapToggle,
            ActionId::LogListChip,
            ActionId::LogListExcludeChip,
            ActionId::LogListLock,
            ActionId::LogListBookmark,
            ActionId::LogListYank,
            ActionId::LogListYankMsgLine,
            ActionId::LogListVisualLine,
            ActionId::LogListNextMatch,
            ActionId::LogListPrevMatch,
            ActionId::LogListNextSevere,
            ActionId::LogListPrevSevere,
        ] {
            if km_code(app, id, code) {
                crate::action::dispatch(app, id);
                return;
            }
        }
        if km_code(app, ActionId::LogListTime, code) {
            crate::action::dispatch(app, ActionId::LogListTime);
            return;
        }
    }

    if matches!(
        app.focus,
        Focus::ChipStrip | Focus::ExcludeStrip | Focus::HighlightStrip
    ) {
        if km_code(app, ActionId::StripPrevGroup, code) {
            crate::action::dispatch(app, ActionId::StripPrevGroup);
            return;
        }
        if km_code(app, ActionId::StripNextGroup, code) {
            crate::action::dispatch(app, ActionId::StripNextGroup);
            return;
        }
    }

    if km_code(app, ActionId::GlobalFilterNew, code) {
        crate::action::dispatch(app, ActionId::GlobalFilterNew);
        return;
    }
    if km_code(app, ActionId::GlobalHighlightNew, code) {
        crate::action::dispatch(app, ActionId::GlobalHighlightNew);
        return;
    }
    if km_code(app, ActionId::GlobalExcludeNew, code) {
        crate::action::dispatch(app, ActionId::GlobalExcludeNew);
        return;
    }
    if km_code(app, ActionId::GlobalOpenHelp, code) || km_code(app, ActionId::StripOpenHelp, code) {
        crate::action::dispatch(app, ActionId::GlobalOpenHelp);
    }
}

const VISUAL_KEYS: &[ActionId] = &[
    ActionId::VisualMoveDown,
    ActionId::VisualMoveUp,
    ActionId::VisualJumpDown,
    ActionId::VisualJumpUp,
    ActionId::VisualYankRaw,
    ActionId::VisualYankMsg,
    ActionId::VisualCancel,
];

const YANK_CHORD: &[ActionId] = &[
    ActionId::YankCancel,
    ActionId::YankCli,
    ActionId::YankMsg,
    ActionId::YankTag,
    ActionId::YankPkg,
    ActionId::YankPid,
    ActionId::YankTid,
    ActionId::YankLevel,
    ActionId::YankRaw,
    ActionId::YankLine,
    ActionId::YankTime,
];

const CHIP_CHORD: &[ActionId] = &[
    ActionId::ChipFieldCancel,
    ActionId::ChipFieldTag,
    ActionId::ChipFieldMsg,
    ActionId::ChipFieldPkg,
    ActionId::ChipFieldPid,
    ActionId::ChipFieldTid,
    ActionId::ChipFieldLevel,
];

const LOCK_CHORD: &[ActionId] = &[
    ActionId::LockCancel,
    ActionId::LockPid,
    ActionId::LockTid,
    ActionId::LockClear,
    ActionId::LockViewHighlight,
    ActionId::LockViewSevere,
];

const TIME_CHORD: &[ActionId] = &[
    ActionId::TimeCancel,
    ActionId::TimeSet,
    ActionId::TimeClear,
    ActionId::TimeHistogram,
];

const MARK_CHORD: &[ActionId] = &[
    ActionId::BookmarkCancel,
    ActionId::BookmarkAdd,
    ActionId::BookmarkRemove,
    ActionId::BookmarkManage,
];

/// First action in `ids` (priority order) whose binding matches `code`.
fn bound_action(app: &App, ids: &[ActionId], code: KeyCode) -> Option<ActionId> {
    ids.iter().copied().find(|&id| km_code(app, id, code))
}

/// Second key of `y` / `c` / `C`. The dispatched action disarms the chord;
/// an unmatched key disarms here (with a flash for chip fields).
fn handle_field_operator(app: &mut App, code: KeyCode) {
    let yank = app.pending == Pending::Yank;
    if app.focus != app::Focus::LogList {
        app.pending = Pending::None;
        return;
    }
    let ids = if yank { YANK_CHORD } else { CHIP_CHORD };
    if let Some(id) = bound_action(app, ids, code) {
        crate::action::dispatch(app, id);
        return;
    }
    app.pending = Pending::None;
    if !yank {
        app.set_flash(if chip_field_unsupported_key(app, code) {
            "NO RAW/TIMESTAMP"
        } else {
            "UNKNOWN FIELD"
        });
    }
}

/// Keys that are valid yank fields but not chip fields (`r`/`y`/`s` by default).
fn chip_field_unsupported_key(app: &App, code: KeyCode) -> bool {
    km_code(app, ActionId::YankRaw, code)
        || km_code(app, ActionId::YankLine, code)
        || km_code(app, ActionId::YankTime, code)
}
