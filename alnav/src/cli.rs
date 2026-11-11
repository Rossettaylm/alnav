//! CLI definition (`TuiCli`) and startup-argument validation / initial filter state.

use crate::filter_model::{Group, GroupList, TimeBound};
use crate::fuzzy::SameFieldOp;
use crate::input;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "alnav",
    about = "Interactive vim-style App/Android Log Navigator (TUI)"
)]
pub(crate) struct TuiCli {
    /// Log file to browse
    #[arg(short, long)]
    pub(crate) file: Option<String>,

    #[arg(short, long, value_name = "REGEX")]
    pub(crate) tag: Vec<String>,

    #[arg(short, long, value_name = "REGEX")]
    pub(crate) msg: Vec<String>,

    #[arg(short, long, value_name = "LEVEL")]
    pub(crate) level: Option<String>,

    #[arg(long, value_name = "REGEX")]
    pub(crate) pkg: Vec<String>,

    #[arg(long, value_name = "REGEX")]
    pub(crate) pid: Vec<String>,

    #[arg(long, value_name = "REGEX")]
    pub(crate) tid: Vec<String>,

    #[arg(long, value_name = "TIME")]
    pub(crate) since: Option<String>,

    #[arg(long, value_name = "TIME")]
    pub(crate) until: Option<String>,

    /// Accepted for CLI compatibility; TUI filter/search are always case-insensitive.
    #[arg(short = 'i', long)]
    pub(crate) ignore_case: bool,

    /// Max in-memory lines before the oldest are evicted (live mode only; ignored for `-f`)
    #[arg(long, default_value_t = 500_000)]
    pub(crate) max_lines: usize,

    /// Capture logs directly from hdc hilog (HarmonyOS device)
    #[arg(long)]
    pub(crate) hdc: bool,

    /// Capture logs directly from adb logcat (Android device)
    #[arg(long)]
    pub(crate) adb: bool,

    /// Device serial number (for --hdc or --adb with multiple devices)
    #[arg(long, value_name = "SERIAL")]
    pub(crate) device: Option<String>,

    /// Config directory override (reads `theme.toml`/`config.toml`/`keymap.toml`; default: `$ALNAV_HOME` or `~/.config/alnav`)
    #[arg(long, value_name = "DIR")]
    pub(crate) config_path: Option<PathBuf>,

    /// Write default `config.toml` and `keymap.toml` into the config directory, then exit.
    #[arg(long)]
    pub(crate) init: bool,

    /// With `--init`, overwrite existing config/keymap files.
    #[arg(long)]
    pub(crate) force: bool,
}

pub(crate) fn validate_source(cli: &TuiCli) -> Result<(), String> {
    if cli.hdc && cli.adb {
        return Err("--hdc and --adb are mutually exclusive".into());
    }
    if (cli.hdc || cli.adb) && cli.file.is_some() {
        return Err("--hdc/--adb cannot be combined with -f".into());
    }
    if cli.device.is_some() && !cli.hdc && !cli.adb {
        return Err("--device requires --hdc or --adb".into());
    }
    // No source → Dashboard (deferred bind). Filters may still be provided.
    Ok(())
}

/// Startup chip Filter group (no time — time goes to [`App::time_bound`]).
pub(crate) fn initial_group(cli: &TuiCli) -> Result<GroupList, String> {
    // TUI always matches case-insensitively (CLI `-i` is a no-op).
    // Startup CLI multi-values keep OR (`msg:a|b` label); interactive chips use And.
    if cli.tag.is_empty()
        && cli.msg.is_empty()
        && cli.pkg.is_empty()
        && cli.pid.is_empty()
        && cli.tid.is_empty()
        && cli.level.is_none()
    {
        return Ok(GroupList::default());
    }
    if let Some(l) = &cli.level {
        if alnav::parser::Level::from_str(l).is_none() {
            return Err(format!("unknown level '{}', expected V/D/I/W/E/F", l));
        }
    }
    let mut chips = Vec::new();
    for v in &cli.tag {
        chips.push(input::Chip {
            field: input::ChipField::Tag,
            value: v.clone(),
        });
    }
    for v in &cli.msg {
        chips.push(input::Chip {
            field: input::ChipField::Msg,
            value: v.clone(),
        });
    }
    for v in &cli.pkg {
        chips.push(input::Chip {
            field: input::ChipField::Pkg,
            value: v.clone(),
        });
    }
    for v in &cli.pid {
        chips.push(input::Chip {
            field: input::ChipField::Pid,
            value: v.clone(),
        });
    }
    for v in &cli.tid {
        chips.push(input::Chip {
            field: input::ChipField::Tid,
            value: v.clone(),
        });
    }
    if let Some(l) = &cli.level {
        chips.push(input::Chip {
            field: input::ChipField::Level,
            value: l.clone(),
        });
    }
    let mut label_parts = Vec::new();
    if !cli.tag.is_empty() {
        label_parts.push(format!("tag:{}", cli.tag.join("|")));
    }
    if !cli.msg.is_empty() {
        label_parts.push(format!("msg:{}", cli.msg.join("|")));
    }
    if !cli.pkg.is_empty() {
        label_parts.push(format!("pkg:{}", cli.pkg.join("|")));
    }
    if !cli.pid.is_empty() {
        label_parts.push(format!("pid:{}", cli.pid.join("|")));
    }
    if !cli.tid.is_empty() {
        label_parts.push(format!("tid:{}", cli.tid.join("|")));
    }
    if let Some(l) = &cli.level {
        label_parts.push(format!("level:{l}"));
    }
    let label = if label_parts.is_empty() {
        "(startup filter)".to_string()
    } else {
        label_parts.join(" AND ")
    };
    Ok(GroupList {
        groups: vec![Group {
            label,
            chips,
            enabled: true,
            same_field_op: SameFieldOp::Or,
        }],
        excludes: Vec::new(),
    })
}

/// Startup `--since`/`--until` → global session time window (not a Filter group).
pub(crate) fn initial_time_bound(cli: &TuiCli) -> Option<TimeBound> {
    if cli.since.is_none() && cli.until.is_none() {
        return None;
    }
    Some(TimeBound {
        since: cli.since.clone(),
        until: cli.until.clone(),
    })
}

/// Whether any one-shot CLI filter flag was passed. Such a run is a command-line
/// intent, not a curated session, so R5 suppresses both session read and write.
/// `-i` (no-op in the TUI) and `--max-lines` do not count.
pub(crate) fn cli_has_filter_args(cli: &TuiCli) -> bool {
    !cli.tag.is_empty()
        || !cli.msg.is_empty()
        || !cli.pkg.is_empty()
        || !cli.pid.is_empty()
        || !cli.tid.is_empty()
        || cli.level.is_some()
        || cli.since.is_some()
        || cli.until.is_some()
}

#[cfg(test)]
mod tests;
