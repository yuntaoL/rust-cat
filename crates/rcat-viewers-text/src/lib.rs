//! Built-in TextViewer.
//!
//! Handles likely text files with correct, safe dumping (lossy UTF-8).
//! This implementation prioritizes **correctness** of the final output.

pub mod text_slice;

use std::io::Write;
use std::sync::Arc;

use rcat_core::backing::{self, FileBacking};
use rcat_core::dump::{self, DumpOptions};
use rcat_core::file_info::FileInfo;
use rcat_core::probe::FileProbe;
use rcat_core::view::{ViewAnchor, ViewContext, ViewportResult};
use rcat_core::{FileViewer, ViewerPriority};
use tracing::trace;

pub use text_slice::wrap_to_width;

/// The built-in viewer for human-readable text files.
pub struct TextViewer;

impl Default for TextViewer {
    fn default() -> Self {
        Self
    }
}

fn backing_bytes(info: &FileInfo) -> std::io::Result<Arc<FileBacking>> {
    backing::backing_for_info(info)
}

impl FileViewer for TextViewer {
    fn name(&self) -> &'static str {
        "Text"
    }

    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority {
        let prelim = probe.preliminary();
        let prio = match prelim.kind {
            rcat_core::file_info::ContentKind::Text => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Empty => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Binary => ViewerPriority::Low,
        };
        trace!(kind = ?prelim.kind, ?prio, "TextViewer::can_handle");
        prio
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        dump::dump_text(info, writer, opts)
    }

    fn render_viewport(&self, ctx: &ViewContext) -> ViewportResult {
        let anchor = ctx.anchor;
        let raw = ctx.anchor_raw();
        let data = ctx.session.bytes();
        let size = ctx.session.size();
        let lines = text_slice::render_display_rows(data, size, raw, ctx.max_rows, ctx.content_width);
        let status = text_slice::text_status(size, raw);
        ViewportResult {
            lines,
            status,
            anchor,
            source_byte: Some(raw.min(size.saturating_sub(1))),
        }
    }

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        width: u16,
    ) -> Vec<String> {
        trace!(
            start = start_offset,
            rows = max_rows,
            width,
            "TextViewer::render_lines"
        );
        match backing_bytes(info) {
            Ok(b) => {
                text_slice::render_display_rows(b.bytes(), b.size(), start_offset, max_rows, width)
            }
            Err(_) => vec!["(error opening file)".to_string()],
        }
    }

    fn advance_anchor(&self, ctx: &ViewContext, delta: i64) -> ViewAnchor {
        let raw = text_slice::advance_lines_bytes(
            ctx.session.bytes(),
            ctx.session.size(),
            ctx.anchor_raw(),
            delta,
            ctx.content_width,
        );
        ViewAnchor::from_raw(self.position_kind(), raw)
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, width: u16) -> u64 {
        trace!(current, delta, width, "TextViewer::advance_lines");
        match backing_bytes(info) {
            Ok(b) => text_slice::advance_lines_bytes(b.bytes(), b.size(), current, delta, width),
            Err(_) => current,
        }
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        trace!(pos, "TextViewer::status");
        text_slice::text_status(info.size, pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcat_core::session::FileSession;
    use rcat_core::view::ViewContext;
    use tempfile::NamedTempFile;

    fn write_temp(content: &[u8]) -> NamedTempFile {
        let f = NamedTempFile::new().unwrap();
        std::fs::write(f.path(), content).unwrap();
        f
    }

    #[test]
    fn render_lines_empty_file_shows_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, b"").unwrap();
        let info = FileInfo::from_path(&path).unwrap();
        let lines = TextViewer.render_lines(&info, 0, 5, 80);
        assert_eq!(lines, vec!["(empty file)".to_string()]);
    }

    #[test]
    fn render_viewport_uses_session_bytes_without_reopen() {
        let f = write_temp(b"line0\nline1\nline2\n");
        let session = FileSession::open(f.path()).unwrap();
        let ctx = ViewContext::at_byte(&session, 0, 80, 3);
        let vp = TextViewer.render_viewport(&ctx);
        assert!(vp.lines.iter().any(|l| l.contains("line0")));
        assert!(vp.status.starts_with("Text  "));
    }

    #[test]
    fn text_viewer_prefers_text_files() {
        let f = write_temp(b"hello world\n");
        let info = FileInfo::from_path(f.path()).unwrap();
        let prefix = rcat_core::probe::PrefixProbe::from_path(f.path()).unwrap();
        let mut probe = rcat_core::probe::FileProbeWithInfo::new(&info, prefix);

        let viewer = TextViewer;
        assert_eq!(viewer.can_handle(&mut probe), ViewerPriority::Normal);
    }

    #[test]
    fn text_viewer_dumps_with_lossy_utf8_correctly() {
        let data = b"hello\xff\xfe world\n";
        let f = write_temp(data);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = TextViewer;
        let mut buf = Vec::new();
        viewer
            .dump(&info, &mut buf, &DumpOptions::default())
            .unwrap();

        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("hello"));
        assert!(s.contains("world"));
    }

    #[test]
    fn wrap_to_width_basic() {
        assert_eq!(wrap_to_width("hello", 10), vec!["hello"]);
        assert_eq!(wrap_to_width("hello", 5), vec!["hello"]);
        assert_eq!(wrap_to_width("hello world", 5), vec!["hello", " worl", "d"]);
        assert_eq!(wrap_to_width("", 10), vec![""]);
    }

    #[test]
    fn render_lines_respects_width_and_wrapping() {
        let content = b"1234567890\nshort\nvery long line that should wrap when width is small";
        let f = write_temp(content);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = TextViewer;

        let lines = viewer.render_lines(&info, 0, 10, 80);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "1234567890");

        let lines = viewer.render_lines(&info, 0, 20, 3);
        assert!(lines.len() > 3);
        assert_eq!(lines[0].chars().count(), 3);
    }

    #[test]
    fn advance_lines_respects_width_for_wrapped_content() {
        let content = b"abcdefghij\n";
        let f = write_temp(content);
        let info = FileInfo::from_path(f.path()).unwrap();
        let viewer = TextViewer;

        let pos = viewer.advance_lines(&info, 0, 1, 4);
        assert_eq!(pos, 4);

        let pos = viewer.advance_lines(&info, 0, 2, 4);
        assert_eq!(pos, 8);
    }

    #[test]
    fn render_lines_starts_mid_wrapped_line() {
        let content = b"0123456789\n";
        let f = write_temp(content);
        let info = FileInfo::from_path(f.path()).unwrap();
        let viewer = TextViewer;

        let lines = viewer.render_lines(&info, 5, 5, 4);
        assert!(!lines.is_empty());
        assert_eq!(lines[0].chars().count(), 4);
    }

    #[test]
    fn advance_anchor_uses_session_slice() {
        let f = write_temp(b"aaaa\nbbbb\n");
        let session = FileSession::open(f.path()).unwrap();
        let ctx = ViewContext::at_byte(&session, 0, 4, 1);
        let next = TextViewer.advance_anchor(&ctx, 1);
        assert_eq!(next.raw(), 5);
    }
}