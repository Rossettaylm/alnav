mod app;
mod filter_model;
mod ingest;
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
    if let Some(l) = &cli.level { label_parts.push(format!("level:{l}")); }
    if let Some(s) = &cli.since { label_parts.push(format!("since:{s}")); }
    if let Some(u) = &cli.until { label_parts.push(format!("until:{u}")); }
    let label = if label_parts.is_empty() { "(startup filter)".to_string() } else { label_parts.join(" AND ") };
    Ok(GroupList { groups: vec![Group { label, expr, time }] })
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
    execute!(io::stdout(), EnterAlternateScreen).map_err(|e| e.to_string())?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).map_err(|e| e.to_string())?;

    let result = run(&mut terminal, &mut app, &rx);

    disable_raw_mode().map_err(|e| e.to_string())?;
    execute!(io::stdout(), LeaveAlternateScreen).map_err(|e| e.to_string())?;

    result
}

fn run<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    rx: &std::sync::mpsc::Receiver<model::EntryRow>,
) -> Result<(), String> {
    while !app.should_quit {
        app.drain(rx);

        terminal
            .draw(|frame| {
                let [list_area, status_area] =
                    Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
                ui::render_log_list(app, frame, list_area);
                let status = format!("{}/{}", app.cursor + 1, app.visible.len());
                frame.render_widget(ratatui::widgets::Paragraph::new(status), status_area);
            })
            .map_err(|e| e.to_string())?;

        if event::poll(Duration::from_millis(100)).map_err(|e| e.to_string())? {
            if let Event::Key(key) = event::read().map_err(|e| e.to_string())? {
                match key.code {
                    KeyCode::Char('q') => app.should_quit = true,
                    KeyCode::Char('j') => app.move_cursor(1),
                    KeyCode::Char('k') => app.move_cursor(-1),
                    KeyCode::Char('g') => app.jump_top(),
                    KeyCode::Char('G') => app.jump_bottom(),
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
