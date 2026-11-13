//! Key-dispatch tests for `handlers`, split by surface; shared fixtures live here.

mod help_palette;
mod layout;
mod normal;
mod panels;
mod picker;
mod picker_highlight;
mod source;

use crate::app::App;

pub(super) fn drain_lines(app: &mut App, lines: &[&str]) {
    let (tx, rx) = std::sync::mpsc::channel();
    for line in lines {
        tx.send(crate::model::EntryRow::from_line(line).unwrap())
            .unwrap();
    }
    drop(tx);
    app.drain(&rx);
}
