//! TUI entry: panic hook, config/keymap load, source bind, terminal setup/teardown.

use crate::app::App;
use crate::cli::{cli_has_filter_args, initial_group, initial_time_bound, validate_source, TuiCli};
use crate::event_loop::run;
use crate::live_ctl::{spawn_live_ctl, LiveBackend};
use crate::source_bind::{bind_file_source, persist_current_file};
use crate::{config, dashboard, export, input, keymap, recent};
use crossterm::event;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{self, IsTerminal};

pub(crate) fn run_tui(cli: TuiCli) -> Result<(), String> {
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            event::DisableMouseCapture
        );
        default_panic(info);
    }));

    let config_dir = config::resolve_config_dir(cli.config_path.as_deref());
    if cli.init {
        for line in keymap::init_config_dir(&config_dir, cli.force)? {
            println!("{line}");
        }
        return Ok(());
    }

    if let Err(error) = validate_source(&cli) {
        eprintln!("alnav: {error}");
        std::process::exit(2);
    }

    if !io::stdout().is_terminal() {
        eprintln!("alnav: stdout is not a terminal, refusing to start");
        std::process::exit(2);
    }

    let (app_config, config_status) = config::load_config(&config_dir);
    let theme_status = config::load_theme(&config_dir, &app_config.theme);
    let (keymap_store, keymap_status) = keymap::load_keymap(&config_dir);

    let groups = initial_group(&cli)?;
    let time_bound = initial_time_bound(&cli);
    let mut app = App::new(cli.max_lines);
    app.config = app_config;
    app.keymap = keymap_store;
    app.config_dir = config_dir.clone();
    app.recent = recent::RecentFiles::load(&config_dir);
    app.log_corpus.configure(
        app.config.log_dirs.clone(),
        app.config.log_extensions.clone(),
    );
    app.groups = groups;
    app.time_bound = time_bound;
    // R5: one rule gates both read and write, so a one-shot `--tag` run can
    // neither consume nor clobber the curated session.
    app.session_restore_disabled = !app.config.restore_session || cli_has_filter_args(&cli);
    if let Some(hint) = theme_status.status_hint() {
        app.set_flash(hint);
    }
    if let Some(hint) = config_status.status_hint() {
        app.set_flash(hint);
    }
    if let Some(hint) = keymap_status.status_hint() {
        app.set_flash(hint);
    } else if let Some(hint) = keymap::KeymapLoadStatus::warning_hint(&app.keymap.warnings) {
        app.set_flash(hint);
    }

    let mut live = if cli.hdc || cli.adb {
        let backend = if cli.hdc {
            LiveBackend::Hdc
        } else {
            LiveBackend::Adb
        };
        app.export_source = if cli.hdc {
            export::ExportSource::Hdc {
                device: cli.device.clone(),
            }
        } else {
            export::ExportSource::Adb {
                device: cli.device.clone(),
            }
        };
        match spawn_live_ctl(backend, cli.device.clone()) {
            Ok(ctl) => Some(ctl),
            Err(error) => {
                eprintln!("alnav: {error}");
                std::process::exit(2);
            }
        }
    } else if let Some(path) = cli.file.clone() {
        match bind_file_source(&mut app, &path, false) {
            Ok(()) => None,
            Err(error) => {
                eprintln!("alnav: {error}");
                std::process::exit(2);
            }
        }
    } else {
        // Preserve startup time_bound until a source is chosen; stream bind clears it.
        app.dashboard = Some(dashboard::DashboardState::new(app.recent.clone()));
        None
    };

    enable_raw_mode().map_err(|e| e.to_string())?;
    let setup: Result<Terminal<CrosstermBackend<io::Stdout>>, String> = (|| {
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            event::EnableMouseCapture
        )
        .map_err(|e| e.to_string())?;
        let backend = CrosstermBackend::new(io::stdout());
        let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;
        // Hide until a draft surface requests a hardware caret via
        // `Frame::set_cursor_position` (ratatui shows only when set).
        terminal.hide_cursor().map_err(|e| e.to_string())?;
        Ok(terminal)
    })();
    let mut terminal = match setup {
        Ok(t) => t,
        Err(e) => {
            let _ = disable_raw_mode();
            let _ = execute!(
                io::stdout(),
                LeaveAlternateScreen,
                event::DisableMouseCapture
            );
            // `live` drops here, killing the child if any
            return Err(e);
        }
    };

    let mut input = input::InputBox::default();
    let _ = cli.ignore_case; // retained for CLI compat; TUI always ignore-case
    let result = run(&mut terminal, &mut app, &mut input, &mut live);

    let disable_result = disable_raw_mode().map_err(|e| e.to_string());
    let leave_result = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )
    .map_err(|e| e.to_string());
    // R3: covers `q`, Ctrl+C and Dashboard Quit alike — they all leave `run()`.
    // A panic unwinds past this point, which is the intent (no save on panic).
    persist_current_file(&app);
    // `live` drops here, killing the child if not already killed
    disable_result.and(leave_result)?;

    result
}
