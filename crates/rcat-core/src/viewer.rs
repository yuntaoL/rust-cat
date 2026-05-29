//! Viewer trait (the primary extension point for file type support).
//!
//! Phase 1: basic trait definition only. Real implementations come in later phases.

use std::io::Write;

use crate::dump::DumpOptions;
use crate::file_info::FileInfo;
use crate::probe::FileProbe;

/// How strongly a viewer wants to handle a particular file.
///
/// ## Priority Convention (Important for Extensibility)
///
/// - **Specialized viewers** (e.g. `JsonViewer`, `ElfViewer`, `PngViewer`) may return
///   `Preferred` when they have a strong, specific match.
/// - **Default/generic viewers** (`TextViewer` and `HexViewer`) should **never** return
///   higher than `Normal`. They are intended as fallbacks so that more specific
///   viewers can take priority when appropriate.
///
/// This convention prevents priority collisions when multiple viewers could
/// theoretically handle the same file (e.g. a JSON file is both "Text" and "JSON").
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum ViewerPriority {
    /// This viewer cannot handle the file at all.
    None,
    /// Weak interest (fallback).
    Low,
    /// Normal interest.
    Normal,
    /// This viewer is the best choice for the file.
    Preferred,
}

/// Core trait that all viewers (built-in and plugins) must implement.
///
/// This trait is the primary extension point. Viewers are responsible for
/// producing correct output (especially in non-interactive dump mode).
pub trait FileViewer: Send + Sync {
    /// Human-readable name of this viewer (e.g. "Text", "Hex", "ELF").
    fn name(&self) -> &'static str;

    /// Return how suitable this viewer is for the given file.
    ///
    /// The `probe` allows the viewer to read a limited amount of raw data
    /// (currently up to 16 KiB) with caching provided by the host.
    /// This enables plugins to perform deep format-specific detection
    /// without opening the file themselves.
    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority;

    /// Dump the file content to the given writer using this viewer's format.
    ///
    /// This is the key method for correctness in non-interactive / piped usage.
    /// Each viewer controls its exact output (text encoding handling, hex formatting, etc.).
    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()>;

    /// Render a viewport of the file as **display rows** for the TUI.
    ///
    /// `max_rows` is the maximum number of *visual* rows the viewer should return
    /// (after any wrapping the viewer chooses to do).
    ///
    /// `width` is the available width in terminal columns for the content area
    /// (the viewer should use this to decide wrapping / truncation so that
    /// the lines it returns are appropriate for the current viewport).
    ///
    /// The position (`start_offset`) meaning is private to the viewer.
    ///
    /// Default implementation returns a placeholder. Specialized viewers should override this.
    fn render_lines(
        &self,
        _info: &FileInfo,
        _start_offset: u64,
        _max_rows: u16,
        _width: u16,
    ) -> Vec<String> {
        vec![format!(
            "[{} viewer] render_lines not implemented",
            self.name()
        )]
    }

    // Future richer TUI integration (for very custom viewers):
    // fn render(&self, frame: &mut Frame, area: Rect, info: &FileInfo, offset: u64);
    // fn handle_input(&mut self, event: KeyEvent) -> Option<Action>;

    /// Advance (or retreat) the viewport position by a number of *display rows*
    /// according to this viewer's rendering (respecting the given `width` for wrapping).
    ///
    /// - Positive `delta` moves downward (toward the end of the view).
    /// - Negative `delta` moves upward (toward the beginning).
    ///
    /// `width` is the current content width in columns. Viewers that perform
    /// wrapping should use it to decide how many visual rows a logical line occupies
    /// when computing the new position.
    ///
    /// The meaning of the `u64` position remains private to the viewer.
    ///
    /// Default: crude 16-byte steps (ignores width).
    fn advance_lines(&self, _info: &FileInfo, current: u64, delta: i64, _width: u16) -> u64 {
        if delta >= 0 {
            current.saturating_add(delta as u64 * 16)
        } else {
            current.saturating_sub((-delta) as u64 * 16)
        }
    }

    /// Return a human-readable description of the current viewport position.
    /// Used by the TUI to build a unified status line across all viewers.
    ///
    /// Examples of good output:
    ///   "Text  0x1a40 / 68% (line ~42)"
    ///   "JSON  line 7/29 (24%)"
    ///   "Hex   0x00001000"
    ///
    /// The default just shows the raw viewer name and numeric position.
    fn status(&self, _info: &FileInfo, pos: u64) -> String {
        format!("{} @ {}", self.name(), pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_info::FileInfo;
    use std::path::PathBuf;

    // Minimal viewer that only implements name and can_handle
    struct DummyViewer;

    impl FileViewer for DummyViewer {
        fn name(&self) -> &'static str {
            "Dummy"
        }

        fn can_handle(&self, _probe: &mut dyn crate::probe::FileProbe) -> ViewerPriority {
            ViewerPriority::Normal
        }

        fn dump(
            &self,
            _info: &FileInfo,
            _writer: &mut dyn std::io::Write,
            _opts: &crate::dump::DumpOptions,
        ) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn default_render_lines_and_advance_are_used() {
        let viewer = DummyViewer;
        let info = FileInfo {
            path: PathBuf::from("/tmp/dummy"),
            absolute_path: None,
            size: 100,
            kind: crate::file_info::ContentKind::Text,
            type_description: "text".into(),
            extension: None,
            detected: crate::detection::PreliminaryDetection::default(),
        };

        let lines = viewer.render_lines(&info, 0, 10, 80);
        assert!(lines[0].contains("Dummy viewer"));

        let new_pos = viewer.advance_lines(&info, 10, 5, 80);
        assert_eq!(new_pos, 10 + 5 * 16); // default 16-byte steps
    }
}
