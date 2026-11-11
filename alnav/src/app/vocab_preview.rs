//! Vocab candidate matching and throttled Filter/Exclude draft preview.

use super::App;

impl App {
    /// Filter/Exclude draft Preview with 50ms throttle (Candidate panel SLO).
    /// Fast typing reuses the last computed hits until the throttle window elapses.
    pub fn preview_filter_throttled(
        &self,
        input: &crate::input::InputBox,
        limit: usize,
    ) -> Vec<crate::preview::PreviewHit> {
        use std::time::{Duration, Instant};

        const PREVIEW_THROTTLE: Duration = Duration::from_millis(50);
        let key = {
            let chips = crate::preview::input_estimated_chips(input);
            let mut s = String::new();
            if input.exclude_mode {
                s.push('!');
            }
            for c in &chips {
                s.push_str(c.field.keyword());
                s.push('=');
                s.push_str(&c.value);
                s.push('\n');
            }
            s.push_str(&limit.to_string());
            s
        };
        let now = Instant::now();
        {
            let cache = self.preview_cache.borrow();
            if cache.key == key {
                return cache.lines.clone();
            }
            if let Some(t) = cache.at {
                if now.duration_since(t) < PREVIEW_THROTTLE {
                    return cache.lines.clone();
                }
            }
        }
        let lines = crate::preview::preview_filter_lines(self, input, limit);
        let mut cache = self.preview_cache.borrow_mut();
        cache.key = key;
        cache.lines = lines.clone();
        cache.at = Some(now);
        lines
    }

    /// Kick / refresh async vocab candidate matching from current picker draft.
    /// No-op when picker is closed or not on a vocab-backed New/Edit path.
    pub fn ensure_vocab_candidates(&mut self) {
        use crate::input::ChipField;
        use crate::picker::{PickerKind, PickerMode};
        use crate::vocab::CandidateScope;

        enum Action {
            Request {
                scope: CandidateScope,
                query: String,
            },
            Clear,
            Nop,
        }

        let action = {
            let Some(session) = self.picker.as_ref() else {
                return;
            };
            if !matches!(session.mode, PickerMode::New | PickerMode::Edit { .. }) {
                return;
            }
            match &session.kind {
                PickerKind::Highlight => {
                    if session.draft.is_empty() {
                        // Empty New lists no history; cancel any in-flight vocab job.
                        Action::Clear
                    } else {
                        Action::Request {
                            scope: CandidateScope::All,
                            query: session.draft.to_string(),
                        }
                    }
                }
                PickerKind::Filter | PickerKind::Exclude => {
                    let Some(input) = session.input.as_ref() else {
                        return;
                    };
                    match input.draft_field {
                        None => Action::Clear,
                        Some(ChipField::Tag) => Action::Request {
                            scope: CandidateScope::Tag,
                            query: input.draft.to_string(),
                        },
                        Some(ChipField::Pkg) => Action::Request {
                            scope: CandidateScope::Pkg,
                            query: input.draft.to_string(),
                        },
                        Some(ChipField::Msg) => Action::Request {
                            scope: CandidateScope::Msg,
                            query: input.draft.to_string(),
                        },
                        Some(ChipField::Level | ChipField::Pid | ChipField::Tid) => Action::Clear,
                    }
                }
                _ => Action::Nop,
            }
        };

        match action {
            Action::Request { scope, query } => {
                self.vocab_match.request(&self.vocab, scope, &query);
            }
            Action::Clear => {
                if self.vocab_match.pending() || !self.vocab_match.display_labels().is_empty() {
                    self.vocab_match.clear();
                }
            }
            Action::Nop => {}
        }
    }

    /// Drain finished vocab match jobs (call each frame).
    pub fn poll_vocab_match(&mut self) {
        self.vocab_match.poll();
    }

    /// Block until the in-flight vocab match completes (tests).
    #[cfg(test)]
    pub fn flush_vocab_match(&mut self) {
        self.ensure_vocab_candidates();
        self.vocab_match.flush(std::time::Duration::from_secs(5));
    }
}
