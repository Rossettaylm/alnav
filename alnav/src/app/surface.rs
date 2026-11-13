//! Which surface owns input and paints on top: one precedence order for routing and drawing.

use super::App;

/// Topmost interactive surface (below [`Dialog`]). Key routing and overlay
/// painting both `match` on [`App::surface`], so precedence is defined once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    OpenFile,
    StreamSource,
    Dashboard,
    Picker,
    TimePanel,
    HighlightBox,
    CommandPalette,
    Compare,
    Help,
    Summary,
    Hist,
    /// Row detail overlay; keys still route through Normal mode.
    Detail,
    /// Log list / strips with no overlay.
    Main,
}

/// Small confirm-style dialog layered above any [`Surface`] (e.g. rename over Picker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Confirm,
    PresetName,
}

impl App {
    pub fn surface(&self) -> Surface {
        if self.open_file_panel.is_some() {
            Surface::OpenFile
        } else if self.stream_source_panel.is_some() {
            Surface::StreamSource
        } else if self.dashboard.is_some() {
            Surface::Dashboard
        } else if self.picker.is_some() {
            Surface::Picker
        } else if self.time_panel.is_some() {
            Surface::TimePanel
        } else if self.highlight_box.editing {
            Surface::HighlightBox
        } else if self.command_palette.is_some() {
            Surface::CommandPalette
        } else if self.compare.is_some() {
            Surface::Compare
        } else if self.help_open {
            Surface::Help
        } else if self.summary_open() {
            Surface::Summary
        } else if self.hist_open() {
            Surface::Hist
        } else if self.detail_open() {
            Surface::Detail
        } else {
            Surface::Main
        }
    }

    pub fn dialog(&self) -> Option<Dialog> {
        if self.confirm.is_some() {
            Some(Dialog::Confirm)
        } else if self.preset_name.is_some() {
            Some(Dialog::PresetName)
        } else {
            None
        }
    }
}
