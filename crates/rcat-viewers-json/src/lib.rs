//! JSON viewer — raw file bytes with syntax highlighting (no parse/reformat).
//!
//! A file viewer must not reorder keys or regenerate content. We delegate line
//! rendering and scrolling to [`TextViewer`] (byte-anchored) and apply JSON
//! styling in the TUI by viewer name.

use std::io::Write;

use rcat_core::dump::{self, DumpOptions};
use rcat_core::file_info::FileInfo;
use rcat_core::probe::FileProbe;
use rcat_core::view::{PositionKind, ViewAnchor, ViewContext, ViewportResult};
use rcat_core::viewer::{FileViewer, ViewerPriority};
use rcat_viewers_text::TextViewer;

/// JSON viewer: same bytes as on disk as Text mode, JSON colors in the TUI.
pub struct JsonViewerLogic;

const INNER: TextViewer = TextViewer;

impl JsonViewerLogic {
    fn status_byte(info: &FileInfo, pos: u64) -> String {
        let pct = if info.size == 0 {
            100
        } else {
            ((pos as f64 / info.size as f64) * 100.0) as u32
        };
        format!("JSON  {} / {} B ({pct}%)", pos, info.size)
    }
}

impl FileViewer for JsonViewerLogic {
    fn name(&self) -> &'static str {
        "JSON"
    }

    /// Byte offset into the file — same coordinate system as Text and Hex.
    fn position_kind(&self) -> PositionKind {
        PositionKind::Byte
    }

    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority {
        let prelim = probe.preliminary();
        if prelim.extension.as_deref() == Some("json")
            || prelim.mime_type.as_deref() == Some("application/json")
        {
            ViewerPriority::Preferred
        } else {
            ViewerPriority::None
        }
    }

    fn render_viewport(&self, ctx: &ViewContext) -> ViewportResult {
        let anchor = ctx.anchor;
        let raw = ctx.anchor_raw();
        let lines = self.render_lines(ctx.info(), raw, ctx.max_rows, ctx.content_width);
        let status = self.status(ctx.info(), raw);
        ViewportResult {
            lines,
            status,
            anchor,
            source_byte: Some(raw.min(ctx.session.size().saturating_sub(1))),
        }
    }

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        width: u16,
    ) -> Vec<String> {
        INNER.render_lines(info, start_offset, max_rows, width)
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, width: u16) -> u64 {
        INNER.advance_lines(info, current, delta, width)
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        Self::status_byte(info, pos)
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        dump::dump_text(info, writer, opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Keys must appear in file order — never serde re-serialize pretty print.
    #[test]
    fn raw_view_preserves_key_order_on_first_line() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"z_first":1,"a_second":2}}"#).unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let logic = JsonViewerLogic;
        let lines = logic.render_lines(&info, 0, 5, 80);
        let joined = lines.join("\n");
        let z_pos = joined.find("z_first").expect("z_first");
        let a_pos = joined.find("a_second").expect("a_second");
        assert!(
            z_pos < a_pos,
            "file order must be preserved; got:\n{joined}"
        );
    }

    #[test]
    fn uses_byte_position_kind() {
        let logic = JsonViewerLogic;
        assert_eq!(logic.position_kind(), PositionKind::Byte);
    }

    #[test]
    fn advance_and_status_use_bytes() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "line0\nline1\nline2\n").unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let logic = JsonViewerLogic;
        let pos = logic.advance_lines(&info, 0, 1, 80);
        assert!(pos > 0);
        let status = logic.status(&info, pos);
        assert!(status.starts_with("JSON  "));
        assert!(status.contains('/'));
    }
}
