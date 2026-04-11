use aloggrep::filter::FilterChain;
use aloggrep::formatter::{FieldSet, Formatter};
use aloggrep::OutputFormat;
use clap::Parser;

use aloggrep::Cli;

fn make_formatter(format: OutputFormat) -> Formatter {
    let cli = Cli::parse_from(["aloggrep"]);
    let chain = FilterChain::from_cli(&cli).unwrap();
    Formatter::new(format, false, &chain, FieldSet::all())
}

#[test]
fn test_context_line_text_unchanged() {
    let f = make_formatter(OutputFormat::Text);
    let mut buf = Vec::new();
    let line = "04-02 10:00:00.000  1234  5678 D Tag     : hello";
    f.write_context_line(line, &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert_eq!(out.trim(), line);
    assert!(!out.contains("context"));
}

#[test]
fn test_context_line_json_parseable() {
    let f = make_formatter(OutputFormat::Json);
    let mut buf = Vec::new();
    f.write_context_line("04-02 10:00:00.000  1234  5678 D Tag     : hello", &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("\"context\":true"));
    assert!(out.contains("\"level\":\"D\""));
    assert!(out.contains("\"tag\":\"Tag\""));
    assert!(out.contains("\"msg\":\"hello\""));
}

#[test]
fn test_context_line_json_unparseable() {
    let f = make_formatter(OutputFormat::Json);
    let mut buf = Vec::new();
    f.write_context_line("some random unparseable line", &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("\"context\":true"));
    assert!(out.contains("\"raw\":\"some random unparseable line\""));
}

#[test]
fn test_context_line_json_escaping() {
    let f = make_formatter(OutputFormat::Json);
    let mut buf = Vec::new();
    f.write_context_line("line with \"quotes\" and \\backslash", &mut buf).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.contains("\\\"quotes\\\""));
    assert!(out.contains("\\\\backslash"));
}
