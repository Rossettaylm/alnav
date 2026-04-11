pub mod crash;
pub mod dedupe;
pub mod expr;
pub mod filter;
pub mod formatter;
pub mod histogram;
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
    after_long_help = "\x1b[1mExamples:\x1b[0m

  \x1b[4mBasic filtering\x1b[0m
  adb logcat | aloggrep --tag OkHttp --level W
  aloggrep -f app.log --tag \"OkHttp|Retrofit\" --level E
  aloggrep -f app.log --msg error -i              # case-insensitive
  aloggrep -f app.log --tag Debug -v              # invert match
  aloggrep -f app.log --tag A --tag B             # tag=A OR tag=B
  aloggrep -f app.log --tag A --tag B --and       # tag=A AND tag=B

  \x1b[4mBoolean expressions (-e)\x1b[0m
  aloggrep -f app.log -e 'msg ~ timeout and level >= W'
  aloggrep -f app.log -e '(tag ~ OkHttp or tag ~ Retrofit) and level >= W'
  aloggrep -f app.log -e 'not tag ~ Debug'
  aloggrep -f app.log -e 'tag ~ OkHttp' -e 'tag ~ Retrofit'  # multiple -e = OR
  # Syntax: tag|msg|pkg ~ <regex>, level >= V|D|I|W|E|F
  # Combine with: and, or, not, ( )

  \x1b[4mContext lines\x1b[0m
  aloggrep -f app.log --tag crash -C 3            # 3 lines before + after
  aloggrep -f app.log -e 'level >= E' -B 5 -A 2  # 5 before, 2 after

  \x1b[4mMulti-line merge\x1b[0m
  aloggrep -f app.log --tag AndroidRuntime -M     # merge stack traces
  adb logcat | aloggrep -M --level E              # merged error entries

  \x1b[4mCrash extraction\x1b[0m
  aloggrep -f app.log --crashes                   # all crashes → JSON
  aloggrep -f app.log --crashes --tag MyApp       # filter + extract
  aloggrep -f app.log --crashes --limit 5         # first 5 crashes

  \x1b[4mSampling (manage large output)\x1b[0m
  aloggrep -f app.log --level E --tail 50           # last 50 errors
  aloggrep -f app.log --level E --limit 20 --tail 20 # first 20 + last 20
  aloggrep -f app.log --sample 100                  # uniform sample of 100

  \x1b[4mDeduplicate (group similar lines)\x1b[0m
  aloggrep -f app.log --level E --dedupe          # group errors by pattern
  aloggrep -f app.log --dedupe --limit 20         # top 20 patterns
  aloggrep -f app.log --dedupe --format json      # JSON output for AI
  # Numbers/hex/UUIDs are normalized: \"timeout 100ms\" ≈ \"timeout 200ms\"

  \x1b[4mOutput formats\x1b[0m
  aloggrep -f app.log --tag crash --format json --limit 50
  aloggrep -f app.log --format csv > out.csv
  aloggrep -f app.log --tag crash --count         # print match count only
  aloggrep -f app.log --summary                   # stats + top errors + crash count

  \x1b[4mTime range\x1b[0m
  aloggrep -f app.log --since 10:30:00 --until 10:35:00
  aloggrep -f app.log --since '2026-03-04 10:30:00' --until '2026-03-04 10:35:00'
  aloggrep -f app.log --since '04-02 12:00:00'      # threadtime date+time

  \x1b[4mPID/TID filtering\x1b[0m
  aloggrep -f app.log --tid 5678 --level W           # track specific thread
  aloggrep -f app.log --pid 1234 --tid 5678          # PID + TID combined
  aloggrep -f app.log -e 'pid ~ 3542 and level >= E' # expression with pid/tid

  \x1b[4mHistogram (time distribution)\x1b[0m
  aloggrep -f app.log --histogram 1m                 # level distribution per minute
  aloggrep -f app.log --histogram 10s --level E      # error count per 10 seconds

  \x1b[4mField selection\x1b[0m
  aloggrep -f app.log --level E --fields level,tag,msg --format json  # minimal output
  aloggrep -f app.log --fields timestamp,msg         # time + message only

  \x1b[4mTime-based context\x1b[0m
  aloggrep -f app.log --level F --time-context 5s    # all logs within 5s of fatal errors
  aloggrep -f app.log --tag crash --time-context 10s # 10s window around crash lines

  \x1b[4mMulti-file time sort\x1b[0m
  aloggrep -f 'logs/*.log' --sort-time --level E     # merge-sort by timestamp

  \x1b[4mFollow PID/TID\x1b[0m
  aloggrep -f app.log --tag OkHttp --level E --follow-pid  # all logs from error PIDs
  aloggrep -f app.log --crashes --follow-tid                # all logs from crashed threads"
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

    /// Boolean expression filter (repeatable, OR between multiple -e)
    #[arg(short = 'e', long = "expr", value_name = "EXPR")]
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
}
