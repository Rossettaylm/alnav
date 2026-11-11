//! Tests for `ui` rendering, split by surface; shared buffer helpers and modal shell test.

mod candidates;
mod compare;
mod crash_detail;
mod dashboard;
mod entry;
mod help;
mod input;
mod layout;
mod log_list;
mod palette;
mod picker;
mod status;
mod strips;
mod summary;
mod text;

use super::*;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
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
fn modal_shell_draws_rounded_corners() {
    let backend = TestBackend::new(40, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            let area = Rect::new(2, 1, 36, 5);
            let _ = render_modal_shell("Input", frame, area);
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    // Rounded BorderType corners (ratatui): ╭ ╮ ╰ ╯
    let mut corners = 0u32;
    for y in 1..6u16 {
        for x in 2..38u16 {
            match buf[(x, y)].symbol() {
                "╭" | "╮" | "╰" | "╯" => corners += 1,
                _ => {}
            }
        }
    }
    assert_eq!(corners, 4, "modal shell should paint four rounded corners");
}
