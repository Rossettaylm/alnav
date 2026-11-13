//! Unit tests for `App`, grouped by concern.

use super::*;
use crate::bookmark::{Bookmark, JumpResult};
use crate::filter_model::Group;
use crate::highlight_model::HighlightGroup;
use crate::model::{is_severe_row, EntryRow};
use crate::store::FileStore;
use std::time::Duration;

mod export_tests;
mod file_store_tests;
mod flash_tests;
mod focus_tests;
mod follow_tests;
mod highlight_tests;
mod severe_tests;
mod surface_tests;
mod tests;
mod vocab_tests;
mod yank_and_search_tests;
