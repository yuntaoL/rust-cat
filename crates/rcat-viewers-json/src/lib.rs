//! JSON viewer logic and display-line ↔ source-byte mapping for cross-viewer sync.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use rcat_core::dump::DumpOptions;
use rcat_core::file_info::FileInfo;
use rcat_core::probe::FileProbe;
use rcat_core::view::{PositionKind, ViewAnchor, ViewContext, ViewportResult};
use rcat_core::viewer::{FileViewer, ViewerPriority};
use serde_json::{Value, from_slice, to_string_pretty};

/// External / built-in JSON viewer implementation.
pub struct JsonViewerLogic;

impl JsonViewerLogic {
    pub fn pretty_lines_from_path(&self, path: &Path) -> io::Result<Vec<String>> {
        let info = FileInfo::from_path(path)?;
        if info.size == 0 {
            return Ok(vec!["(empty file)".to_string()]);
        }

        let mut file = File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let pretty = match from_slice::<Value>(&buf) {
            Ok(value) => to_string_pretty(&value)
                .unwrap_or_else(|_| String::from_utf8_lossy(&buf).into_owned()),
            Err(_) => {
                let mut s = String::from_utf8_lossy(&buf).into_owned();
                s.insert_str(0, "(not valid JSON — showing raw)\n");
                s
            }
        };

        Ok(pretty.lines().map(|l| l.to_string()).collect())
    }

    /// Map each pretty-printed display line to a source byte offset (monotonic scan).
    pub fn line_byte_offsets(&self, path: &Path) -> io::Result<Vec<u64>> {
        let lines = self.pretty_lines_from_path(path)?;
        if lines.is_empty() {
            return Ok(vec![0]);
        }

        let mut file = File::open(path)?;
        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;

        let mut offsets = Vec::with_capacity(lines.len());
        let mut search_from = 0usize;
        for line in &lines {
            let mut off = find_display_line_in_source(&raw, line, search_from);
            if off < search_from {
                off = search_from;
            }
            offsets.push(off as u64);
            search_from = off.saturating_add(1).min(raw.len());
        }
        Ok(offsets)
    }

    pub fn byte_at_display_line(&self, path: &Path, line: u64) -> io::Result<u64> {
        let offsets = self.line_byte_offsets(path)?;
        let idx = (line as usize).min(offsets.len().saturating_sub(1));
        Ok(offsets[idx])
    }

    /// Display line whose source span contains `byte` (largest line start still <= byte).
    pub fn display_line_at_byte(&self, path: &Path, byte: u64) -> io::Result<u64> {
        let offsets = self.line_byte_offsets(path)?;
        if offsets.is_empty() {
            return Ok(0);
        }
        if byte < offsets[0] {
            return Ok(0);
        }
        let mut lo = 0usize;
        let mut hi = offsets.len() - 1;
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if offsets[mid] <= byte {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        Ok(lo as u64)
    }

    pub fn line_count(&self, path: &Path) -> io::Result<usize> {
        Ok(self.pretty_lines_from_path(path)?.len().max(1))
    }

    pub fn render_lines_at(
        &self,
        path: &Path,
        start_offset: u64,
        max_rows: u16,
    ) -> io::Result<Vec<String>> {
        let all = self.pretty_lines_from_path(path)?;
        let start = start_offset as usize;
        let lines: Vec<String> = all
            .iter()
            .skip(start)
            .take(max_rows as usize)
            .cloned()
            .collect();
        Ok(if lines.is_empty() {
            vec!["(end of file)".to_string()]
        } else {
            lines
        })
    }

    pub fn advance_lines_at(&self, path: &Path, current: u64, delta: i64) -> io::Result<u64> {
        let total = self.line_count(path)?;
        let max_pos = total.saturating_sub(1) as u64;
        let new_pos = (current as i64 + delta).max(0) as u64;
        Ok(new_pos.min(max_pos))
    }

    pub fn status_at(&self, path: &Path, position: u64) -> io::Result<String> {
        let total = self.line_count(path)?;
        let pct = if total == 0 {
            0
        } else {
            ((position as f64 / total as f64) * 100.0) as u32
        };
        Ok(format!(
            "JSON  line {}/{} ({pct}%)",
            position + 1,
            total.max(1)
        ))
    }
}

impl FileViewer for JsonViewerLogic {
    fn name(&self) -> &'static str {
        "JSON"
    }

    fn position_kind(&self) -> PositionKind {
        PositionKind::DisplayLine
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

    fn source_byte_for_anchor(&self, info: &FileInfo, anchor: ViewAnchor) -> Option<u64> {
        match anchor {
            ViewAnchor::Byte(b) => Some(b.min(info.size.saturating_sub(1))),
            ViewAnchor::DisplayLine(line) => self
                .byte_at_display_line(&info.path, line)
                .ok()
                .map(|b| b.min(info.size.saturating_sub(1))),
            ViewAnchor::Frame(_) => None,
        }
    }

    fn display_line_for_byte(&self, info: &FileInfo, byte: u64) -> Option<u64> {
        self.display_line_at_byte(&info.path, byte).ok()
    }

    fn render_viewport(&self, ctx: &ViewContext) -> ViewportResult {
        let anchor = ctx.anchor;
        let line = ctx.anchor_raw();
        let lines = self
            .render_lines_at(&ctx.session.path(), line, ctx.max_rows)
            .unwrap_or_else(|_| vec!["(json render error)".to_string()]);
        let status = self
            .status_at(&ctx.session.path(), line)
            .unwrap_or_else(|_| format!("JSON @ line {line}"));
        let source_byte = self.source_byte_for_anchor(ctx.info(), anchor);
        ViewportResult {
            lines,
            status,
            anchor,
            source_byte,
        }
    }

    fn dump(&self, info: &FileInfo, writer: &mut dyn Write, opts: &DumpOptions) -> io::Result<()> {
        let all = self.pretty_lines_from_path(&info.path)?;
        let start = opts.offset as usize;
        let end = match opts.length {
            Some(len) => (start + len as usize).min(all.len()),
            None => all.len(),
        };
        for line in &all[start..end] {
            writeln!(writer, "{line}")?;
        }
        Ok(())
    }
}

/// Token from a pretty-printed line used to locate the same content in the raw file.
pub fn extract_search_token(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Some(start) = trimmed.find('"') {
        let rest = &trimmed[start + 1..];
        if let Some(end) = rest.find('"') {
            return trimmed[start..start + 1 + end + 1].to_string();
        }
    }
    trimmed.chars().take(48).collect()
}

fn find_display_line_in_source(file: &[u8], display_line: &str, search_from: usize) -> usize {
    let token = extract_search_token(display_line);
    if token.len() < 2 {
        return search_from.min(file.len().saturating_sub(1));
    }
    let needle = token.as_bytes();
    if let Some(pos) = find_substring(&file[search_from..], needle) {
        return search_from + pos;
    }
    // Do not search the whole file again — that resets many lines to offset 0 (`{` at BOF).
    search_from.min(file.len().saturating_sub(1))
}

fn find_substring(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn maps_display_line_to_source_byte() {
        let mut f = NamedTempFile::new().unwrap();
        write!(
            f,
            r#"{{"alpha":1,"beta":2,"gamma":3,"delta":4,"epsilon":5}}"#
        )
        .unwrap();
        let logic = JsonViewerLogic;
        let lines = logic.pretty_lines_from_path(f.path()).unwrap();
        assert!(lines.len() > 3);
        let offsets = logic.line_byte_offsets(f.path()).unwrap();
        let byte_at_first = offsets[0];
        let byte_later = logic.byte_at_display_line(f.path(), 2).unwrap();
        assert!(byte_later >= byte_at_first);
        let raw = std::fs::read(f.path()).unwrap();
        assert!(byte_later < raw.len() as u64);
    }

    #[test]
    fn display_line_at_byte_start_is_zero() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"a":1,"b":2,"c":3}}"#).unwrap();
        let logic = JsonViewerLogic;
        let line = logic.display_line_at_byte(f.path(), 0).unwrap();
        assert_eq!(line, 0);
    }

    #[test]
    fn display_line_at_byte_mid_file_is_not_zero() {
        let mut f = NamedTempFile::new().unwrap();
        let mut obj = String::from("{");
        for i in 0..40 {
            if i > 0 {
                obj.push(',');
            }
            obj.push_str(&format!("\n  \"key{i:02}\": {i}"));
        }
        obj.push_str("\n}\n");
        write!(f, "{obj}").unwrap();
        let raw = std::fs::read(f.path()).unwrap();
        let mid_byte = (raw.len() / 2) as u64;

        let logic = JsonViewerLogic;
        let offsets = logic.line_byte_offsets(f.path()).unwrap();
        assert!(
            offsets.windows(2).any(|w| w[1] > w[0]),
            "offsets should advance: {:?}",
            &offsets[..offsets.len().min(10)]
        );

        let line = logic.display_line_at_byte(f.path(), mid_byte).unwrap();
        assert!(
            line > 0,
            "mid-file byte {mid_byte} should not map to display line 0 (offsets len {})",
            offsets.len()
        );
    }

    #[test]
    fn byte_round_trip_is_consistent() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"first":1,"second":2,"third":3,"fourth":4}}"#).unwrap();
        let logic = JsonViewerLogic;
        let target_line = 3u64;
        let byte = logic.byte_at_display_line(f.path(), target_line).unwrap();
        let back = logic.display_line_at_byte(f.path(), byte).unwrap();
        assert_eq!(back, target_line);
    }
}
