//! Built-in TextViewer.
//!
//! Handles likely text files with correct, safe dumping (lossy UTF-8).
//! This implementation prioritizes **correctness** of the final output.

use std::io::Write;

use rcat_core::dump::{self, DumpOptions};
use rcat_core::file_info::FileInfo;
use rcat_core::probe::FileProbe;
use rcat_core::{FileViewer, ViewerPriority};

/// The built-in viewer for human-readable text files.
pub struct TextViewer;

impl Default for TextViewer {
    fn default() -> Self {
        Self
    }
}

impl FileViewer for TextViewer {
    fn name(&self) -> &'static str {
        "Text"
    }

    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority {
        // IMPORTANT: The default TextViewer deliberately returns at most `Normal`.
        // See the documentation on `ViewerPriority` for the reasoning.
        // Specialized viewers (JsonViewer, MarkdownViewer, etc.) should return
        // `Preferred` when they have a strong match.
        let prelim = probe.preliminary();

        match prelim.kind {
            rcat_core::file_info::ContentKind::Text => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Empty => ViewerPriority::Normal,
            rcat_core::file_info::ContentKind::Binary => ViewerPriority::Low,
        }
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        // Delegate to the proven correct implementation in core.
        // This guarantees consistent, high-quality text output.
        dump::dump_text(info, writer, opts)
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
    fn text_viewer_prefers_text_files() {
        let f = write_temp(b"hello world\n");
        let info = FileInfo::from_path(f.path()).unwrap();
        let prefix = rcat_core::probe::PrefixProbe::from_path(f.path()).unwrap();
        let mut probe = rcat_core::probe::FileProbeWithInfo::new(&info, prefix);

        let viewer = TextViewer;
        // Default TextViewer is intentionally conservative (max Normal)
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
}
