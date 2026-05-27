//! Built-in TextViewer.
//!
//! Handles likely text files with correct, safe dumping (lossy UTF-8).
//! This implementation prioritizes **correctness** of the final output.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};

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

/// Snap byte offset to the beginning of the containing logical line by scanning
/// backward for '\n' (limited distance for safety on huge files / giant lines).
fn snap_text_line_start(file: &mut std::fs::File, offset: u64) -> u64 {
    if offset == 0 {
        return 0;
    }
    let back = std::cmp::min(8192u64, offset);
    if file.seek(SeekFrom::Start(offset - back)).is_err() {
        return offset;
    }
    let mut buf = vec![0u8; back as usize];
    let n = file.read(&mut buf).unwrap_or(0);
    if n == 0 {
        return offset;
    }
    if let Some(pos) = buf[..n].iter().rposition(|&b| b == b'\n') {
        let candidate = (offset - back) + (pos as u64) + 1;
        if candidate <= offset {
            return candidate;
        }
    }
    if offset <= back { 0 } else { offset }
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

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        width: u16,
    ) -> Vec<String> {
        use std::fs::File;

        if info.size == 0 {
            return vec!["(empty file)".to_string()];
        }

        let mut file = match File::open(&info.path) {
            Ok(f) => f,
            Err(_) => return vec!["(error opening file)".to_string()],
        };

        // Snap to the start of a logical line so scrolling feels line-based
        // even when start_offset lands in the middle of a (possibly wrapped) line.
        let start = snap_text_line_start(&mut file, start_offset.min(info.size));

        if file.seek(SeekFrom::Start(start)).is_err() {
            return vec!["(seek error)".to_string()];
        }

        // === Width-aware render that supports starting mid-wrap ===
        // The start_offset may point inside a long logical line. We must begin
        // emitting from the correct *visual row* inside that line.
        let mut display_rows: Vec<String> = Vec::new();
        let mut current = start_offset.min(info.size);

        // Find the logical line containing 'current'
        let mut line_start = self.snap_to_line_start_in_file(&info.path, current);

        while display_rows.len() < max_rows as usize {
            let (line_content, line_len_incl_nl) = self.read_logical_line(&info.path, line_start);
            if line_len_incl_nl == 0 {
                break;
            }

            let wrapped = wrap_to_width(&line_content, width);

            // If we are starting inside this line, figure out which visual row to begin from
            let start_row_in_this_line = if current > line_start {
                let bytes_into_line = current.saturating_sub(line_start);
                self.visual_row_index_for_byte(&line_content, bytes_into_line, width)
            } else {
                0
            };

            for (i, row) in wrapped.into_iter().enumerate() {
                if i < start_row_in_this_line {
                    continue;
                }
                if display_rows.len() >= max_rows as usize {
                    break;
                }
                display_rows.push(row);
            }

            // Move to next logical line
            line_start += line_len_incl_nl;

            // If we went past the original current, reset so next lines start from row 0
            if current < line_start {
                current = line_start;
            }

            if line_start >= info.size {
                break;
            }
        }

        if display_rows.is_empty() {
            display_rows.push("(end of file)".to_string());
        }

        display_rows.truncate(max_rows as usize);
        display_rows
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, width: u16) -> u64 {
        if delta == 0 {
            return current;
        }

        let mut pos = current.min(info.size);
        let steps = delta.unsigned_abs() as usize;
        let forward = delta > 0;

        for _ in 0..steps {
            if forward {
                pos = self.advance_one_visual_row(&info.path, pos, width, info.size);
            } else {
                pos = self.retreat_one_visual_row(&info.path, pos, width);
            }
            if pos == 0 && !forward {
                break;
            }
            if pos >= info.size && forward {
                break;
            }
        }
        pos.min(info.size)
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        let pct = if info.size == 0 {
            100
        } else {
            ((pos as f64 / info.size as f64) * 100.0) as u32
        };
        format!("Text  0x{:08x} / {}%", pos, pct)
    }
}

impl TextViewer {
    #[allow(dead_code)]
    /// Advance forward by N raw logical lines (used by advance_lines).
    fn advance_text_lines_forward(
        &self,
        path: &std::path::Path,
        current: u64,
        lines: u64,
        file_size: u64,
    ) -> u64 {
        if lines == 0 {
            return current;
        }
        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return current,
        };
        let start = snap_text_line_start(&mut file, current.min(file_size));
        if file.seek(SeekFrom::Start(start)).is_err() {
            return start;
        }
        let mut reader = BufReader::new(file);
        let mut pos = start;
        let mut remaining = lines;
        let mut buf = String::new();
        while remaining > 0 {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    pos += n as u64;
                    remaining -= 1;
                }
                Err(_) => break,
            }
        }
        pos.min(file_size)
    }

    /// Retreat by N raw logical lines (used by advance_lines). Simple and robust.
    #[allow(dead_code)]
    fn retreat_text_lines_backward(&self, path: &std::path::Path, current: u64, lines: u64) -> u64 {
        if lines == 0 {
            return current;
        }
        let mut pos = current;

        for _ in 0..lines {
            if pos == 0 {
                break;
            }
            pos = self.previous_line_start(path, pos);
        }
        pos
    }

    /// Return the byte offset of the start of the logical line immediately before `from` (exclusive).
    /// Returns 0 if we are already on the first line.
    fn previous_line_start(&self, path: &std::path::Path, from: u64) -> u64 {
        if from == 0 {
            return 0;
        }
        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return 0,
        };

        // Search backward in chunks until we find a '\n' before `from`
        let mut search = from;
        loop {
            let back = std::cmp::min(8192u64, search);
            let start = search - back;
            if file.seek(SeekFrom::Start(start)).is_err() {
                return 0;
            }
            let mut buf = vec![0u8; back as usize];
            let n = file.read(&mut buf).unwrap_or(0);
            if n == 0 {
                return 0;
            }

            // Look for the rightmost '\n' in this window that is before `from`
            for i in (0..n).rev() {
                if buf[i] == b'\n' {
                    let candidate = start + (i as u64) + 1;
                    if candidate < from {
                        return candidate;
                    }
                }
            }

            if start == 0 {
                return 0;
            }
            search = start;
        }
    }

    // Small convenience wrapper around the module-level snap function.
    fn snap_to_line_start_in_file(&self, path: &std::path::Path, offset: u64) -> u64 {
        let mut f = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return offset,
        };
        snap_text_line_start(&mut f, offset)
    }

    // ------------------------------------------------------------------
    // Width-aware helpers for per-visual-row scrolling (the key UX fix)
    // ------------------------------------------------------------------

    /// Read one logical line starting at `line_start` (up to next \n or EOF).
    /// Returns (content_without_newline, byte_length_including_newline_if_present)
    fn read_logical_line(&self, path: &std::path::Path, line_start: u64) -> (String, u64) {
        let mut file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(_) => return (String::new(), 0),
        };
        if file.seek(SeekFrom::Start(line_start)).is_err() {
            return (String::new(), 0);
        }
        let mut reader = BufReader::new(file);
        let mut buf = String::new();
        match reader.read_line(&mut buf) {
            Ok(0) => (String::new(), 0),
            Ok(n) => {
                let had_newline = buf.ends_with('\n');
                if had_newline {
                    buf.pop();
                    if buf.ends_with('\r') {
                        buf.pop();
                    }
                }
                (buf, n as u64)
            }
            Err(_) => (String::new(), 0),
        }
    }

    /// Given a byte offset inside a logical line (line_start .. line_start+line_len),
    /// and the width, return which visual row (0-based) this byte falls into when the line is wrapped.
    fn visual_row_index_for_byte(
        &self,
        line_content: &str,
        byte_offset_in_line: u64,
        width: u16,
    ) -> usize {
        if width == 0 {
            return 0;
        }
        let w = width as usize;
        let bytes_up_to_pos: usize = line_content
            .as_bytes()
            .get(..byte_offset_in_line.min(line_content.len() as u64) as usize)
            .map(|b| b.len())
            .unwrap_or(0);

        // Count how many full rows are before this byte position (very simple char-based)
        let mut row = 0usize;
        let mut col = 0usize;
        for (i, _ch) in line_content.char_indices() {
            if i >= bytes_up_to_pos {
                break;
            }
            col += 1;
            if col >= w {
                row += 1;
                col = 0;
            }
        }
        row
    }

    /// Return the byte offset (relative to line_start) of the first character of the given visual row
    /// when wrapping `line_content` to `width`.
    fn byte_offset_of_visual_row(&self, line_content: &str, row_index: usize, width: u16) -> u64 {
        if width == 0 || row_index == 0 {
            return 0;
        }
        let w = width as usize;
        let mut current_row = 0usize;
        let mut col = 0usize;
        let mut byte_idx = 0usize;

        for ch in line_content.chars() {
            if current_row >= row_index {
                break;
            }
            let ch_bytes = ch.len_utf8();
            col += 1;
            byte_idx += ch_bytes;
            if col >= w {
                current_row += 1;
                col = 0;
            }
        }
        byte_idx as u64
    }

    /// Advance exactly one visual display row from the current byte position, respecting wrapping at `width`.
    fn advance_one_visual_row(
        &self,
        path: &std::path::Path,
        current: u64,
        width: u16,
        file_size: u64,
    ) -> u64 {
        if current >= file_size {
            return file_size;
        }

        let line_start = self.snap_to_line_start_in_file(path, current);
        let (line_content, line_len_incl_nl) = self.read_logical_line(path, line_start);

        if line_content.is_empty() {
            return (line_start + line_len_incl_nl).min(file_size);
        }

        let wrapped = wrap_to_width(&line_content, width);
        if wrapped.len() <= 1 {
            return (line_start + line_len_incl_nl).min(file_size);
        }

        let bytes_into_line = current.saturating_sub(line_start);
        let current_row = self.visual_row_index_for_byte(&line_content, bytes_into_line, width);

        if current_row + 1 < wrapped.len() {
            let bytes_to_skip =
                self.byte_offset_of_visual_row(&line_content, current_row + 1, width);
            return line_start + bytes_to_skip;
        }

        (line_start + line_len_incl_nl).min(file_size)
    }

    /// Retreat exactly one visual display row.
    fn retreat_one_visual_row(&self, path: &std::path::Path, current: u64, width: u16) -> u64 {
        if current == 0 {
            return 0;
        }

        let line_start = self.snap_to_line_start_in_file(path, current);
        let (line_content, _line_len) = self.read_logical_line(path, line_start);

        let bytes_into_line = current.saturating_sub(line_start);
        let current_row = self.visual_row_index_for_byte(&line_content, bytes_into_line, width);

        if current_row > 0 {
            let bytes_to_skip =
                self.byte_offset_of_visual_row(&line_content, current_row - 1, width);
            return line_start + bytes_to_skip;
        }

        if line_start == 0 {
            return 0;
        }

        let prev_line_start = self.previous_line_start(path, line_start);
        let (prev_content, _prev_len) = self.read_logical_line(path, prev_line_start);
        let prev_wrapped = wrap_to_width(&prev_content, width);

        if prev_wrapped.is_empty() {
            return prev_line_start;
        }

        let last_row_idx = prev_wrapped.len() - 1;
        let bytes_to_last_row = self.byte_offset_of_visual_row(&prev_content, last_row_idx, width);
        prev_line_start + bytes_to_last_row
    }
}

/// Very simple width-aware wrapper (monospace assumption).
/// Returns one String per visual display row, each ≤ `width` columns.
fn wrap_to_width(s: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return vec![s.to_string()];
    }
    let w = width as usize;
    if s.chars().count() <= w {
        return vec![s.to_string()];
    }

    let mut out = Vec::new();
    let mut current = String::new();
    let mut cur_w = 0usize;

    for ch in s.chars() {
        let ch_w = 1; // good enough for now (we can use unicode-width crate later)
        if cur_w + ch_w > w && !current.is_empty() {
            out.push(std::mem::take(&mut current));
            cur_w = 0;
        }
        current.push(ch);
        cur_w += ch_w;
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
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

    #[test]
    fn wrap_to_width_basic() {
        assert_eq!(wrap_to_width("hello", 10), vec!["hello"]);
        assert_eq!(wrap_to_width("hello", 5), vec!["hello"]);
        // Our current simple wrapper does hard char wrapping (no word awareness)
        assert_eq!(wrap_to_width("hello world", 5), vec!["hello", " worl", "d"]);
        assert_eq!(wrap_to_width("", 10), vec![""]);
    }

    #[test]
    fn render_lines_respects_width_and_wrapping() {
        let content = b"1234567890\nshort\nvery long line that should wrap when width is small";
        let f = write_temp(content);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = TextViewer;

        // Wide enough → no wrapping
        let lines = viewer.render_lines(&info, 0, 10, 80);
        assert!(lines.iter().any(|l| l.contains("1234567890")));

        // Very narrow width → heavy wrapping
        let lines = viewer.render_lines(&info, 0, 20, 3);
        assert!(lines.len() >= 3);
        // First logical line "1234567890" should be split
        assert!(lines[0].starts_with("123"));
    }

    #[test]
    fn advance_lines_respects_width_for_wrapped_content() {
        let content = b"12345678901234567890\nnext line";
        let f = write_temp(content);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = TextViewer;

        // Start at 0 with width=5 (each line becomes 4 display rows)
        let pos1 = viewer.advance_lines(&info, 0, 1, 5);
        assert!(
            pos1 > 0 && pos1 < 10,
            "Should move within first logical line"
        );

        let pos2 = viewer.advance_lines(&info, pos1, 3, 5);
        assert!(pos2 >= 20, "Should have crossed into second line");
    }

    #[test]
    fn render_lines_starts_mid_wrapped_line() {
        let content = b"abcdefghij"; // 10 chars, no newline
        let f = write_temp(content);
        let info = FileInfo::from_path(f.path()).unwrap();

        let viewer = TextViewer;

        // Width 4 → 3 display rows for the line
        // Start at byte offset 5 (somewhere in the middle)
        let lines = viewer.render_lines(&info, 5, 5, 4);

        // Should not start with "abcd"
        assert!(!lines[0].starts_with("abcd"));
        assert!(lines[0].starts_with("efg") || lines[0].starts_with("fgh"));
    }
}
