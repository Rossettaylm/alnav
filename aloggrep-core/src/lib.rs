pub mod clearkey;
pub mod crash;
pub mod dedupe;
pub mod expr;
pub mod filter;
pub mod formatter;
pub mod hdc;
pub mod histogram;
pub mod logcolor;
pub mod multiline;
pub mod parser;
pub mod sampler;
pub mod summary;

use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
    Csv,
}

#[derive(Parser)]
#[command(
    name = "aloggrep",
    about = "Lightweight Android logcat filter & analyzer",
    version
)]
pub struct Cli {
    /// Filter by tag (regex, repeatable, OR logic within)
    #[arg(short, long, value_name = "REGEX")]
    pub tag: Vec<String>,

    /// Filter by message content (regex, repeatable, OR logic within)
    #[arg(short, long, value_name = "REGEX")]
    pub msg: Vec<String>,

    /// Minimum log level: V, D, I, W, E, F
    #[arg(short, long, value_name = "LEVEL")]
    pub level: Option<String>,

    /// Filter by package name (repeatable, OR logic within)
    #[arg(short, long, value_name = "NAME")]
    pub package: Vec<String>,

    /// Read from log file(s) instead of stdin (supports glob)
    #[arg(short, long, value_name = "PATH")]
    pub file: Vec<String>,

    /// Output format
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Max lines to output (0 = unlimited)
    #[arg(long, default_value_t = 0)]
    pub limit: usize,

    /// Only print match count
    #[arg(long)]
    pub count: bool,

    /// Print aggregated summary (JSON)
    #[arg(long)]
    pub summary: bool,

    /// Start time filter (HH:MM:SS or YYYY-MM-DD HH:MM:SS or MM-DD HH:MM:SS)
    #[arg(long, value_name = "TIME")]
    pub since: Option<String>,

    /// End time filter (HH:MM:SS or YYYY-MM-DD HH:MM:SS or MM-DD HH:MM:SS)
    #[arg(long, value_name = "TIME")]
    pub until: Option<String>,

    /// Disable colored output
    #[arg(long)]
    pub no_color: bool,

    /// Case-insensitive matching
    #[arg(short = 'i', long)]
    pub ignore_case: bool,

    /// Invert match (exclude matching lines)
    #[arg(short = 'v', long)]
    pub invert: bool,

    /// Use AND logic for same-type filters (default is OR)
    #[arg(long)]
    pub and: bool,

    /// Boolean expression filter (repeatable, OR between multiple -e).
    /// Syntax: tag|msg|pkg ~ REGEX, level >= V|D|I|W|E|F. Combine with and/or/not/( ).
    /// e.g. -e 'msg ~ timeout and level >= W'
    #[arg(short = 'e', long = "expr", value_name = "EXPR", long_help = "Boolean expression filter (repeatable, OR between multiple -e).\nSyntax: tag|msg|pkg ~ REGEX, level >= V|D|I|W|E|F. Combine with and/or/not/( ).\ne.g. -e 'msg ~ timeout and level >= W'")]
    pub expr: Vec<String>,

    /// Show NUM lines of context around each match
    #[arg(short = 'C', long = "context", value_name = "NUM")]
    pub context: Option<usize>,

    /// Show NUM lines after each match
    #[arg(short = 'A', long = "after-context", value_name = "NUM")]
    pub after_context: Option<usize>,

    /// Show NUM lines before each match
    #[arg(short = 'B', long = "before-context", value_name = "NUM")]
    pub before_context: Option<usize>,

    /// Deduplicate: group similar lines, show count + time range
    #[arg(long)]
    pub dedupe: bool,

    /// Merge multi-line entries (e.g. stack traces) into one logical entry
    #[arg(short = 'M', long)]
    pub multiline: bool,

    /// Extract crashes as structured JSON (implies --multiline)
    #[arg(long)]
    pub crashes: bool,

    /// Show last N matched entries (combine with --limit for head+tail)
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub tail: usize,

    /// Uniformly sample N entries from all matches (reservoir sampling)
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub sample: usize,

    /// Filter by PID (regex, repeatable, OR logic within)
    #[arg(long, value_name = "REGEX")]
    pub pid: Vec<String>,

    /// Filter by TID (regex, repeatable, OR logic within)
    #[arg(long, value_name = "REGEX")]
    pub tid: Vec<String>,

    /// Time bucket histogram: group entries by interval (e.g. 10s, 1m, 5m)
    #[arg(long, value_name = "INTERVAL")]
    pub histogram: Option<String>,

    /// Select output fields: timestamp,pid,tid,level,tag,msg (comma-separated)
    #[arg(long, value_name = "FIELDS")]
    pub fields: Option<String>,

    /// Sort entries by timestamp across multiple files
    #[arg(long)]
    pub sort_time: bool,

    /// Show context by time window (e.g. 5s, 10s) instead of line count
    #[arg(long, value_name = "DURATION")]
    pub time_context: Option<String>,

    /// Follow matched PIDs: show all log entries from processes that match the filter
    #[arg(long)]
    pub follow_pid: bool,

    /// Follow matched TIDs: show all log entries from threads that match the filter
    #[arg(long)]
    pub follow_tid: bool,

    /// Highlight matching words in output (regex, case-insensitive, repeatable, each with different color)
    #[arg(long, value_name = "REGEX")]
    pub highlight: Vec<String>,

    /// Show usage examples
    #[arg(long)]
    pub example: bool,

    /// Capture logs directly from hdc hilog (HarmonyOS device)
    #[arg(long)]
    pub hdc: bool,

    /// Device serial number (for --hdc with multiple devices)
    #[arg(long, value_name = "SERIAL")]
    pub device: Option<String>,
}
