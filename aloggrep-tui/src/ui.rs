use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem};
use ratatui::Frame;

use crate::app::App;
use aloggrep::parser::Level;

fn level_color(level: Level) -> Color {
    match level {
        Level::V | Level::D => Color::DarkGray,
        Level::I => Color::White,
        Level::W => Color::Yellow,
        Level::E | Level::F => Color::Red,
    }
}

pub fn render_log_list(app: &App, frame: &mut Frame, area: Rect) {
    let items: Vec<ListItem> = app
        .visible_rows()
        .map(|row| {
            let line = Line::from(vec![
                Span::raw(format!("{:<18} ", row.timestamp)),
                Span::styled(format!("{} ", row.level.as_char()), Style::default().fg(level_color(row.level))),
                Span::raw(format!("{:<16} ", row.tag)),
                Span::raw(format!("[{}:{}] ", row.pid, row.tid)),
                Span::raw(row.msg.to_string()),
            ]);
            ListItem::new(line)
        })
        .collect();
    frame.render_widget(List::new(items), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn cell_text(buf: &ratatui::buffer::Buffer) -> String {
        let mut s = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn test_render_log_list_shows_tag_and_msg() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I MyTag   : hello world").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&app, frame, frame.area()))
            .unwrap();

        let content = cell_text(terminal.backend().buffer());
        assert!(content.contains("MyTag"));
        assert!(content.contains("hello world"));
    }
}
