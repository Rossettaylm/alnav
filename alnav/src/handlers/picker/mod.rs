//! Unified / Filter / Exclude / Highlight / Preset picker: render data, keys, submit.

pub(super) mod keys;
pub(super) mod render_data;
pub(super) mod submit;

const LEVEL_CANDIDATES: &[&str] = &["V", "D", "I", "W", "E", "F"];

fn level_field_candidates(query: &str) -> Vec<String> {
    crate::fuzzy::fuzzy_str_labels(LEVEL_CANDIDATES, query)
}
