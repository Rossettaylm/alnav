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
