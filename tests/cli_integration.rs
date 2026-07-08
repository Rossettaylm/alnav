use std::io::Write;
use std::process::Command;

use assert_cmd::cargo::CommandCargoExt;
use tempfile::NamedTempFile;

/// Helper: create a temp file with given content and return its path.
fn temp_log(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().unwrap();
    f.write_all(content.as_bytes()).unwrap();
    f.flush().unwrap();
    f
}

/// Helper: run aloggrep with args and stdin content.
fn run_with_stdin(args: &[&str], stdin: &str) -> std::process::Output {
    let mut cmd = Command::cargo_bin("aloggrep").unwrap();
    cmd.args(args);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

/// Helper: run aloggrep with -f flag pointing to temp file.
fn run_with_file(args: &[&str], content: &str) -> (std::process::Output, NamedTempFile) {
    let f = temp_log(content);
    let path = f.path().to_str().unwrap().to_string();
    let mut all_args = vec!["-f", &path];
    all_args.extend_from_slice(args);
    let mut cmd = Command::cargo_bin("aloggrep").unwrap();
    cmd.args(&all_args);
    let output = cmd.output().unwrap();
    (output, f)
}

const SAMPLE_LOG: &str = "\
04-02 10:00:00.000  1234  5678 D OkHttp  : request started
04-02 10:00:01.000  1234  5678 I OkHttp  : connected to server
04-02 10:00:02.000  1234  5678 W OkHttp  : slow response 500ms
04-02 10:00:03.000  1234  5678 E OkHttp  : timeout after 30000ms
04-02 10:00:04.000  2222  3333 D Retrofit: sending request
04-02 10:00:05.000  2222  3333 I Retrofit: response 200
04-02 10:00:06.000  3333  4444 E DB      : connection lost
04-02 10:00:07.000  3333  4444 W DB      : reconnecting
04-02 10:00:08.000  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main
04-02 10:00:09.000  4444  5555 E ActivityManager: ANR in com.example.app
";

// ═══════════════════════════════════════════════════════════════════════
// Basic filtering
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_tag_filter() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 4);
    assert!(stdout.lines().all(|l| l.contains("OkHttp")));
}

#[test]
fn test_cli_msg_filter() {
    let (out, _f) = run_with_file(&["--msg", "timeout"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("timeout after 30000ms"));
}

#[test]
fn test_cli_level_filter() {
    let (out, _f) = run_with_file(&["--level", "E"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // E and F level: 4 lines (OkHttp E, DB E, AndroidRuntime E, ActivityManager E)
    assert_eq!(stdout.lines().count(), 4);
}

#[test]
fn test_cli_combined_tag_level() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp", "--level", "W"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // OkHttp lines with level >= W: W and E = 2 lines
    assert_eq!(stdout.lines().count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Output formats
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_format_json() {
    let (out, _f) = run_with_file(&["--tag", "DB", "--format", "json"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    for line in stdout.lines() {
        assert!(line.starts_with('{'));
        assert!(line.ends_with('}'));
        assert!(line.contains("\"tag\":\"DB\""));
    }
}

#[test]
fn test_cli_format_csv() {
    let (out, _f) = run_with_file(&["--tag", "DB", "--format", "csv"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    for line in stdout.lines() {
        // CSV has comma-separated fields
        assert!(line.contains(','));
        assert!(line.contains("DB"));
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Count mode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_count() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp", "--count"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.trim(), "4");
}

#[test]
fn test_cli_count_no_match() {
    let (out, _f) = run_with_file(&["--tag", "NonExistent", "--count"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(!out.status.success()); // exit code 1
    assert_eq!(stdout.trim(), "0");
}

// ═══════════════════════════════════════════════════════════════════════
// Summary mode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_summary() {
    let (out, _f) = run_with_file(&["--summary"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(stdout.contains("\"total\":10"));
    assert!(stdout.contains("\"matched\":10"));
    assert!(stdout.contains("\"crashes\":2"));
}

#[test]
fn test_cli_summary_with_filter() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp", "--summary"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(stdout.contains("\"total\":4"));
    assert!(stdout.contains("\"matched\":4"));
}

// ═══════════════════════════════════════════════════════════════════════
// Dedupe mode
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_dedupe() {
    let content = "\
04-02 10:00:00.000  1234  5678 E OkHttp  : timeout after 100ms
04-02 10:00:01.000  1234  5678 E OkHttp  : timeout after 200ms
04-02 10:00:02.000  1234  5678 E OkHttp  : timeout after 300ms
04-02 10:00:03.000  1234  5678 E OkHttp  : connection refused
";
    let (out, _f) = run_with_file(&["--dedupe"], content);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(stdout.contains("3x"));
    assert!(stdout.contains("timeout after <N>ms"));
    assert!(stdout.contains("1x"));
}

#[test]
fn test_cli_dedupe_json() {
    let content = "\
04-02 10:00:00.000  1234  5678 E Tag     : err 100
04-02 10:00:01.000  1234  5678 E Tag     : err 200
";
    let (out, _f) = run_with_file(&["--dedupe", "--format", "json"], content);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(stdout.contains("\"count\":2"));
    assert!(stdout.contains("\"pattern\":\"err <N>\""));
}

// ═══════════════════════════════════════════════════════════════════════
// Limit
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_limit() {
    let (out, _f) = run_with_file(&["--limit", "3"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// Invert
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_invert() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp", "-v"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // All lines NOT matching OkHttp: 10 - 4 = 6
    assert_eq!(stdout.lines().count(), 6);
    assert!(!stdout.contains("OkHttp"));
}

// ═══════════════════════════════════════════════════════════════════════
// Case-insensitive
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_ignore_case() {
    let (out, _f) = run_with_file(&["--tag", "okhttp", "-i"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 4);
}

// ═══════════════════════════════════════════════════════════════════════
// Expression filter
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_expr_basic() {
    let (out, _f) = run_with_file(&["-e", "tag ~ OkHttp and level >= W"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 2);
}

#[test]
fn test_cli_expr_or() {
    let (out, _f) = run_with_file(&["-e", "tag ~ DB or tag ~ Retrofit"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 4); // 2 DB + 2 Retrofit
}

#[test]
fn test_cli_multiple_expr_or() {
    // Multiple -e are OR'd
    let (out, _f) = run_with_file(&["-e", "tag ~ DB", "-e", "tag ~ Retrofit"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 4);
}

// ═══════════════════════════════════════════════════════════════════════
// Time range
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_since_until() {
    let (out, _f) = run_with_file(&["--since", "10:00:03", "--until", "10:00:06"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // Lines at 10:00:03, 10:00:04, 10:00:05, 10:00:06
    assert_eq!(stdout.lines().count(), 4);
}

#[test]
fn test_cli_since_only() {
    let (out, _f) = run_with_file(&["--since", "10:00:08"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // Lines at 10:00:08 and 10:00:09
    assert_eq!(stdout.lines().count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Histogram
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_histogram() {
    let (out, _f) = run_with_file(&["--histogram", "10s"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(stdout.contains("\"bucket\":\"04-02 10:00:00\""));
    assert!(stdout.contains("\"_stats\""));
}

// ═══════════════════════════════════════════════════════════════════════
// Fields selection
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_fields_json() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp", "--fields", "level,tag,msg", "--format", "json", "--limit", "1"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(stdout.contains("\"level\""));
    assert!(stdout.contains("\"tag\""));
    assert!(stdout.contains("\"msg\""));
    assert!(!stdout.contains("\"timestamp\""));
    assert!(!stdout.contains("\"pid\""));
}

// ═══════════════════════════════════════════════════════════════════════
// Crashes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_crashes() {
    let content = "\
04-02 10:00:00.000  1234  5678 D OkHttp  : normal
04-02 10:00:01.000  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main
04-02 10:00:02.000  1234  5678 E ActivityManager: ANR in com.app
04-02 10:00:03.000  1234  5678 F DEBUG   : signal 11 (SIGSEGV)
";
    let (out, _f) = run_with_file(&["--crashes"], content);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // Crashes output is JSON
    assert!(stdout.contains("\"type\":\"fatal_exception\""));
    assert!(stdout.contains("\"type\":\"anr\""));
    assert!(stdout.contains("\"type\":\"native_crash\""));
}

// ═══════════════════════════════════════════════════════════════════════
// Multiline
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_multiline() {
    let content = "\
04-02 10:00:00.000  1234  5678 E AndroidRuntime: FATAL EXCEPTION: main
    at com.app.Foo.bar(Foo.java:12)
    at com.app.Baz.qux(Baz.java:34)
04-02 10:00:01.000  1234  5678 D OkHttp  : normal line
";
    let (out, _f) = run_with_file(&["-M", "--tag", "AndroidRuntime"], content);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // Merged output should have the stack trace in one "line"
    assert!(stdout.contains("FATAL EXCEPTION"));
    assert!(stdout.contains("at com.app.Foo"));
}

// ═══════════════════════════════════════════════════════════════════════
// Tail / Sample
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_tail() {
    let (out, _f) = run_with_file(&["--tail", "2"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // Filter out header/separator lines (--- or "showing last")
    let lines: Vec<&str> = stdout.lines()
        .filter(|l| !l.is_empty() && !l.starts_with("---") && !l.contains("showing last") && !l.contains("matched entries"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("ANR in"));
}

#[test]
fn test_cli_sample() {
    let (out, _f) = run_with_file(&["--sample", "3"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    let content_lines: Vec<&str> = stdout.lines()
        .filter(|l| !l.is_empty() && !l.starts_with("---") && !l.contains("sampled") && !l.contains("matched entries"))
        .collect();
    assert_eq!(content_lines.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════════
// Context lines
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_context() {
    let (out, _f) = run_with_file(&["--tag", "DB", "--level", "E", "-C", "1"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // "connection lost" is at line index 6, context 1 means line 5 (before) and line 7 (after)
    assert!(stdout.contains("connection lost"));
    assert!(stdout.contains("Retrofit")); // line before
    assert!(stdout.contains("reconnecting")); // line after
}

#[test]
fn test_cli_before_after_context() {
    let (out, _f) = run_with_file(&["--msg", "FATAL", "-B", "1", "-A", "0"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert!(stdout.contains("FATAL EXCEPTION"));
    assert!(stdout.contains("reconnecting")); // 1 line before
}

// ═══════════════════════════════════════════════════════════════════════
// Time context
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_time_context() {
    let (out, _f) = run_with_file(&["--level", "F", "--time-context", "2s"], SAMPLE_LOG);
    // No F-level lines in our sample, so might not match anything
    // But the command should not crash
    assert!(out.status.code().unwrap() <= 1);
}

// ═══════════════════════════════════════════════════════════════════════
// Follow PID/TID
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_follow_pid() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp", "--level", "E", "--follow-pid"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // OkHttp E is pid=1234, so all lines with pid=1234 should be in output
    // pid=1234 lines: OkHttp D, OkHttp I, OkHttp W, OkHttp E, AndroidRuntime E = 5
    assert_eq!(stdout.lines().count(), 5);
}

#[test]
fn test_cli_follow_tid() {
    let (out, _f) = run_with_file(&["--tag", "DB", "--level", "E", "--follow-tid"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // DB E is tid=4444, all lines with tid=4444: DB E and DB W = 2
    assert_eq!(stdout.lines().count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Sort time
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_sort_time() {
    let content = "\
04-02 10:00:05.000  1234  5678 D Tag2    : later
04-02 10:00:01.000  1234  5678 D Tag1    : earlier
04-02 10:00:03.000  1234  5678 D Tag3    : middle
";
    let (out, _f) = run_with_file(&["--sort-time"], content);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].contains("earlier"));
    assert!(lines[1].contains("middle"));
    assert!(lines[2].contains("later"));
}

// ═══════════════════════════════════════════════════════════════════════
// Exit codes
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_exit_code_match() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp"], SAMPLE_LOG);
    assert_eq!(out.status.code(), Some(0));
}

#[test]
fn test_cli_exit_code_no_match() {
    let (out, _f) = run_with_file(&["--tag", "NonExistent"], SAMPLE_LOG);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn test_cli_exit_code_bad_args() {
    let mut cmd = Command::cargo_bin("aloggrep").unwrap();
    cmd.args(["--level", "INVALID_LEVEL", "-f", "/dev/null"]);
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(2));
}

// ═══════════════════════════════════════════════════════════════════════
// Stdin input
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_stdin_pipe() {
    let input = "04-02 10:00:00.000  1234  5678 E OkHttp  : error\n04-02 10:00:01.000  1234  5678 D Other   : debug\n";
    let out = run_with_stdin(&["--tag", "OkHttp"], input);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("OkHttp"));
}

#[test]
fn test_cli_stdin_no_filter_passthrough() {
    let input = "04-02 10:00:00.000  1234  5678 D Tag     : msg\nrandom unparseable line\n";
    let out = run_with_stdin(&[], input);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // Both lines pass through (no filter)
    assert_eq!(stdout.lines().count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// Glob file input
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_glob_multiple_files() {
    let dir = tempfile::tempdir().unwrap();
    let f1 = dir.path().join("a.log");
    let f2 = dir.path().join("b.log");
    std::fs::write(&f1, "04-02 10:00:00.000  1234  5678 E Tag     : err1\n").unwrap();
    std::fs::write(&f2, "04-02 10:00:01.000  1234  5678 E Tag     : err2\n").unwrap();

    let glob_pattern = dir.path().join("*.log").to_str().unwrap().to_string();
    let mut cmd = Command::cargo_bin("aloggrep").unwrap();
    cmd.args(["-f", &glob_pattern, "--count"]);
    let output = cmd.output().unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim(), "2");
}

// ═══════════════════════════════════════════════════════════════════════
// PID/TID filter
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_pid_filter() {
    let (out, _f) = run_with_file(&["--pid", "1234"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // pid=1234 lines: first 4 OkHttp + AndroidRuntime = 5
    assert_eq!(stdout.lines().count(), 5);
}

#[test]
fn test_cli_tid_filter() {
    let (out, _f) = run_with_file(&["--tid", "3333"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // tid=3333: Retrofit D, Retrofit I = 2
    assert_eq!(stdout.lines().count(), 2);
}

// ═══════════════════════════════════════════════════════════════════════
// AND logic
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_and_logic() {
    let (out, _f) = run_with_file(&["--msg", "timeout", "--msg", "30000", "--and"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("timeout after 30000ms"));
}

// ═══════════════════════════════════════════════════════════════════════
// No-color flag
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_no_color() {
    let (out, _f) = run_with_file(&["--tag", "OkHttp", "--no-color", "--limit", "1"], SAMPLE_LOG);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // No ANSI escape codes
    assert!(!stdout.contains("\x1b["));
}

// ═══════════════════════════════════════════════════════════════════════
// Hilog format support
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_hilog_package_filter() {
    let content = "\
04-16 11:52:56.297 11114 11114 I A00201/com.tencent.mqq/QRouter: msg1
04-16 11:52:57.000 11114 11114 I A00201/com.other.app/OtherTag: msg2
";
    let (out, _f) = run_with_file(&["--package", "com.tencent.mqq"], content);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("QRouter"));
}

// ═══════════════════════════════════════════════════════════════════════
// Xlog format support
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_xlog_format() {
    let content = "\
2026-03-04 10:23:28.872|1[3542]3831|3542|I|NTKernel|mobile_msf cmd
2026-03-04 10:23:29.000|1[3542]3831|3542|E|NTKernel|error occurred
2026-03-04 10:23:30.000|1[9999]3831|9999|D|Other|other msg
";
    let (out, _f) = run_with_file(&["--tag", "NTKernel", "--level", "E"], content);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("error occurred"));
}

// ═══════════════════════════════════════════════════════════════════════
// Edge: empty input
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_empty_input() {
    let out = run_with_stdin(&["--tag", "OkHttp"], "");
    assert_eq!(out.status.code(), Some(1)); // no match
}

// ═══════════════════════════════════════════════════════════════════════
// Edge: non-parseable lines with no filter (passthrough)
// ═══════════════════════════════════════════════════════════════════════

#[test]
fn test_cli_unparseable_lines_passthrough() {
    let input = "random text\nanother random line\n";
    let out = run_with_stdin(&[], input);
    let stdout = String::from_utf8(out.stdout).unwrap();
    // No filter → passthrough non-parseable lines
    assert_eq!(stdout.lines().count(), 2);
}

#[test]
fn test_cli_unparseable_lines_with_filter_skipped() {
    let input = "random text\n04-02 10:00:00.000  1234  5678 E Tag     : err\n";
    let out = run_with_stdin(&["--tag", "Tag"], input);
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(out.status.success());
    // Only parseable line matching filter is emitted
    assert_eq!(stdout.lines().count(), 1);
}
