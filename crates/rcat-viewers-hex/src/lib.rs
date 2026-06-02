//! Built-in HexViewer.
//!
//! Produces classic, correct hex + ASCII output (16 bytes per line with proper padding).
//! Reads exclusively from host mmap slices (no per-frame file reopen).

use std::io::Write;
use std::sync::Arc;

use rcat_core::backing::{self, FileBacking};
use rcat_core::dump::{self, DumpOptions};
use rcat_core::file_info::FileInfo;
use rcat_core::probe::FileProbe;
use rcat_core::view::{ViewAnchor, ViewContext, ViewportResult};
use rcat_core::{FileViewer, ViewerPriority};
use tracing::trace;

fn hex_lines_from_bytes(data: &[u8], file_size: u64, start_offset: u64, max_rows: u16) -> Vec<String> {
    if file_size == 0 || data.is_empty() {
        return vec!["(empty file)".to_string()];
    }

    let start = start_offset.min(file_size);
    let bytes_to_read = (max_rows as u64 * 16).min(file_size.saturating_sub(start)) as usize;
    let buffer = if start as usize >= data.len() {
        &[][..]
    } else {
        let end = (start as usize + bytes_to_read).min(data.len());
        &data[start as usize..end]
    };

    let mut lines = Vec::new();
    for (i, chunk) in buffer.chunks(16).enumerate() {
        let addr = start + (i as u64 * 16);
        let hex_part: String = chunk
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        let ascii_part: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..=0x7e).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        lines.push(format!("{addr:08x}: {hex_part:<48} |{ascii_part}"));
    }

    if lines.is_empty() {
        lines.push("(end of file)".to_string());
    }

    lines
}

fn hex_status(size: u64, pos: u64) -> String {
    let pct = if size == 0 {
        100
    } else {
        ((pos as f64 / size as f64) * 100.0) as u32
    };
    format!("Hex  0x{pos:08X} · {pos} / {size} B ({pct}%)", size = size)
}

fn backing_bytes(info: &FileInfo) -> std::io::Result<Arc<FileBacking>> {
    backing::backing_for_info(info)
}

/// The built-in viewer for binary / hex data.
pub struct HexViewer;

impl Default for HexViewer {
    fn default() -> Self {
        Self
    }
}

impl FileViewer for HexViewer {
    fn name(&self) -> &'static str {
        "Hex"
    }

    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority {
        let prelim = probe.preliminary();
        let prio = match prelim.kind {
            rcat_core::file_info::ContentKind::Binary => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Empty => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Text => ViewerPriority::Low,
        };
        trace!(kind = ?prelim.kind, ?prio, "HexViewer::can_handle");
        prio
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        dump::dump_hex(info, writer, opts)
    }

    fn render_viewport(&self, ctx: &ViewContext) -> ViewportResult {
        let anchor = ctx.anchor;
        let start_offset = ctx.anchor_raw();
        trace!(
            start = start_offset,
            rows = ctx.max_rows,
            "HexViewer::render_viewport"
        );

        let size = ctx.session.size();
        let lines = hex_lines_from_bytes(ctx.session.bytes(), size, start_offset, ctx.max_rows);
        let status = hex_status(size, start_offset);

        ViewportResult {
            lines,
            status,
            anchor,
            source_byte: Some(start_offset.min(size.saturating_sub(1))),
        }
    }

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        _width: u16,
    ) -> Vec<String> {
        trace!(
            start = start_offset,
            rows = max_rows,
            "HexViewer::render_lines"
        );
        match backing_bytes(info) {
            Ok(b) => hex_lines_from_bytes(b.bytes(), b.size(), start_offset, max_rows),
            Err(_) => vec!["(error opening file)".to_string()],
        }
    }

    fn advance_anchor(&self, ctx: &ViewContext, delta: i64) -> ViewAnchor {
        let step: u64 = 16;
        let size = ctx.session.size();
        let current = ctx.anchor_raw();
        let raw = if delta >= 0 {
            current
                .saturating_add((delta as u64) * step)
                .min(size.saturating_sub(1))
        } else {
            current.saturating_sub(((-delta) as u64) * step)
        };
        ViewAnchor::Byte(raw)
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, _width: u16) -> u64 {
        trace!(current, delta, "HexViewer::advance_lines");
        let step: u64 = 16;
        if delta >= 0 {
            let next = current.saturating_add((delta as u64) * step);
            next.min(info.size.saturating_sub(1))
        } else {
            current.saturating_sub(((-delta) as u64) * step)
        }
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        trace!(pos, "HexViewer::status");
        hex_status(info.size, pos)
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
    fn hex_viewer_prefers_binary_files() {
        let f = write_temp(&[0u8, 1, 2, 0xff, 0x00]);
        let info = FileInfo::from_path(f.path()).unwrap();
        let prefix = rcat_core::probe::PrefixProbe::from_path(f.path()).unwrap();
        let mut probe = rcat_core::probe::FileProbeWithInfo::new(&info, prefix);

        let viewer = HexViewer;
        assert_eq!(viewer.can_handle(&mut probe), ViewerPriority::Normal);
    }

    #[test]
    fn hex_viewer_produces_correct_padded_output() {
        let data: Vec<u8> = (0u8..=16).collect();
        let f = write_temp(&data);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = HexViewer;
        let mut buf = Vec::new();
        viewer
            .dump(&info, &mut buf, &DumpOptions::default())
            .unwrap();

        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s.lines().count(), 2);
        assert!(s.contains("10"));
        assert!(s.contains("|."));
    }

    #[test]
    fn hex_render_viewport_uses_session_slice() {
        let data: Vec<u8> = (0u8..=100).collect();
        let f = write_temp(&data);
        let session = FileSession::open(f.path()).unwrap();
        let viewer = HexViewer;
        let ctx = ViewContext::at_byte(&session, 16, 80, 3);
        let vp = viewer.render_viewport(&ctx);
        assert_eq!(vp.lines.len(), 3);
        assert!(vp.lines[0].starts_with("00000010:"));
        assert!(vp.status.contains("Hex"));
    }

    #[test]
    fn hex_render_lines_uses_backing_once() {
        let data: Vec<u8> = (0u8..=100).collect();
        let f = write_temp(&data);
        let info = FileInfo::from_path(f.path()).unwrap();
        let session = FileSession::from_info(info).unwrap();

        let viewer = HexViewer;
        let lines = viewer.render_lines(session.info(), 0, 5, 80);
        assert_eq!(lines.len(), 5);

        let pos = viewer.advance_lines(session.info(), 0, 3, 80);
        assert_eq!(pos, 48);
    }
}