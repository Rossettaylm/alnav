//! H10: export current TUI filter state as a one-line `alnav grep` CLI command.

use crate::theme;
use alnav::expr::Expr;

/// How the TUI session was started (mirrored into the exported command).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportSource {
    File(String),
    Hdc { device: Option<String> },
    Adb { device: Option<String> },
}

impl Default for ExportSource {
    fn default() -> Self {
        Self::File(String::new())
    }
}

impl ExportSource {
    pub fn is_file(&self) -> bool {
        matches!(self, Self::File(_))
    }

    pub fn is_live(&self) -> bool {
        matches!(self, Self::Hdc { .. } | Self::Adb { .. })
    }

    /// Status-bar / dashboard-aligned nerdfont for the bound source.
    pub fn status_glyph(&self) -> &'static str {
        match self {
            Self::File(_) => theme::GLYPH_SOURCE_FILE,
            Self::Hdc { .. } => theme::GLYPH_SOURCE_HDC,
            Self::Adb { .. } => theme::GLYPH_SOURCE_ADB,
        }
    }
}

/// Build `alnav grep <source> -i [-e '<predicate>']`.
///
/// `predicate` is [`crate::filter_model::compile_predicate`]'s output (enabled
/// Filter / Exclude / lock / time window; never Search). Its `Display` parses
/// back to the same expression, so the command selects the same parsed rows.
pub fn build_cli_command(source: &ExportSource, predicate: Option<&Expr>) -> String {
    let mut parts: Vec<String> = vec!["alnav".into(), "grep".into()];

    match source {
        ExportSource::File(path) => {
            parts.push("-f".into());
            parts.push(shell_quote(path));
        }
        ExportSource::Hdc { device } | ExportSource::Adb { device } => {
            parts.push(
                if matches!(source, ExportSource::Hdc { .. }) {
                    "--hdc"
                } else {
                    "--adb"
                }
                .into(),
            );
            if let Some(serial) = device.as_deref().filter(|s| !s.is_empty()) {
                parts.push("--device".into());
                parts.push(shell_quote(serial));
            }
        }
    }

    parts.push("-i".into());

    if let Some(expr) = predicate {
        parts.push("-e".into());
        parts.push(shell_quote(&expr.to_string()));
    }

    parts.join(" ")
}

/// POSIX-ish single-quote shell escaping.
pub fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".into();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_glyph_matches_dashboard_source_icons() {
        assert_eq!(
            ExportSource::File("app.log".into()).status_glyph(),
            theme::GLYPH_SOURCE_FILE
        );
        assert_eq!(
            ExportSource::Hdc { device: None }.status_glyph(),
            theme::GLYPH_SOURCE_HDC
        );
        assert_eq!(
            ExportSource::Adb { device: None }.status_glyph(),
            theme::GLYPH_SOURCE_ADB
        );
    }

    #[test]
    fn sources_and_empty_predicate() {
        let file = build_cli_command(&ExportSource::File("a b.log".into()), None);
        assert_eq!(file, "alnav grep -f 'a b.log' -i");
        let hdc = build_cli_command(
            &ExportSource::Hdc {
                device: Some("XYZ".into()),
            },
            None,
        );
        assert_eq!(hdc, "alnav grep --hdc --device 'XYZ' -i");
        let adb = build_cli_command(&ExportSource::Adb { device: None }, None);
        assert_eq!(adb, "alnav grep --adb -i");
    }

    #[test]
    fn predicate_with_single_quote_survives_shell_quoting() {
        let e = Expr::literal(alnav::expr::TextField::Msg, "can't", true);
        let cmd = build_cli_command(&ExportSource::File("f".into()), Some(&e));
        assert_eq!(cmd, r#"alnav grep -f 'f' -i -e 'msg ~ "can'\''t"'"#);
    }

    #[test]
    fn shell_quote_handles_embedded_single_quote() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }
}
