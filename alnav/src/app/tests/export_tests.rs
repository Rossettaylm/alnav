//! `yc` contract: the exported `-e` selects exactly the rows the TUI shows.

use super::*;
use crate::filter_model::{GroupList, TimeBound};
use crate::fuzzy::SameFieldOp;
use crate::input::{Chip, ChipField};
use alnav::expr::Expr;
use alnav::parser::LogEntry;

const CORPUS: &[&str] = &[
    "04-02 10:00:00.000    12    20 I OkHttp  : request foo started",
    "04-02 10:00:01.000   123    20 E OkHttp  : timeout bar after foo",
    "04-02 10:00:02.000    12    21 W Retrofit: retry FOO x BAR",
    "04-02 10:00:03.000    12    20 D Noise   : spam can't \"q\" (0)|x",
    "04-02 10:05:00.000  4444  4445 E ActivityManager: ANR in com.example",
    "04-16 10:06:00.297  12  20 I A00201/com.app/QRouter: hilog with pkg foo",
    "04-16 10:07:00.297  12  20 E A00201/com.other/QRouter: other pkg",
    "not a log line at all",
];

fn chip(field: ChipField, value: &str) -> Chip {
    Chip {
        field,
        value: value.into(),
    }
}

fn group(chips: Vec<Chip>, op: SameFieldOp) -> Group {
    Group {
        label: "g".into(),
        chips,
        enabled: true,
        same_field_op: op,
    }
}

/// Visible rows (by corpus line) in a stream session under `setup`.
fn tui_visible(setup: impl FnOnce(&mut App)) -> (Vec<String>, String) {
    let mut app = App::new(1000);
    app.export_source = crate::export::ExportSource::File("corpus.log".into());
    setup(&mut app);
    app.rebuild_visible();
    let (tx, rx) = std::sync::mpsc::channel();
    for line in CORPUS {
        tx.send(EntryRow::from_line_or_raw(line)).unwrap();
    }
    drop(tx);
    app.drain(&rx);
    let rows = (0..app.index.visible.len())
        .map(|i| app.row_at(i).unwrap().raw.clone())
        .collect();
    (rows, app.export_cli_command())
}

/// Lines `alnav grep -i -e <expr>` would print for the same corpus.
fn cli_selected(cmd: &str) -> Vec<String> {
    let expr = cmd.split_once(" -e '").map(|(_, rest)| {
        rest.strip_suffix('\'')
            .expect("single -e arg is last")
            .replace("'\\''", "'")
    });
    let parsed = expr.map(|e| Expr::parse(&e, true).expect("exported -e parses"));
    CORPUS
        .iter()
        .filter(|line| match (&parsed, LogEntry::parse(line)) {
            (None, _) => true,
            (Some(e), Some(entry)) => e.matches(&entry),
            (Some(_), None) => false,
        })
        .map(|s| s.to_string())
        .collect()
}

fn assert_consistent(name: &str, setup: impl FnOnce(&mut App)) {
    let (tui, cmd) = tui_visible(setup);
    assert_eq!(tui, cli_selected(&cmd), "{name}: {cmd}");
}

#[test]
fn exported_command_selects_exactly_the_tui_visible_rows() {
    assert_consistent("no filter", |_| {});
    assert_consistent("pid lock is exact (12 vs 123)", |app| {
        app.lock_pid = Some("12".into());
    });
    assert_consistent("tid lock", |app| app.lock_tid = Some("20".into()));
    assert_consistent("multi-word chip: atoms in any order", |app| {
        app.groups.groups.push(group(
            vec![chip(ChipField::Msg, "bar foo")],
            SameFieldOp::And,
        ));
    });
    assert_consistent("startup Or group", |app| {
        app.groups.groups.push(group(
            vec![
                chip(ChipField::Tag, "okhttp"),
                chip(ChipField::Tag, "retrofit"),
                chip(ChipField::Level, "W"),
            ],
            SameFieldOp::Or,
        ));
    });
    assert_consistent("groups OR, exclude, metacharacters, quotes", |app| {
        app.groups.groups.push(group(
            vec![chip(ChipField::Tag, "OkHttp")],
            SameFieldOp::And,
        ));
        app.groups.groups.push(group(
            vec![chip(ChipField::Msg, "can't \"q\" (0)|x")],
            SameFieldOp::And,
        ));
        app.groups
            .push_exclude(chip(ChipField::Msg, "timeout"))
            .unwrap();
    });
    assert_consistent("pkg chip never matches rows without a package", |app| {
        app.groups
            .groups
            .push(group(vec![chip(ChipField::Pkg, "com")], SameFieldOp::And));
    });
    assert_consistent("pkg exclude", |app| {
        app.groups
            .push_exclude(chip(ChipField::Pkg, "com.other"))
            .unwrap();
    });
    assert_consistent("disabled rules are skipped", |app| {
        let mut off = group(vec![chip(ChipField::Tag, "Noise")], SameFieldOp::And);
        off.enabled = false;
        app.groups.groups.push(off);
        app.groups
            .push_exclude(chip(ChipField::Tag, "okhttp"))
            .unwrap();
        app.groups.excludes[0].enabled = false;
    });
    assert_consistent("time window, both forms", |app| {
        app.time_bound = Some(TimeBound {
            since: Some("10:00:01".into()),
            until: Some("04-16 10:06:00".into()),
        });
    });
    assert_consistent("everything at once", |app| {
        app.groups = GroupList::default();
        app.groups.groups.push(group(
            vec![chip(ChipField::Level, "I"), chip(ChipField::Msg, "foo")],
            SameFieldOp::And,
        ));
        app.groups
            .push_exclude(chip(ChipField::Tag, "retrofit"))
            .unwrap();
        app.lock_pid = Some("12".into());
        app.time_bound = Some(TimeBound {
            since: Some("09:00:00".into()),
            until: None,
        });
    });
}
