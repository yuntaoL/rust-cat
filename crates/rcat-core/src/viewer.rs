//! Viewer trait (the primary extension point for file type support).
//!
//! Phase 1: basic trait definition only. Real implementations come in later phases.

use std::io::Write;

use crate::dump::DumpOptions;
use crate::file_info::FileInfo;
use crate::probe::FileProbe;
use crate::view::{PositionKind, ViewAnchor, ViewContext, ViewportResult};

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

    /// How this viewer interprets [`ViewAnchor`] / scroll position.
    fn position_kind(&self) -> PositionKind {
        PositionKind::Byte
    }

    /// Largest meaningful anchor value for scroll-percent math (e.g. `size - 1` for bytes,
    /// `line_count - 1` for display-line viewers). Override for `DisplayLine` plugins.
    fn scroll_extent(&self, info: &FileInfo) -> u64 {
        match self.position_kind() {
            PositionKind::Byte | PositionKind::Frame => info.size.saturating_sub(1),
            PositionKind::DisplayLine => 1,
        }
    }

    /// Render a viewport for the interactive TUI (primary path).
    ///
    /// Built-in and plugin viewers should override this when possible. The default
    /// composes [`render_lines`](Self::render_lines) and [`status`](Self::status) for
    /// backward compatibility.
    fn render_viewport(&self, ctx: &ViewContext) -> ViewportResult {
        let anchor = ctx.anchor;
        let raw = ctx.anchor_raw();
        let lines = self.render_lines(ctx.info(), raw, ctx.max_rows, ctx.content_width);
        let status = self.status(ctx.info(), raw);
        ViewportResult {
            lines,
            status,
            anchor,
            source_byte: match anchor {
                ViewAnchor::Byte(b) => Some(b),
                _ => None,
            },
        }
    }

    /// Source file byte offset for the current anchor (for Text/Hex/JSON sync).
    fn source_byte_for_anchor(&self, info: &FileInfo, anchor: ViewAnchor) -> Option<u64> {
        match anchor {
            ViewAnchor::Byte(b) => Some(b.min(info.size.saturating_sub(1))),
            _ => None,
        }
    }

    /// Display-line anchor for a source byte (display-line viewers only).
    fn display_line_for_byte(&self, _info: &FileInfo, _byte: u64) -> Option<u64> {
        None
    }

    /// Advance scroll position by display rows according to this viewer's model.
    fn advance_anchor(&self, ctx: &ViewContext, delta: i64) -> ViewAnchor {
        let raw = self.advance_lines(ctx.info(), ctx.anchor_raw(), delta, ctx.content_width);
        ViewAnchor::from_raw(self.position_kind(), raw)
    }

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
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, b"x").unwrap();
        let session = crate::FileSession::open(f.path()).unwrap();

        let lines = viewer.render_lines(session.info(), 0, 10, 80);
        assert!(lines[0].contains("Dummy viewer"));

        let new_pos = viewer.advance_lines(session.info(), 10, 5, 80);
        assert_eq!(new_pos, 10 + 5 * 16); // default 16-byte steps

        let ctx = ViewContext::at_byte(&session, 0, 80, 10);
        let vp = viewer.render_viewport(&ctx);
        assert!(vp.lines[0].contains("Dummy viewer"));
        assert!(vp.status.contains("Dummy"));

        let ctx2 = ViewContext::at_byte(&session, 10, 80, 1);
        assert_eq!(viewer.advance_anchor(&ctx2, 5).raw(), 10 + 5 * 16);
    }
}
