mod action;
mod app;
mod bookmark;
mod candidate_match;
mod cli;
mod command_palette;
mod config;
mod dashboard;
mod event_loop;
mod export;
mod filter_model;
mod fuzzy;
mod handlers;
mod help;
mod highlight_model;
mod hist_panel;
mod ingest;
mod input;
mod keymap;
mod live_ctl;
mod log_corpus;
mod model;
mod palette;
mod picker;
mod preset;
mod preview;
mod recent;
mod scan;
mod session;
mod source_bind;
mod source_panel;
mod store;
mod text_field;
mod theme;
mod theme_builtins;
mod time_panel;
mod tui;
mod ui;
mod vocab;

use crate::cli::TuiCli;
use crate::tui::run_tui;
use clap::Parser;
use std::path::PathBuf;

fn argv0_basename() -> String {
    std::env::args_os()
        .next()
        .map(|a| {
            PathBuf::from(a)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .unwrap_or_else(|| "alnav".into())
}

fn main() -> Result<(), String> {
    let bin = argv0_basename();
    match bin.as_str() {
        "aloggrep" | "alg" => {
            let cli = alnav::Cli::parse();
            alnav::run_cli(cli);
            Ok(())
        }
        "aloggrep-tui" => run_tui(TuiCli::parse()),
        _ => {
            // Default product entry: `alnav` (and cargo-run target names).
            let args: Vec<String> = std::env::args().collect();
            if args.get(1).map(String::as_str) == Some("grep") {
                let mut cli_args = vec!["alnav grep".to_string()];
                cli_args.extend(args.into_iter().skip(2));
                let cli = alnav::Cli::parse_from(cli_args);
                alnav::run_cli(cli);
                Ok(())
            } else {
                run_tui(TuiCli::parse())
            }
        }
    }
}
