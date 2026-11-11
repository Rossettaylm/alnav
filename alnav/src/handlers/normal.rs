//! Normal-mode key handling: focus/strip/loglist keys, leader and strip `d` chords, chip helpers.

use crate::app::App;
use crate::handlers::km_code;
use crate::{app, input, keymap};
use crossterm::event::KeyCode;

fn handle_strip_d_chord(app: &mut App, kind: app::StripKind, code: KeyCode) -> bool {
    use app::StripKind;
    use keymap::ActionId;
    if !matches!(
        (kind, app.focus),
        (StripKind::Filter, app::Focus::ChipStrip)
            | (StripKind::Exclude, app::Focus::ExcludeStrip)
            | (StripKind::Highlight, app::Focus::HighlightStrip)
    ) {
        return false;
    }
    if app.pending_d {
        if km_code(app, ActionId::StripDDelete, code) {
            crate::action::dispatch(app, ActionId::StripDDelete);
            return true;
        }
        if km_code(app, ActionId::StripDDisable, code) {
            crate::action::dispatch(app, ActionId::StripDDisable);
            return true;
        }
        app.pending_d = false;
        return false;
    }
    if km_code(app, ActionId::StripPendingD, code) {
        crate::action::dispatch(app, ActionId::StripPendingD);
        return true;
    }
    false
}

fn handle_leader_key(app: &mut App, code: KeyCode) -> bool {
    use keymap::ActionId;
    if app.pending_leader {
        app.pending_leader = false;
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
    use keymap::ActionId;

    // Visual-line: handle selection motions / yank before anything else.
    if app.visual_anchor.is_some() && app.focus == Focus::LogList {
        if km_code(app, ActionId::VisualMoveDown, code) {
            crate::action::dispatch(app, ActionId::VisualMoveDown);
            return;
        }
        if km_code(app, ActionId::VisualMoveUp, code) {
            crate::action::dispatch(app, ActionId::VisualMoveUp);
            return;
        }
        if km_code(app, ActionId::VisualJumpDown, code) {
            crate::action::dispatch(app, ActionId::VisualJumpDown);
            return;
        }
        if km_code(app, ActionId::VisualJumpUp, code) {
            crate::action::dispatch(app, ActionId::VisualJumpUp);
            return;
        }
        if km_code(app, ActionId::VisualYankRaw, code) {
            crate::action::dispatch(app, ActionId::VisualYankRaw);
            return;
        }
        if km_code(app, ActionId::VisualYankMsg, code) {
            crate::action::dispatch(app, ActionId::VisualYankMsg);
            return;
        }
        if km_code(app, ActionId::VisualCancel, code) {
            crate::action::dispatch(app, ActionId::VisualCancel);
            return;
        }
        app.clear_visual();
        // fall through so the key still does its Normal action
    }

    if handle_leader_key(app, code) {
        return;
    }

    // Yank operator pending: consume the second key (or Esc) and return.
    if app.pending_yank {
        if app.focus == Focus::LogList {
            let yank_id = if km_code(app, ActionId::YankCancel, code) {
                Some(ActionId::YankCancel)
            } else if km_code(app, ActionId::YankCli, code) {
                Some(ActionId::YankCli)
            } else if km_code(app, ActionId::YankMsg, code) {
                Some(ActionId::YankMsg)
            } else if km_code(app, ActionId::YankTag, code) {
                Some(ActionId::YankTag)
            } else if km_code(app, ActionId::YankPkg, code) {
                Some(ActionId::YankPkg)
            } else if km_code(app, ActionId::YankPid, code) {
                Some(ActionId::YankPid)
            } else if km_code(app, ActionId::YankTid, code) {
                Some(ActionId::YankTid)
            } else if km_code(app, ActionId::YankLevel, code) {
                Some(ActionId::YankLevel)
            } else if km_code(app, ActionId::YankRaw, code)
                || km_code(app, ActionId::YankLine, code)
            {
                Some(ActionId::YankRaw)
            } else if km_code(app, ActionId::YankTime, code) {
                Some(ActionId::YankTime)
            } else {
                None
            };
            if let Some(id) = yank_id {
                crate::action::dispatch(app, id);
            } else {
                app.pending_yank = false;
            }
        } else {
            app.pending_yank = false;
        }
        return;
    }

    // Chip-from-cursor operator pending.
    if app.pending_chip {
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::ChipFieldCancel, code) {
                crate::action::dispatch(app, ActionId::ChipFieldCancel);
            } else if let Some(id) = chip_action_from_keymap(app, code) {
                crate::action::dispatch(app, id);
            } else if chip_field_unsupported_key(app, code) {
                app.pending_chip = false;
                app.set_flash("NO RAW/TIMESTAMP");
            } else {
                app.pending_chip = false;
                app.set_flash("UNKNOWN FIELD");
            }
        } else {
            app.pending_chip = false;
        }
        return;
    }

    // Exclude-from-cursor operator pending.
    if app.pending_exclude {
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::ChipFieldCancel, code) {
                crate::action::dispatch(app, ActionId::ChipFieldCancel);
            } else if let Some(id) = chip_action_from_keymap(app, code) {
                crate::action::dispatch(app, id);
            } else if chip_field_unsupported_key(app, code) {
                app.pending_exclude = false;
                app.set_flash("NO RAW/TIMESTAMP");
            } else {
                app.pending_exclude = false;
                app.set_flash("UNKNOWN FIELD");
            }
        } else {
            app.pending_exclude = false;
        }
        return;
    }

    // Session lock / view-focus operator pending.
    if app.pending_lock {
        app.pending_lock = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::LockCancel, code) {
                crate::action::dispatch(app, ActionId::LockCancel);
            } else if km_code(app, ActionId::LockPid, code) {
                crate::action::dispatch(app, ActionId::LockPid);
            } else if km_code(app, ActionId::LockTid, code) {
                crate::action::dispatch(app, ActionId::LockTid);
            } else if km_code(app, ActionId::LockClear, code) {
                crate::action::dispatch(app, ActionId::LockClear);
            } else if km_code(app, ActionId::LockViewHighlight, code) {
                crate::action::dispatch(app, ActionId::LockViewHighlight);
            } else if km_code(app, ActionId::LockViewSevere, code) {
                crate::action::dispatch(app, ActionId::LockViewSevere);
            } else {
                app.set_flash("UNKNOWN");
            }
        }
        return;
    }

    // Global time-window operator pending (file mode).
    if app.pending_time {
        app.pending_time = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::TimeCancel, code) {
                crate::action::dispatch(app, ActionId::TimeCancel);
            } else if km_code(app, ActionId::TimeSet, code) {
                crate::action::dispatch(app, ActionId::TimeSet);
            } else if km_code(app, ActionId::TimeClear, code) {
                crate::action::dispatch(app, ActionId::TimeClear);
            } else if km_code(app, ActionId::TimeHistogram, code) {
                crate::action::dispatch(app, ActionId::TimeHistogram);
            } else {
                app.set_flash("UNKNOWN");
            }
        }
        return;
    }

    // Bookmark operator pending.
    if app.pending_m {
        app.pending_m = false;
        if app.focus == Focus::LogList {
            if km_code(app, ActionId::BookmarkCancel, code) {
                crate::action::dispatch(app, ActionId::BookmarkCancel);
            } else if km_code(app, ActionId::BookmarkAdd, code) {
                crate::action::dispatch(app, ActionId::BookmarkAdd);
            } else if km_code(app, ActionId::BookmarkRemove, code) {
                crate::action::dispatch(app, ActionId::BookmarkRemove);
            } else if km_code(app, ActionId::BookmarkManage, code) {
                crate::action::dispatch(app, ActionId::BookmarkManage);
            } else {
                app.set_flash("UNKNOWN");
            }
        }
        return;
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
        app.pending_d = false;
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

fn chip_action_from_keymap(app: &App, code: KeyCode) -> Option<keymap::ActionId> {
    use keymap::ActionId;
    if km_code(app, ActionId::ChipFieldTag, code) {
        Some(ActionId::ChipFieldTag)
    } else if km_code(app, ActionId::ChipFieldMsg, code) {
        Some(ActionId::ChipFieldMsg)
    } else if km_code(app, ActionId::ChipFieldPkg, code) {
        Some(ActionId::ChipFieldPkg)
    } else if km_code(app, ActionId::ChipFieldPid, code) {
        Some(ActionId::ChipFieldPid)
    } else if km_code(app, ActionId::ChipFieldTid, code) {
        Some(ActionId::ChipFieldTid)
    } else if km_code(app, ActionId::ChipFieldLevel, code) {
        Some(ActionId::ChipFieldLevel)
    } else {
        None
    }
}

/// Keys that are valid yank fields but not chip fields (`r`/`y`/`s` by default).
fn chip_field_unsupported_key(app: &App, code: KeyCode) -> bool {
    use keymap::ActionId;
    km_code(app, ActionId::YankRaw, code)
        || km_code(app, ActionId::YankLine, code)
        || km_code(app, ActionId::YankTime, code)
}
