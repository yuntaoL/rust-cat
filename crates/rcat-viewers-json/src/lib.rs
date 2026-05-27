//! Built-in JsonViewer.
//!
//! Demonstrates a specialized viewer that claims `Preferred` priority for JSON files.
//! This validates that the FileViewer + ViewerRegistry + FileProbe design correctly
//! supports plugin-style extensibility (core does first-pass via infer, specialized
//! viewer refines and wins with higher priority).
//!
//! - can_handle returns Preferred for application/json or .json files
//! - dump: best-effort pretty-prints JSON (respects byte offset/length on raw input)
//! - render_lines: pretty-prints then pages by logical lines (start_offset treated as line hint)

use std::io::Write;

use rcat_core::dump::DumpOptions;
use rcat_core::file_info::FileInfo;
use rcat_core::probe::FileProbe;
use rcat_core::{FileViewer, ViewerPriority};
use serde_json::{Value, from_slice, to_string_pretty};

/// The specialized viewer for JSON files.
#[derive(Default)]
pub struct JsonViewer;

impl JsonViewer {
    /// Build the current pretty lines (called from render + advance).
    fn pretty_lines(&self, info: &FileInfo) -> Vec<String> {
        use std::fs::File;
        use std::io::Read;

        if info.size == 0 {
            return vec!["(empty file)".to_string()];
        }

        let mut file = match File::open(&info.path) {
            Ok(f) => f,
            Err(_) => return vec!["(error opening file)".to_string()],
        };

        let mut buf = Vec::new();
        if file.read_to_end(&mut buf).is_err() {
            return vec!["(read error)".to_string()];
        }

        let pretty = match from_slice::<Value>(&buf) {
            Ok(value) => to_string_pretty(&value)
                .unwrap_or_else(|_| String::from_utf8_lossy(&buf).into_owned()),
            Err(_) => {
                let mut s = String::from_utf8_lossy(&buf).into_owned();
                s.insert_str(0, "(not valid JSON — showing raw)\n");
                s
            }
        };

        pretty.lines().map(|l| l.to_string()).collect()
    }

    /// Returns the flat list of *display rows* after wrapping every logical pretty line to `width`.
    /// This is the source of truth for both rendering and per-visual-row scrolling.
    fn display_rows(&self, info: &FileInfo, width: u16) -> Vec<String> {
        let logical = self.pretty_lines(info);
        let mut rows = Vec::new();
        for line in logical {
            rows.extend(wrap_to_width(&line, width));
        }
        rows
    }
}

/// Simple width wrapper (monospace assumption). Duplicated here to keep crates independent.
fn wrap_to_width(s: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return vec![s.to_string()];
    }
    let w = width as usize;
    if s.chars().count() <= w {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cw = 0usize;
    for ch in s.chars() {
        if cw + 1 > w && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
            cw = 0;
        }
        cur.push(ch);
        cw += 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

impl FileViewer for JsonViewer {
    fn name(&self) -> &'static str {
        "JSON"
    }

    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority {
        // Specialized viewers return Preferred to take precedence over the generic TextViewer.
        // This is the key extensibility test: TextViewer returns Normal, we return Preferred
        // for JSON and win in find_best().
        let prelim = probe.preliminary();

        let is_json_by_prelim = prelim
            .mime_type
            .as_deref()
            .map(|m| m == "application/json")
            .unwrap_or(false)
            || prelim
                .extension
                .as_deref()
                .map(|e| e.eq_ignore_ascii_case("json"))
                .unwrap_or(false);

        if is_json_by_prelim {
            return ViewerPriority::Preferred;
        }

        // Deeper content sniff using the probe (real plugin capability test).
        // Many .json files have no magic bytes, so infer may not set mime/ext.
        // We read a small prefix and check for typical JSON start after optional whitespace.
        if let Ok(prefix) = probe.read_bytes(0, 512) {
            let s = String::from_utf8_lossy(prefix);
            let trimmed = s.trim_start_matches(|c: char| c.is_whitespace());
            if (trimmed.starts_with('{') || trimmed.starts_with('[')) && !trimmed.contains('\0') {
                // Looks like JSON array or object and no obvious binary garbage.
                // Return Preferred so we beat the generic TextViewer.
                return ViewerPriority::Preferred;
            }
        }

        ViewerPriority::None
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        use std::fs::File;
        use std::io::{Read, Seek};

        if info.size == 0 {
            return Ok(());
        }

        let mut file = File::open(&info.path)?;
        let start = opts.offset.min(info.size) as usize;
        let len = opts.length.unwrap_or(info.size - opts.offset) as usize;
        let end = (start + len).min(info.size as usize);

        let mut buf = vec![0u8; end - start];
        if start > 0 {
            file.seek(std::io::SeekFrom::Start(start as u64))?;
        }
        let n = file.read(&mut buf)?;
        buf.truncate(n);

        // Try to parse and pretty-print the slice
        match from_slice::<Value>(&buf) {
            Ok(value) => {
                let pretty = to_string_pretty(&value).unwrap_or_else(|_| {
                    // Fallback: treat as text
                    String::from_utf8_lossy(&buf).into_owned()
                });
                writer.write_all(pretty.as_bytes())?;
                if !pretty.ends_with('\n') {
                    writer.write_all(b"\n")?;
                }
            }
            Err(_) => {
                // Not valid JSON in the slice — dump raw with lossy
                let s = String::from_utf8_lossy(&buf);
                writer.write_all(s.as_bytes())?;
            }
        }

        Ok(())
    }

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        width: u16,
    ) -> Vec<String> {
        let rows = self.display_rows(info, width);
        if rows.is_empty() {
            return vec!["(empty JSON)".to_string()];
        }

        // start_offset is now interpreted as a *display row index* in the fully wrapped view.
        let start = (start_offset as usize).min(rows.len().saturating_sub(1));
        let end = (start + max_rows as usize).min(rows.len());

        let mut out = rows[start..end].to_vec();
        if out.is_empty() {
            out.push("(end of JSON)".to_string());
        }
        out
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, width: u16) -> u64 {
        let rows = self.display_rows(info, width);
        if rows.is_empty() {
            return 0;
        }

        // Position = index into the flat list of display rows.
        // This gives perfect "one visual row per keypress" behavior,
        // even when a single pretty line wraps into many display rows.
        let total = rows.len() as u64;
        let mut pos = current.min(total.saturating_sub(1));

        if delta > 0 {
            pos = pos.saturating_add(delta as u64);
        } else {
            pos = pos.saturating_sub((-delta) as u64);
        }
        pos.min(total.saturating_sub(1))
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        // When we have width we can give accurate display-row progress.
        // For status we recompute with a reasonable default width if needed.
        // Here we just report logical pretty lines for simplicity (good enough).
        let pretty = self.pretty_lines(info);
        let line_count = pretty.len();
        if line_count > 0 {
            let cur = (pos.min((line_count - 1) as u64) + 1) as usize;
            let pct = ((cur as f64 / line_count as f64) * 100.0) as u32;
            format!("JSON  line {}/{} ({}%)", cur, line_count, pct)
        } else {
            let pct = if info.size == 0 {
                100
            } else {
                ((pos as f64 / info.size as f64) * 100.0) as u32
            };
            format!("JSON  @{} / {}%", pos, pct)
        }
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
    fn json_viewer_claims_preferred_for_json() {
        let data = br#"{"hello": "world", "n": 42}"#;
        // Use .json suffix so FileInfo detects extension (and content sniff also works)
        let mut builder = tempfile::Builder::new();
        let f = builder.suffix(".json").tempfile().unwrap();
        std::fs::write(f.path(), data).unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let prefix = rcat_core::probe::PrefixProbe::from_path(f.path()).unwrap();
        let mut probe = rcat_core::probe::FileProbeWithInfo::new(&info, prefix);

        let viewer = JsonViewer;
        assert_eq!(viewer.can_handle(&mut probe), ViewerPriority::Preferred);
    }

    #[test]
    fn json_viewer_returns_none_for_non_json() {
        let f = write_temp(b"just plain text\nno json here");
        let info = FileInfo::from_path(f.path()).unwrap();
        let prefix = rcat_core::probe::PrefixProbe::from_path(f.path()).unwrap();
        let mut probe = rcat_core::probe::FileProbeWithInfo::new(&info, prefix);

        let viewer = JsonViewer;
        assert_eq!(viewer.can_handle(&mut probe), ViewerPriority::None);
    }

    #[test]
    fn json_viewer_pretty_prints_in_dump() {
        let data = br#"{"a":1,"b":[2,3]}"#;
        let f = write_temp(data);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = JsonViewer;
        let mut buf = Vec::new();
        viewer
            .dump(&info, &mut buf, &DumpOptions::default())
            .unwrap();

        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("{\n"));
        assert!(s.contains("  \"a\": 1"));
    }

    #[test]
    fn json_render_lines_and_advance_respect_width() {
        let data =
            br#"{"long": "this is a very long string that will wrap when the width is small"}"#;
        let f = write_temp(data);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = JsonViewer;

        // Wide width
        let lines_wide = viewer.render_lines(&info, 0, 20, 100);
        assert!(!lines_wide.is_empty());

        // Very narrow width → more display rows
        let lines_narrow = viewer.render_lines(&info, 0, 20, 10);
        assert!(lines_narrow.len() > lines_wide.len());

        // Advancing by display rows with narrow width
        let pos = viewer.advance_lines(&info, 0, 2, 10);
        assert!(pos > 0);
    }
}
