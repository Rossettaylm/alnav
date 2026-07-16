mod app;
mod filter_model;
mod ingest;
mod input;
mod model;
mod ui;

use std::io::{self, IsTerminal};
use std::time::Duration;

use clap::Parser;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::Terminal;

use aloggrep::expr::Expr;

use app::App;
use filter_model::{Group, GroupList, TimeBound};

#[derive(Parser)]
#[command(name = "aloggrep-tui", about = "Interactive vim-style viewer for aloggrep")]
struct Cli {
    /// Log file to browse
    #[arg(short, long)]
    file: String,

    #[arg(short, long, value_name = "REGEX")]
    tag: Vec<String>,

    #[arg(short, long, value_name = "REGEX")]
    msg: Vec<String>,

    #[arg(short, long, value_name = "LEVEL")]
    level: Option<String>,

    #[arg(long, value_name = "REGEX")]
    pkg: Vec<String>,

    #[arg(long, value_name = "REGEX")]
    pid: Vec<String>,

    #[arg(long, value_name = "REGEX")]
    tid: Vec<String>,

    #[arg(long, value_name = "TIME")]
    since: Option<String>,

    #[arg(long, value_name = "TIME")]
    until: Option<String>,

    #[arg(short = 'i', long)]
    ignore_case: bool,

    /// Max in-memory lines before the oldest are evicted (streaming modes)
    #[arg(long, default_value_t = 100_000)]
    max_lines: usize,
}

fn initial_group(cli: &Cli) -> Result<GroupList, String> {
    let expr = Expr::from_filters(&cli.tag, &cli.msg, &cli.pkg, &cli.pid, &cli.tid, cli.level.as_deref(), cli.ignore_case)?;
    let time = if cli.since.is_some() || cli.until.is_some() {
        Some(TimeBound { since: cli.since.clone(), until: cli.until.clone() })
    } else {
        None
    };
    if expr.is_none() && time.is_none() {
        return Ok(GroupList::default());
    }
    let mut label_parts = Vec::new();
    if !cli.tag.is_empty() { label_parts.push(format!("tag:{}", cli.tag.join("|"))); }
    if !cli.msg.is_empty() { label_parts.push(format!("msg:{}", cli.msg.join("|"))); }
    if !cli.pkg.is_empty() { label_parts.push(format!("pkg:{}", cli.pkg.join("|"))); }
    if !cli.pid.is_empty() { label_parts.push(format!("pid:{}", cli.pid.join("|"))); }
    if !cli.tid.is_empty() { label_parts.push(format!("tid:{}", cli.tid.join("|"))); }
    if let Some(l) = &cli.level { label_parts.push(format!("level:{l}")); }
    if let Some(s) = &cli.since { label_parts.push(format!("since:{s}")); }
    if let Some(u) = &cli.until { label_parts.push(format!("until:{u}")); }
    let label = if label_parts.is_empty() { "(startup filter)".to_string() } else { label_parts.join(" AND ") };
    Ok(GroupList { groups: vec![Group { label, expr, time }] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        let mut full = vec!["aloggrep-tui"];
        full.extend_from_slice(args);
        Cli::parse_from(full)
    }

    #[test]
    fn test_initial_group_empty_cli_yields_empty_group_list() {
        let c = cli(&["-f", "app.log"]);
        let groups = initial_group(&c).unwrap();
        assert!(groups.groups.is_empty());
    }

    #[test]
    fn test_initial_group_pid_only_label_contains_pid() {
        let c = cli(&["-f", "app.log", "--pid", "1234"]);
        let groups = initial_group(&c).unwrap();
        assert_eq!(groups.groups.len(), 1);
        assert!(groups.groups[0].label.contains("pid:1234"), "label was: {}", groups.groups[0].label);
    }

    #[test]
    fn test_initial_group_multiple_fields_label_contains_all() {
        let c = cli(&[
            "-f", "app.log",
            "-t", "TagA",
            "-m", "boom",
            "--pkg", "com.example",
            "--pid", "111",
            "--tid", "222",
            "-l", "E",
            "--since", "10:00:00",
            "--until", "10:01:00",
        ]);
        let groups = initial_group(&c).unwrap();
        assert_eq!(groups.groups.len(), 1);
        let label = &groups.groups[0].label;
        assert!(label.contains("tag:TagA"), "label was: {label}");
        assert!(label.contains("msg:boom"), "label was: {label}");
        assert!(label.contains("pkg:com.example"), "label was: {label}");
        assert!(label.contains("pid:111"), "label was: {label}");
        assert!(label.contains("tid:222"), "label was: {label}");
        assert!(label.contains("level:E"), "label was: {label}");
        assert!(label.contains("since:10:00:00"), "label was: {label}");
        assert!(label.contains("until:10:01:00"), "label was: {label}");
    }
}

fn main() -> Result<(), String> {
    let cli = Cli::parse();

    if !io::stdout().is_terminal() {
        eprintln!("aloggrep-tui: stdout is not a terminal, refusing to start");
        std::process::exit(2);
    }

    let groups = initial_group(&cli)?;
    let mut app = App::new(cli.max_lines);
    app.groups = groups;

    let rx = ingest::spawn_file_ingest(cli.file.clone()).map_err(|e| e.to_string())?;

    enable_raw_mode().map_err(|e| e.to_string())?;
    let setup: Result<Terminal<CrosstermBackend<io::Stdout>>, String> = (|| {
        execute!(io::stdout(), EnterAlternateScreen).map_err(|e| e.to_string())?;
        let backend = CrosstermBackend::new(io::stdout());
        Terminal::new(backend).map_err(|e| e.to_string())
    })();
    let mut terminal = match setup {
        Ok(t) => t,
        Err(e) => {
            let _ = disable_raw_mode();
            return Err(e);
        }
    };

    let mut input = input::InputBox::default();
    let result = run(&mut terminal, &mut app, &mut input, &rx, cli.ignore_case);

    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(io::stdout(), LeaveAlternateScreen).map_err(|e| e.to_string())?;

    result
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    input: &mut input::InputBox,
    rx: &std::sync::mpsc::Receiver<model::EntryRow>,
    ignore_case: bool,
) -> Result<(), String> {
    use app::Mode;

    while !app.should_quit {
        app.drain(rx);

        terminal
            .draw(|frame| {
                let [chip_area, list_area, input_area, status_area] = Layout::vertical([
                    Constraint::Length(1),
                    Constraint::Fill(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .areas(frame.area());
                ui::render_chip_strip(app, frame, chip_area);
                ui::render_log_list(app, frame, list_area);
                ui::render_input_box(input, app.mode, frame, input_area);
                let status = format!("{}/{}", app.cursor + 1, app.visible.len());
                frame.render_widget(ratatui::widgets::Paragraph::new(status), status_area);
            })
            .map_err(|e| e.to_string())?;

        if !event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            continue;
        }
        let Event::Key(key) = event::read().map_err(|e| e.to_string())? else { continue };

        match app.mode {
            Mode::Normal => handle_normal_key(app, input, key.code),
            Mode::Insert => handle_insert_key(app, input, key.code, ignore_case)?,
        }
    }
    Ok(())
}

fn handle_normal_key(app: &mut App, input: &mut input::InputBox, code: KeyCode) {
    use app::Focus;

    if code != KeyCode::Char('d') {
        app.pending_dd = false;
    }

    match (app.focus, code) {
        (_, KeyCode::Char('q')) => app.should_quit = true,
        (_, KeyCode::Tab) => app.cycle_focus_forward(),
        (_, KeyCode::BackTab) => app.cycle_focus_backward(),
        (Focus::LogList, KeyCode::Char('j')) => app.move_cursor(1),
        (Focus::LogList, KeyCode::Char('k')) => app.move_cursor(-1),
        (Focus::LogList, KeyCode::Char('g')) => app.jump_top(),
        (Focus::LogList, KeyCode::Char('G')) => app.jump_bottom(),
        (Focus::ChipStrip, KeyCode::Char('h')) => app.move_group_cursor(-1),
        (Focus::ChipStrip, KeyCode::Char('l')) => app.move_group_cursor(1),
        (Focus::ChipStrip, KeyCode::Char('d')) => {
            if app.pending_dd {
                app.delete_focused_group();
                app.pending_dd = false;
            } else {
                app.pending_dd = true;
            }
        }
        (_, KeyCode::Char('a') | KeyCode::Char('i') | KeyCode::Char('o')) => {
            app.focus = Focus::Input;
            app.mode = app::Mode::Insert;
            let _ = input;
        }
        _ => {}
    }
}

fn handle_insert_key(
    app: &mut App,
    input: &mut input::InputBox,
    code: KeyCode,
    ignore_case: bool,
) -> Result<(), String> {
    if input.popup.is_some() {
        match code {
            KeyCode::Up => input.popup.as_mut().unwrap().move_selection(-1),
            KeyCode::Down => input.popup.as_mut().unwrap().move_selection(1),
            KeyCode::Enter | KeyCode::Tab => input.confirm_popup(),
            KeyCode::Esc => input.cancel_popup(),
            KeyCode::Backspace => input.popup.as_mut().unwrap().backspace(),
            KeyCode::Char(c) => input.popup.as_mut().unwrap().push_char(c),
            _ => {}
        }
        return Ok(());
    }

    match code {
        KeyCode::Esc => {
            *input = input::InputBox::default();
            app.mode = app::Mode::Normal;
        }
        KeyCode::Char('/') => input.open_popup(),
        KeyCode::Enter => {
            if let Some(group) = input.build_group(ignore_case)? {
                app.groups.groups.push(group);
                app.rebuild_visible();
            }
        }
        KeyCode::Backspace => input.backspace(),
        KeyCode::Char(c) => input.push_char(c),
        _ => {}
    }
    Ok(())
}
