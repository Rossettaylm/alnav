//! Key strokes, bindings, and crossterm event normalization.

use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Normalized key stroke (letters stored lowercase; Shift via flag).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyStroke {
    pub code: StrokeCode,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StrokeCode {
    Char(char),
    Esc,
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
}

impl KeyStroke {
    pub fn char(c: char) -> Self {
        Self {
            code: StrokeCode::Char(c),
            ctrl: false,
            shift: false,
            alt: false,
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty key stroke".into());
        }
        let mut ctrl = false;
        let mut shift = false;
        let mut alt = false;
        let mut rest = s;
        loop {
            if let Some(r) = rest.strip_prefix("C-") {
                ctrl = true;
                rest = r;
                continue;
            }
            if let Some(r) = rest.strip_prefix("S-") {
                shift = true;
                rest = r;
                continue;
            }
            if let Some(r) = rest.strip_prefix("M-") {
                alt = true;
                rest = r;
                continue;
            }
            break;
        }
        if rest.is_empty() {
            return Err(format!("incomplete key stroke: {s}"));
        }
        let code = match rest {
            "Space" => StrokeCode::Char(' '),
            "Esc" | "Escape" => StrokeCode::Esc,
            "Enter" | "Return" => StrokeCode::Enter,
            "Tab" => StrokeCode::Tab,
            "BackTab" => StrokeCode::BackTab,
            "Backspace" | "BS" => StrokeCode::Backspace,
            "Delete" | "Del" => StrokeCode::Delete,
            "Up" => StrokeCode::Up,
            "Down" => StrokeCode::Down,
            "Left" => StrokeCode::Left,
            "Right" => StrokeCode::Right,
            "Home" => StrokeCode::Home,
            "End" => StrokeCode::End,
            _ => {
                let mut chars = rest.chars();
                let c = chars.next().ok_or_else(|| format!("bad key: {s}"))?;
                if chars.next().is_some() {
                    return Err(format!("unknown key name: {rest}"));
                }
                if c.is_ascii_uppercase() {
                    return Err(format!(
                        "use S-{} for Shift+letter (bare '{c}' is invalid)",
                        c.to_ascii_lowercase()
                    ));
                }
                StrokeCode::Char(c)
            }
        };
        Ok(Self {
            code,
            ctrl,
            shift,
            alt,
        })
    }

    pub fn format(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("C-");
        }
        if self.alt {
            out.push_str("M-");
        }
        if self.shift {
            out.push_str("S-");
        }
        match self.code {
            StrokeCode::Char(' ') => out.push_str("Space"),
            StrokeCode::Char(c) => out.push(c),
            StrokeCode::Esc => out.push_str("Esc"),
            StrokeCode::Enter => out.push_str("Enter"),
            StrokeCode::Tab => out.push_str("Tab"),
            StrokeCode::BackTab => out.push_str("BackTab"),
            StrokeCode::Backspace => out.push_str("Backspace"),
            StrokeCode::Delete => out.push_str("Delete"),
            StrokeCode::Up => out.push_str("Up"),
            StrokeCode::Down => out.push_str("Down"),
            StrokeCode::Left => out.push_str("Left"),
            StrokeCode::Right => out.push_str("Right"),
            StrokeCode::Home => out.push_str("Home"),
            StrokeCode::End => out.push_str("End"),
        }
        out
    }

    /// Compact UI form: Shift+letter → `J`, otherwise same as [`Self::format`].
    pub fn format_ui(&self) -> String {
        if self.shift && !self.ctrl && !self.alt {
            if let StrokeCode::Char(c) = self.code {
                if c.is_ascii_lowercase() {
                    return c.to_ascii_uppercase().to_string();
                }
            }
        }
        self.format()
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

/// One or more strokes forming a binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Binding {
    pub strokes: Vec<KeyStroke>,
}

impl Binding {
    pub fn parse_str(s: &str) -> Result<Self, String> {
        Ok(Self {
            strokes: vec![KeyStroke::parse(s)?],
        })
    }

    /// No default key. `--init` serializes as `""` (unbind).
    pub fn unbound() -> Self {
        Self {
            strokes: Vec::new(),
        }
    }

    pub fn format(&self) -> String {
        self.strokes
            .iter()
            .map(KeyStroke::format)
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Compact chord for L1 (e.g. `ma`, `mm`).
    pub fn format_compact(&self) -> String {
        if self.strokes.len() == 1 {
            return self.strokes[0].format_ui();
        }
        self.strokes
            .iter()
            .map(|s| match s.code {
                StrokeCode::Char(' ') => "Space".to_string(),
                StrokeCode::Char(c) if !s.ctrl && !s.alt && !s.shift => c.to_string(),
                _ => s.format_ui(),
            })
            .collect()
    }
}

impl fmt::Display for Binding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format())
    }
}

/// Convert a bare [`KeyCode`] (Normal-mode path) into a stroke.
pub fn stroke_from_keycode(code: KeyCode) -> Option<KeyStroke> {
    match code {
        KeyCode::Char(c) if c.is_ascii_uppercase() => Some(KeyStroke {
            code: StrokeCode::Char(c.to_ascii_lowercase()),
            ctrl: false,
            shift: true,
            alt: false,
        }),
        KeyCode::Char(c) => Some(KeyStroke::char(c)),
        KeyCode::Esc => Some(KeyStroke {
            code: StrokeCode::Esc,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Enter => Some(KeyStroke {
            code: StrokeCode::Enter,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Tab => Some(KeyStroke {
            code: StrokeCode::Tab,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::BackTab => Some(KeyStroke {
            code: StrokeCode::BackTab,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Backspace => Some(KeyStroke {
            code: StrokeCode::Backspace,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Delete => Some(KeyStroke {
            code: StrokeCode::Delete,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Up => Some(KeyStroke {
            code: StrokeCode::Up,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Down => Some(KeyStroke {
            code: StrokeCode::Down,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Left => Some(KeyStroke {
            code: StrokeCode::Left,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Right => Some(KeyStroke {
            code: StrokeCode::Right,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::Home => Some(KeyStroke {
            code: StrokeCode::Home,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        KeyCode::End => Some(KeyStroke {
            code: StrokeCode::End,
            ctrl: false,
            shift: false,
            alt: false,
        }),
        _ => None,
    }
}

/// Convert a full [`KeyEvent`] into a stroke.
pub fn stroke_from_event(key: KeyEvent) -> Option<KeyStroke> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift_mod = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Char(c) => {
            let (ch, shift) = if c.is_ascii_uppercase() {
                (c.to_ascii_lowercase(), true)
            } else {
                (c, shift_mod)
            };
            Some(KeyStroke {
                code: StrokeCode::Char(ch),
                ctrl,
                shift,
                alt,
            })
        }
        other => {
            let mut s = stroke_from_keycode(other)?;
            s.ctrl = ctrl;
            s.alt = alt;
            if shift_mod {
                s.shift = true;
            }
            Some(s)
        }
    }
}
