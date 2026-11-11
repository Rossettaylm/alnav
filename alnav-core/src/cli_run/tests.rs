use super::*;
use crate::formatter::FieldSet;
use clap::Parser;

#[test]
fn live_sources_are_mutually_exclusive() {
    let cli = Cli::parse_from(["alnav", "--hdc", "--adb"]);
    assert_eq!(
        validate_source(&cli).unwrap_err(),
        "--hdc and --adb are mutually exclusive"
    );
}

#[test]
fn device_accepts_either_live_source() {
    let adb = Cli::parse_from(["alnav", "--adb", "--device", "A"]);
    let hdc = Cli::parse_from(["alnav", "--hdc", "--device", "H"]);
    assert!(validate_source(&adb).is_ok());
    assert!(validate_source(&hdc).is_ok());
}

#[test]
fn test_follow_pid_basic() {
    let lines: Vec<String> = vec![
        "04-02 10:00:00.000  1234  5678 E OkHttp  : error".to_string(),
        "04-02 10:00:01.000  1234  5679 D OkHttp  : debug same pid".to_string(),
        "04-02 10:00:02.000  9999  1111 I Other   : different pid".to_string(),
        "04-02 10:00:03.000  1234  9999 W Tag2    : same pid again".to_string(),
    ];
    let cli = Cli::parse_from(["alnav", "--level", "E", "--follow-pid"]);
    let filter_chain = FilterChain::from_cli(&cli).unwrap();
    let formatter = Formatter::new(
        OutputFormat::Text,
        false,
        &filter_chain,
        FieldSet::all(),
        &[],
    );
    let mut buf = Vec::new();
    let matched = run_follow(&lines, &filter_chain, &formatter, &cli, &mut buf);
    let out = String::from_utf8(buf).unwrap();
    assert_eq!(matched, 3);
    assert!(out.contains("error"));
    assert!(out.contains("debug same pid"));
    assert!(!out.contains("different pid"));
    assert!(out.contains("same pid again"));
}

#[test]
fn test_follow_tid_basic() {
    let lines: Vec<String> = vec![
        "04-02 10:00:00.000  1234  5678 E OkHttp  : error".to_string(),
        "04-02 10:00:01.000  9999  5678 D Other   : same tid".to_string(),
        "04-02 10:00:02.000  1234  9999 I Tag2    : different tid".to_string(),
    ];
    let cli = Cli::parse_from(["alnav", "--level", "E", "--follow-tid"]);
    let filter_chain = FilterChain::from_cli(&cli).unwrap();
    let formatter = Formatter::new(
        OutputFormat::Text,
        false,
        &filter_chain,
        FieldSet::all(),
        &[],
    );
    let mut buf = Vec::new();
    let matched = run_follow(&lines, &filter_chain, &formatter, &cli, &mut buf);
    let out = String::from_utf8(buf).unwrap();
    assert_eq!(matched, 2);
    assert!(out.contains("error"));
    assert!(out.contains("same tid"));
    assert!(!out.contains("different tid"));
}

#[test]
fn test_follow_pid_and_tid() {
    let lines: Vec<String> = vec![
        "04-02 10:00:00.000  1234  5678 E Tag     : error".to_string(),
        "04-02 10:00:01.000  1234  5678 D Tag     : same both".to_string(),
        "04-02 10:00:02.000  1234  9999 I Tag     : same pid diff tid".to_string(),
        "04-02 10:00:03.000  9999  5678 I Tag     : diff pid same tid".to_string(),
    ];
    let cli = Cli::parse_from(["alnav", "--level", "E", "--follow-pid", "--follow-tid"]);
    let filter_chain = FilterChain::from_cli(&cli).unwrap();
    let formatter = Formatter::new(
        OutputFormat::Text,
        false,
        &filter_chain,
        FieldSet::all(),
        &[],
    );
    let mut buf = Vec::new();
    let matched = run_follow(&lines, &filter_chain, &formatter, &cli, &mut buf);
    assert_eq!(matched, 2);
}

#[test]
fn test_follow_pid_no_matches() {
    let lines: Vec<String> =
        vec!["04-02 10:00:00.000  1234  5678 D Tag     : debug only".to_string()];
    let cli = Cli::parse_from(["alnav", "--level", "E", "--follow-pid"]);
    let filter_chain = FilterChain::from_cli(&cli).unwrap();
    let formatter = Formatter::new(
        OutputFormat::Text,
        false,
        &filter_chain,
        FieldSet::all(),
        &[],
    );
    let mut buf = Vec::new();
    let matched = run_follow(&lines, &filter_chain, &formatter, &cli, &mut buf);
    assert_eq!(matched, 0);
}
