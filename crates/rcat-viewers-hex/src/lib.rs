//! Built-in HexViewer.
//!
//! Produces classic, correct hex + ASCII output (16 bytes per line with proper padding).
//! This is the default for binary files and is designed for high visual and data correctness.

use std::io::{Seek, SeekFrom, Write};

use rcat_core::dump::{self, DumpOptions};
use rcat_core::file_info::FileInfo;
use rcat_core::probe::FileProbe;
use rcat_core::{FileViewer, ViewerPriority};

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
        // IMPORTANT: The default HexViewer deliberately returns at most `Normal`.
        // See the documentation on `ViewerPriority` for the reasoning.
        // Specialized binary viewers (ElfViewer, ImageViewer, ArchiveViewer, etc.)
        // should return `Preferred` when they have a strong match.
        let prelim = probe.preliminary();

        match prelim.kind {
            rcat_core::file_info::ContentKind::Binary => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Empty => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Text => ViewerPriority::Low,
        }
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        // Delegate to the proven correct hex implementation in core.
        dump::dump_hex(info, writer, opts)
    }

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        _width: u16,
    ) -> Vec<String> {
        use std::fs::File;
        use std::io::Read;

        if info.size == 0 {
            return vec!["(empty file)".to_string()];
        }

        let mut file = match File::open(&info.path) {
            Ok(f) => f,
            Err(_) => return vec!["(error opening file)".to_string()],
        };

        let start = start_offset.min(info.size);
        if file.seek(SeekFrom::Start(start)).is_err() {
            return vec!["(seek error)".to_string()];
        }

        let bytes_to_read = (max_rows as u64 * 16).min(info.size - start) as usize;
        let mut buffer = vec![0u8; bytes_to_read];
        let read = file.read(&mut buffer).unwrap_or(0);
        buffer.truncate(read);

        let mut lines = Vec::new();
        for (i, chunk) in buffer.chunks(16).enumerate() {
            let addr = start + (i as u64 * 16);

            let hex_part: String = chunk
                .iter()
                .map(|b| format!("{:02x}", b))
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

            lines.push(format!("{:08x}: {:<48} |{}", addr, hex_part, ascii_part));
        }

        if lines.is_empty() {
            lines.push("(end of file)".to_string());
        }

        lines
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, _width: u16) -> u64 {
        let step: u64 = 16;
        if delta >= 0 {
            let next = current.saturating_add((delta as u64) * step);
            next.min(info.size.saturating_sub(1))
        } else {
            current.saturating_sub(((-delta) as u64) * step)
        }
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        let pct = if info.size == 0 {
            100
        } else {
            ((pos as f64 / info.size as f64) * 100.0) as u32
        };
        format!("Hex  0x{:08x} / {}%", pos, pct)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // Default HexViewer is intentionally conservative (max Normal)
        assert_eq!(viewer.can_handle(&mut probe), ViewerPriority::Normal);
    }

    #[test]
    fn hex_viewer_produces_correct_padded_output() {
        // 17 bytes -> 2 lines, second line has proper padding
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
        // Last line should contain the final byte and have padding
        assert!(s.contains("10"));
        assert!(s.contains("|."));
    }

    #[test]
    fn hex_render_lines_and_advance_work() {
        let data: Vec<u8> = (0u8..=100).collect();
        let f = write_temp(&data);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = HexViewer;

        let lines = viewer.render_lines(&info, 0, 5, 80);
        assert_eq!(lines.len(), 5);

        let pos = viewer.advance_lines(&info, 0, 3, 80);
        assert_eq!(pos, 48); // 3 rows * 16 bytes
    }
}
