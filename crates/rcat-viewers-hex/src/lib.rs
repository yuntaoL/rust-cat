//! Built-in HexViewer.
//!
//! Produces classic, correct hex + ASCII output (16 bytes per line with proper padding).
//! This is the default for binary files and is designed for high visual and data correctness.

use std::io::Write;

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
        // The core already did the heavy lifting with `infer`.
        // We can simply trust its preliminary classification for most cases.
        let prelim = probe.preliminary();

        match prelim.kind {
            rcat_core::file_info::ContentKind::Binary => ViewerPriority::Preferred,
            rcat_core::file_info::ContentKind::Empty => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Text => ViewerPriority::Low,
        }
    }

    fn dump(&self, info: &FileInfo, writer: &mut dyn Write, opts: &DumpOptions) -> std::io::Result<()> {
        // Delegate to the proven correct hex implementation in core.
        dump::dump_hex(info, writer, opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    fn write_temp(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
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
        assert_eq!(viewer.can_handle(&mut probe), ViewerPriority::Preferred);
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
}
