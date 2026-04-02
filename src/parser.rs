use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub enum Level {
    V,
    D,
    I,
    W,
    E,
    F,
}

impl Level {
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            'V' => Some(Self::V),
            'D' => Some(Self::D),
            'I' => Some(Self::I),
            'W' => Some(Self::W),
            'E' => Some(Self::E),
            'F' => Some(Self::F),
            _ => None,
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        let s = s.trim().to_uppercase();
        match s.as_str() {
            "V" | "VERBOSE" => Some(Self::V),
            "D" | "DEBUG" => Some(Self::D),
            "I" | "INFO" => Some(Self::I),
            "W" | "WARN" | "WARNING" => Some(Self::W),
            "E" | "ERROR" => Some(Self::E),
            "F" | "FATAL" => Some(Self::F),
            _ => None,
        }
    }

    pub fn as_char(self) -> char {
        match self {
            Self::V => 'V',
            Self::D => 'D',
            Self::I => 'I',
            Self::W => 'W',
            Self::E => 'E',
            Self::F => 'F',
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry<'a> {
    pub timestamp: &'a str,
    pub pid: &'a str,
    pub tid: &'a str,
    pub level: Level,
    pub tag: &'a str,
    pub msg: &'a str,
}

impl<'a> LogEntry<'a> {
    /// Parse a logcat line. Supports threadtime, xlog, and brief formats.
    pub fn parse(line: &'a str) -> Option<Self> {
        Self::parse_threadtime(line)
            .or_else(|| Self::parse_xlog(line))
            .or_else(|| Self::parse_brief(line))
    }

    /// threadtime: `MM-DD HH:MM:SS.mmm  PID  TID LEVEL TAG     : MSG`
    fn parse_threadtime(line: &'a str) -> Option<Self> {
        // Minimum: "01-01 00:00:00.000 0 0 V T: m"
        if line.len() < 28 {
            return None;
        }

        // Validate date prefix: MM-DD HH:MM:SS.mmm
        // Format: [0]M[1]M[2]-[3]D[4]D[5] [6]H[7]H[8]:[9]M[10]M[11]:[12]S[13]S[14].[15]m[16]m[17]m
        let bytes = line.as_bytes();
        if bytes[2] != b'-' || bytes[5] != b' ' || bytes[8] != b':' || bytes[11] != b':' || bytes[14] != b'.' {
            return None;
        }

        let timestamp = &line[..18];
        let rest = &line[18..];

        // Skip spaces, read PID
        let rest = rest.trim_start();
        let pid_end = rest.find(|c: char| !c.is_ascii_digit())?;
        if pid_end == 0 {
            return None;
        }
        let pid = &rest[..pid_end];
        let rest = rest[pid_end..].trim_start();

        // Read TID
        let tid_end = rest.find(|c: char| !c.is_ascii_digit())?;
        if tid_end == 0 {
            return None;
        }
        let tid = &rest[..tid_end];
        let rest = rest[tid_end..].trim_start();

        // Read level char
        let level_char = rest.chars().next()?;
        let level = Level::from_char(level_char)?;
        let rest = &rest[1..].trim_start();

        // TAG is everything before " : "
        let colon_pos = rest.find(": ")?;
        let tag = rest[..colon_pos].trim();
        let msg = &rest[colon_pos + 2..];

        Some(LogEntry { timestamp, pid, tid, level, tag, msg })
    }

    /// xlog: `YYYY-MM-DD HH:MM:SS.mmm|...|TID|LEVEL|TAG|MSG`
    /// Example: `2026-03-04 10:23:28.872|1[3542]3831|3542|I|NTKernel|[I] message`
    fn parse_xlog(line: &'a str) -> Option<Self> {
        // Must start with YYYY-MM-DD
        if line.len() < 30 {
            return None;
        }
        let bytes = line.as_bytes();
        if bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b' '
            || bytes[13] != b':' || bytes[16] != b':' || bytes[19] != b'.'
        {
            return None;
        }

        let mut parts = line.splitn(6, '|');
        let timestamp = parts.next()?.trim();
        let _pid_info = parts.next()?; // e.g. "1[3542]3831"
        let tid = parts.next()?;       // e.g. "3542"
        let level_str = parts.next()?; // e.g. "I"
        let tag = parts.next()?;       // e.g. "NTKernel"
        let msg = parts.next().unwrap_or("");

        let level = Level::from_char(level_str.chars().next()?)?;

        // Extract PID from pid_info: "1[3542]3831" → "3542"
        let pid = _pid_info
            .find('[')
            .and_then(|start| {
                _pid_info[start + 1..].find(']').map(|end| &_pid_info[start + 1..start + 1 + end])
            })
            .unwrap_or("");

        Some(LogEntry { timestamp, pid, tid, level, tag, msg })
    }

    /// brief: `V/TAG(PID): MSG` or `V/TAG( PID): MSG`
    fn parse_brief(line: &'a str) -> Option<Self> {
        let level_char = line.chars().next()?;
        let level = Level::from_char(level_char)?;

        if line.as_bytes().get(1) != Some(&b'/') {
            return None;
        }

        let rest = &line[2..];
        let paren_open = rest.find('(')?;
        let tag = rest[..paren_open].trim();

        let rest = &rest[paren_open + 1..];
        let paren_close = rest.find(')')?;
        let pid = rest[..paren_close].trim();

        let rest = &rest[paren_close + 1..];
        let msg = rest.strip_prefix(": ").unwrap_or(rest);

        Some(LogEntry {
            timestamp: "",
            pid,
            tid: "",
            level,
            tag,
            msg,
        })
    }

    /// Extract time portion (HH:MM:SS) from timestamp for time range filtering.
    pub fn time_hms(&self) -> Option<&str> {
        if self.timestamp.len() >= 19 && self.timestamp.as_bytes()[4] == b'-' {
            // xlog: "YYYY-MM-DD HH:MM:SS.mmm" → HH:MM:SS is [11..19]
            Some(&self.timestamp[11..19])
        } else if self.timestamp.len() >= 14 {
            // threadtime: "MM-DD HH:MM:SS.mmm" → HH:MM:SS is [6..14]
            Some(&self.timestamp[6..14])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threadtime() {
        let line = "04-02 12:34:56.789  1234  5678 W OkHttp  : Connection timeout after 30s";
        let entry = LogEntry::parse(line).unwrap();
        assert_eq!(entry.timestamp, "04-02 12:34:56.789");
        assert_eq!(entry.pid, "1234");
        assert_eq!(entry.tid, "5678");
        assert_eq!(entry.level, Level::W);
        assert_eq!(entry.tag, "OkHttp");
        assert_eq!(entry.msg, "Connection timeout after 30s");
    }

    #[test]
    fn test_brief() {
        let line = "W/OkHttp(1234): Connection timeout";
        let entry = LogEntry::parse(line).unwrap();
        assert_eq!(entry.level, Level::W);
        assert_eq!(entry.tag, "OkHttp");
        assert_eq!(entry.pid, "1234");
        assert_eq!(entry.msg, "Connection timeout");
    }

    #[test]
    fn test_xlog() {
        let line = "2026-03-04 10:23:28.872|1[3542]3831|3542|I|NTKernel|[I] mobile_msf_depend_proxy.cc(101)::SendMsfRequest cmd:test";
        let entry = LogEntry::parse(line).unwrap();
        assert_eq!(entry.timestamp, "2026-03-04 10:23:28.872");
        assert_eq!(entry.pid, "3542");
        assert_eq!(entry.tid, "3542");
        assert_eq!(entry.level, Level::I);
        assert_eq!(entry.tag, "NTKernel");
        assert!(entry.msg.contains("mobile_msf_depend_proxy"));
    }

    #[test]
    fn test_xlog_time_hms() {
        let line = "2026-03-04 10:23:28.872|1[3542]3831|3542|I|Tag|msg";
        let entry = LogEntry::parse(line).unwrap();
        assert_eq!(entry.time_hms(), Some("10:23:28"));
    }

    #[test]
    fn test_unparseable() {
        assert!(LogEntry::parse("just some random text").is_none());
        assert!(LogEntry::parse("").is_none());
    }

    #[test]
    fn test_level_ordering() {
        assert!(Level::V < Level::D);
        assert!(Level::D < Level::I);
        assert!(Level::W < Level::E);
        assert!(Level::E < Level::F);
    }

    #[test]
    fn test_time_hms() {
        let line = "04-02 12:34:56.789  1234  5678 D Tag     : msg";
        let entry = LogEntry::parse(line).unwrap();
        assert_eq!(entry.time_hms(), Some("12:34:56"));
    }
}
