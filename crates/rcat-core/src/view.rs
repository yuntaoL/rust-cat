//! Viewport types shared by built-in viewers and external plugins.

use crate::file_info::FileInfo;
use crate::session::FileSession;

/// How a viewer interprets its scroll position (`anchor`).
///
/// Declared per viewer so the host can show consistent headers and, later,
/// translate anchors when switching viewers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionKind {
    /// Anchor is a byte offset into the file (hex, raw text).
    #[default]
    Byte,
    /// Anchor is a 0-based index into the viewer's *display* lines (pretty JSON, markdown).
    DisplayLine,
    /// Anchor is a frame index (future video plugins).
    Frame,
}

/// Opaque scroll position for the active viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewAnchor {
    Byte(u64),
    DisplayLine(u64),
    Frame(u64),
}

impl ViewAnchor {
    pub fn kind(&self) -> PositionKind {
        match self {
            ViewAnchor::Byte(_) => PositionKind::Byte,
            ViewAnchor::DisplayLine(_) => PositionKind::DisplayLine,
            ViewAnchor::Frame(_) => PositionKind::Frame,
        }
    }

    pub fn raw(&self) -> u64 {
        match self {
            ViewAnchor::Byte(v) | ViewAnchor::DisplayLine(v) | ViewAnchor::Frame(v) => *v,
        }
    }

    pub fn from_raw(kind: PositionKind, value: u64) -> Self {
        match kind {
            PositionKind::Byte => ViewAnchor::Byte(value),
            PositionKind::DisplayLine => ViewAnchor::DisplayLine(value),
            PositionKind::Frame => ViewAnchor::Frame(value),
        }
    }
}

/// Inputs for rendering one TUI viewport. All viewers use this type.
pub struct ViewContext<'a> {
    pub session: &'a FileSession,
    pub anchor: ViewAnchor,
    pub content_width: u16,
    pub max_rows: u16,
}

impl<'a> ViewContext<'a> {
    pub fn new(
        session: &'a FileSession,
        anchor: ViewAnchor,
        content_width: u16,
        max_rows: u16,
    ) -> Self {
        Self {
            session,
            anchor,
            content_width,
            max_rows,
        }
    }

    /// Convenience when the viewer uses byte offsets (hex, text).
    pub fn at_byte(
        session: &'a FileSession,
        offset: u64,
        content_width: u16,
        max_rows: u16,
    ) -> Self {
        Self::new(session, ViewAnchor::Byte(offset), content_width, max_rows)
    }

    pub fn info(&self) -> &FileInfo {
        self.session.info()
    }

    pub fn anchor_raw(&self) -> u64 {
        self.anchor.raw()
    }
}

/// Output of one viewport render (lines + status in a single call).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViewportResult {
    pub lines: Vec<String>,
    pub status: String,
    pub anchor: ViewAnchor,
}

impl ViewportResult {
    pub fn empty_placeholder(viewer_name: &str, anchor: ViewAnchor) -> Self {
        Self {
            lines: vec![format!(
                "[{viewer_name} viewer] render_viewport not implemented"
            )],
            status: format!("{viewer_name} @ {}", anchor.raw()),
            anchor,
        }
    }
}
