//! Tests for CLI parsing, source validation and initial filter state.

use super::*;
use clap::Parser;

fn cli(args: &[&str]) -> TuiCli {
    let mut full = vec!["alnav"];
    full.extend_from_slice(args);
    TuiCli::parse_from(full)
}

#[test]
fn test_initial_group_empty_cli_yields_empty_group_list() {
    let c = cli(&["-f", "app.log"]);
    let groups = initial_group(&c).unwrap();
    assert!(groups.groups.is_empty());
}

#[test]
fn adb_source_is_accepted() {
    let cli = cli(&["--adb", "--device", "ANDROID"]);
    assert!(cli.adb);
    assert!(!cli.hdc);
    assert_eq!(cli.device.as_deref(), Some("ANDROID"));
}

#[test]
fn live_source_validation_rejects_conflicts() {
    assert_eq!(
        validate_source(&cli(&["--adb", "--hdc"])).unwrap_err(),
        "--hdc and --adb are mutually exclusive"
    );
    assert_eq!(
        validate_source(&cli(&["--adb", "-f", "app.log"])).unwrap_err(),
        "--hdc/--adb cannot be combined with -f"
    );
    assert_eq!(
        validate_source(&cli(&["--device", "ANDROID"])).unwrap_err(),
        "--device requires --hdc or --adb"
    );
}

#[test]
fn no_source_is_allowed_for_dashboard() {
    assert!(validate_source(&cli(&[])).is_ok());
    assert!(validate_source(&cli(&["--tag", "Foo"])).is_ok());
}

#[test]
fn cli_filter_args_detected_per_flag() {
    assert!(!cli_has_filter_args(&cli(&["-f", "a.log"])));
    assert!(!cli_has_filter_args(&cli(&["-f", "a.log", "-i"])));
    assert!(!cli_has_filter_args(&cli(&[
        "-f",
        "a.log",
        "--max-lines",
        "10"
    ])));
    for args in [
        vec!["-f", "a.log", "--tag", "T"],
        vec!["-f", "a.log", "--msg", "m"],
        vec!["-f", "a.log", "--level", "E"],
        vec!["-f", "a.log", "--pkg", "p"],
        vec!["-f", "a.log", "--pid", "1"],
        vec!["-f", "a.log", "--tid", "2"],
        vec!["-f", "a.log", "--since", "10:00:00"],
        vec!["-f", "a.log", "--until", "11:00:00"],
    ] {
        assert!(
            cli_has_filter_args(&cli(&args)),
            "expected suppression for {args:?}"
        );
    }
}

#[test]
fn test_initial_group_pid_only_label_contains_pid() {
    let c = cli(&["-f", "app.log", "--pid", "1234"]);
    let groups = initial_group(&c).unwrap();
    assert_eq!(groups.groups.len(), 1);
    assert!(
        groups.groups[0].label.contains("pid:1234"),
        "label was: {}",
        groups.groups[0].label
    );
}

#[test]
fn test_initial_group_multiple_fields_label_contains_all() {
    let c = cli(&[
        "-f",
        "app.log",
        "-t",
        "TagA",
        "-m",
        "boom",
        "--pkg",
        "com.example",
        "--pid",
        "111",
        "--tid",
        "222",
        "-l",
        "E",
        "--since",
        "10:00:00",
        "--until",
        "10:01:00",
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
    // Time is global, not on the Filter group label.
    assert!(!label.contains("since:"), "label was: {label}");
    assert!(!label.contains("until:"), "label was: {label}");
    let bound = initial_time_bound(&c).unwrap();
    assert_eq!(bound.since.as_deref(), Some("10:00:00"));
    assert_eq!(bound.until.as_deref(), Some("10:01:00"));
}

#[test]
fn test_initial_time_only_yields_empty_groups() {
    let c = cli(&["-f", "app.log", "--since", "10:00:00"]);
    let groups = initial_group(&c).unwrap();
    assert!(groups.groups.is_empty());
    let bound = initial_time_bound(&c).unwrap();
    assert_eq!(bound.since.as_deref(), Some("10:00:00"));
    assert!(bound.until.is_none());
}
