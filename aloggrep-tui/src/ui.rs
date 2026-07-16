use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::{App, Mode};
use crate::input::InputBox;
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
    let list = List::new(items).highlight_style(Style::default().add_modifier(Modifier::REVERSED));
    let mut state = ListState::default();
    if !app.visible.is_empty() {
        state.select(Some(app.cursor));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

pub fn render_chip_strip(app: &App, frame: &mut Frame, area: Rect) {
    let text = if app.groups.groups.is_empty() {
        "(no filter)".to_string()
    } else {
        app.groups
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| {
                if i == app.group_cursor && app.focus == crate::app::Focus::ChipStrip {
                    format!("[{}]", g.label)
                } else {
                    format!(" {} ", g.label)
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    };
    frame.render_widget(Paragraph::new(text), area);
}

pub fn render_input_box(input: &InputBox, mode: Mode, frame: &mut Frame, area: Rect) {
    let mut text = String::from("> ");
    for chip in &input.chips {
        text.push_str(&format!("{}:{} ", chip.field.keyword(), chip.value));
    }
    if let Some(field) = input.draft_field {
        text.push_str(&format!("{}:", field.keyword()));
    }
    text.push_str(&input.draft);
    if mode == Mode::Insert {
        text.push('_');
    }
    frame.render_widget(Paragraph::new(text), area);

    if let Some(popup) = &input.popup {
        let field_color = |f: crate::input::ChipField| match f {
            crate::input::ChipField::Tag => Color::Cyan,
            crate::input::ChipField::Msg => Color::Green,
            crate::input::ChipField::Pkg => Color::Blue,
            crate::input::ChipField::Pid => Color::Magenta,
            crate::input::ChipField::Tid => Color::LightMagenta,
            crate::input::ChipField::Level => Color::Yellow,
        };
        let spans: Vec<Span> = popup
            .matches()
            .into_iter()
            .enumerate()
            .flat_map(|(i, f)| {
                let style = Style::default().fg(field_color(f));
                let style = if i == popup.selected { style.add_modifier(Modifier::REVERSED) } else { style };
                vec![Span::styled(format!(" {} ", f.keyword()), style)]
            })
            .collect();
        let popup_area = Rect { x: area.x, y: area.y.saturating_sub(1), width: area.width, height: 1 };
        frame.render_widget(Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::NONE)), popup_area);
    }
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

    #[test]
    fn test_render_log_list_highlights_selected_row() {
        let mut app = App::new(100);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:00.000  1  1 I RowOne  : first").unwrap()).unwrap();
        tx.send(crate::model::EntryRow::from_line("04-02 10:00:01.000  1  1 I RowTwo  : second").unwrap()).unwrap();
        drop(tx);
        app.drain(&rx);
        app.cursor = 1; // select the second row

        let backend = TestBackend::new(80, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_log_list(&app, frame, frame.area()))
            .unwrap();

        let buf = terminal.backend().buffer();
        let selected_style = buf[(0, 1)].style();
        let unselected_style = buf[(0, 0)].style();
        assert_ne!(
            selected_style, unselected_style,
            "selected row (y=1) should be styled differently from the unselected row (y=0)"
        );
        assert!(selected_style.add_modifier.contains(Modifier::REVERSED));
    }
}
