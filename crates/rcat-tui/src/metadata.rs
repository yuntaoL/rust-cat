//! Metadata sidebar content derived from `FileInfo` and file head bytes.

use ratatui::text::{Line, Span};
use rcat_core::file_info::{ContentKind, FileInfo};
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::theme::Theme;

const MAGIC_PREVIEW_LEN: usize = 16;

/// Human-readable file size (B, KiB, MiB, GiB).
pub fn format_file_size(size: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if size >= GIB {
        format!("{:.2} GiB ({size} B)", size as f64 / GIB as f64)
    } else if size >= MIB {
        format!("{:.2} MiB ({size} B)", size as f64 / MIB as f64)
    } else if size >= KIB {
        format!("{:.2} KiB ({size} B)", size as f64 / KIB as f64)
    } else {
        format!("{size} B")
    }
}

/// Scroll position as 0–100% through the file (by byte offset).
pub fn position_percent(offset: u64, size: u64) -> u8 {
    if size == 0 {
        return 0;
    }
    ((offset.saturating_mul(100)) / size).min(100) as u8
}

/// Scroll position as 0–100% within a viewer-specific extent (e.g. display lines).
pub fn position_percent_in_extent(anchor: u64, extent: u64) -> u8 {
    if extent == 0 {
        return 0;
    }
    ((anchor.saturating_mul(100)) / extent).min(100) as u8
}

/// Header position label for the active viewer.
pub fn format_header_position(
    kind: rcat_core::PositionKind,
    anchor_raw: u64,
    file_size: u64,
    scroll_extent: u64,
) -> String {
    match kind {
        rcat_core::PositionKind::Byte => {
            let pct = position_percent(anchor_raw, file_size);
            format!("byte {anchor_raw} / {file_size} B ({pct}%)")
        }
        rcat_core::PositionKind::DisplayLine => {
            let pct = position_percent_in_extent(anchor_raw, scroll_extent);
            format!("line {} ({pct}%)", anchor_raw + 1)
        }
        rcat_core::PositionKind::Frame => {
            let pct = position_percent_in_extent(anchor_raw, scroll_extent);
            format!("frame {} ({pct}%)", anchor_raw)
        }
    }
}

/// Read up to 16 bytes from the start of the file for the magic preview.
pub fn magic_hex_preview(path: &Path) -> String {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return "—".to_string(),
    };
    let mut buf = [0u8; MAGIC_PREVIEW_LEN];
    let n = file.read(&mut buf).unwrap_or(0);
    if n == 0 {
        return "(empty)".to_string();
    }
    buf[..n]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn kind_label(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Text => "Text",
        ContentKind::Binary => "Binary",
        ContentKind::Empty => "Empty",
    }
}

fn field_line(label: &str, value: impl AsRef<str>, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label}: "), theme.label),
        Span::styled(value.as_ref().to_string(), theme.value),
    ])
}

/// Build styled lines for the metadata sidebar.
pub fn build_metadata_lines(info: &FileInfo, theme: &Theme) -> Vec<Line<'static>> {
    let detected = &info.detected;
    let mut lines = vec![
        Line::from(Span::styled(" Metadata ", theme.sidebar_title)),
        Line::from(""),
        field_line("Size", format_file_size(info.size), theme),
        field_line("Kind", kind_label(info.kind), theme),
        field_line("Type", &info.type_description, theme),
    ];

    if let Some(ext) = &info.extension {
        lines.push(field_line("Ext", ext, theme));
    }
    if let Some(mime) = &detected.mime_type {
        lines.push(field_line("MIME", mime, theme));
    }
    if let Some(fmt) = &detected.format
        && fmt != &info.type_description
    {
        lines.push(field_line("Format", fmt, theme));
    }

    lines.push(Line::from(""));
    lines.push(field_line("Magic", magic_hex_preview(&info.path), theme));

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcat_core::detection::PreliminaryDetection;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn format_file_size_scales() {
        assert!(format_file_size(512).contains("512 B"));
        assert!(format_file_size(2048).contains("KiB"));
    }

    #[test]
    fn position_percent_edges() {
        assert_eq!(position_percent(0, 0), 0);
        assert_eq!(position_percent(0, 100), 0);
        assert_eq!(position_percent(50, 100), 50);
        assert_eq!(position_percent(100, 100), 100);
    }

    #[test]
    fn position_percent_in_extent_works() {
        assert_eq!(position_percent_in_extent(221, 438), 50);
    }

    #[test]
    fn header_position_display_line() {
        let s = format_header_position(rcat_core::PositionKind::DisplayLine, 220, 10_000, 438);
        assert!(s.contains("line 221"));
        assert!(s.contains("50%"));
    }

    #[test]
    fn magic_hex_preview_reads_bytes() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "\x7fELF").unwrap();
        let hex = magic_hex_preview(f.path());
        assert!(hex.starts_with("7f 45 4c 46"));
    }

    #[test]
    fn build_metadata_includes_mime_when_present() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "{{}}").unwrap();
        let info = FileInfo {
            path: f.path().to_path_buf(),
            absolute_path: None,
            size: 2,
            kind: ContentKind::Text,
            type_description: "JSON".into(),
            extension: Some("json".into()),
            detected: PreliminaryDetection {
                mime_type: Some("application/json".into()),
                extension: Some("json".into()),
                format: Some("JSON".into()),
                kind: ContentKind::Text,
            },
            backing: None,
        };
        let theme = Theme::default();
        let text: String = build_metadata_lines(&info, &theme)
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.clone()))
            .collect();
        assert!(text.contains("MIME"));
        assert!(text.contains("application/json"));
    }
}
