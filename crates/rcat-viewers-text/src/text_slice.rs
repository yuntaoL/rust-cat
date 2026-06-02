//! Text rendering and scrolling over an in-memory byte slice (host mmap).

/// Snap byte offset to the start of the containing logical line (scan backward for `\n`).
pub fn snap_line_start(data: &[u8], offset: u64) -> u64 {
    if offset == 0 || data.is_empty() {
        return 0;
    }
    let offset = offset.min(data.len() as u64) as usize;
    let back = std::cmp::min(8192, offset);
    let start = offset - back;
    if let Some(pos) = data[start..offset].iter().rposition(|&b| b == b'\n') {
        return (start + pos + 1) as u64;
    }
    if start == 0 {
        0
    } else {
        offset as u64
    }
}

/// Read one logical line at `line_start`. Returns (content, byte length including newline if present).
pub fn read_logical_line(data: &[u8], line_start: u64) -> (String, u64) {
    let size = data.len() as u64;
    if line_start >= size {
        return (String::new(), 0);
    }
    let start = line_start as usize;
    let rest = &data[start..];
    let nl = rest.iter().position(|&b| b == b'\n');
    let (content_end, len_incl) = match nl {
        Some(i) => (start + i, (i + 1) as u64),
        None => (data.len(), (data.len() - start) as u64),
    };
    let mut content = String::from_utf8_lossy(&data[start..content_end]).into_owned();
    if content.ends_with('\r') {
        content.pop();
    }
    (content, len_incl)
}

/// Byte offset of the line start immediately before `from`.
pub fn previous_line_start(data: &[u8], from: u64) -> u64 {
    if from == 0 || data.is_empty() {
        return 0;
    }
    let mut search = from.min(data.len() as u64) as usize;
    loop {
        let back = std::cmp::min(8192, search);
        let window_start = search - back;
        let window = &data[window_start..search];
        for i in (0..window.len()).rev() {
            if window[i] == b'\n' {
                let candidate = window_start + i + 1;
                if (candidate as u64) < from {
                    return candidate as u64;
                }
            }
        }
        if window_start == 0 {
            return 0;
        }
        search = window_start;
    }
}

pub fn visual_row_index_for_byte(line_content: &str, byte_offset_in_line: u64, width: u16) -> usize {
    if width == 0 {
        return 0;
    }
    let w = width as usize;
    let bytes_up_to_pos = line_content
        .as_bytes()
        .get(..byte_offset_in_line.min(line_content.len() as u64) as usize)
        .map(|b| b.len())
        .unwrap_or(0);

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

pub fn byte_offset_of_visual_row(line_content: &str, row_index: usize, width: u16) -> u64 {
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

pub fn wrap_to_width(s: &str, width: u16) -> Vec<String> {
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
        let ch_w = 1;
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

pub fn render_display_rows(
    data: &[u8],
    file_size: u64,
    start_offset: u64,
    max_rows: u16,
    width: u16,
) -> Vec<String> {
    if file_size == 0 || data.is_empty() {
        return vec!["(empty file)".to_string()];
    }

    let mut display_rows: Vec<String> = Vec::new();
    let mut current = start_offset.min(file_size);
    let mut line_start = snap_line_start(data, current);

    while display_rows.len() < max_rows as usize {
        let (line_content, line_len_incl_nl) = read_logical_line(data, line_start);
        if line_len_incl_nl == 0 {
            break;
        }

        let wrapped = wrap_to_width(&line_content, width);
        let start_row_in_this_line = if current > line_start {
            let bytes_into_line = current.saturating_sub(line_start);
            visual_row_index_for_byte(&line_content, bytes_into_line, width)
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

        line_start += line_len_incl_nl;
        if current < line_start {
            current = line_start;
        }
        if line_start >= file_size {
            break;
        }
    }

    if display_rows.is_empty() {
        display_rows.push("(end of file)".to_string());
    }
    display_rows.truncate(max_rows as usize);
    display_rows
}

pub fn advance_one_visual_row(data: &[u8], file_size: u64, current: u64, width: u16) -> u64 {
    if current >= file_size {
        return file_size;
    }

    let line_start = snap_line_start(data, current);
    let (line_content, line_len_incl_nl) = read_logical_line(data, line_start);

    if line_content.is_empty() {
        return (line_start + line_len_incl_nl).min(file_size);
    }

    let wrapped = wrap_to_width(&line_content, width);
    if wrapped.len() <= 1 {
        return (line_start + line_len_incl_nl).min(file_size);
    }

    let bytes_into_line = current.saturating_sub(line_start);
    let current_row = visual_row_index_for_byte(&line_content, bytes_into_line, width);

    if current_row + 1 < wrapped.len() {
        let bytes_to_skip = byte_offset_of_visual_row(&line_content, current_row + 1, width);
        return line_start + bytes_to_skip;
    }

    (line_start + line_len_incl_nl).min(file_size)
}

pub fn retreat_one_visual_row(data: &[u8], current: u64, width: u16) -> u64 {
    if current == 0 {
        return 0;
    }

    let line_start = snap_line_start(data, current);
    let (line_content, _line_len) = read_logical_line(data, line_start);

    let bytes_into_line = current.saturating_sub(line_start);
    let current_row = visual_row_index_for_byte(&line_content, bytes_into_line, width);

    if current_row > 0 {
        let bytes_to_skip = byte_offset_of_visual_row(&line_content, current_row - 1, width);
        return line_start + bytes_to_skip;
    }

    if line_start == 0 {
        return 0;
    }

    let prev_line_start = previous_line_start(data, line_start);
    let (prev_content, _prev_len) = read_logical_line(data, prev_line_start);
    let prev_wrapped = wrap_to_width(&prev_content, width);

    if prev_wrapped.is_empty() {
        return prev_line_start;
    }

    let last_row_idx = prev_wrapped.len() - 1;
    let bytes_to_last_row = byte_offset_of_visual_row(&prev_content, last_row_idx, width);
    prev_line_start + bytes_to_last_row
}

pub fn advance_lines_bytes(data: &[u8], file_size: u64, current: u64, delta: i64, width: u16) -> u64 {
    if delta == 0 {
        return current;
    }
    let mut pos = current.min(file_size);
    let steps = delta.unsigned_abs() as usize;
    let forward = delta > 0;

    for _ in 0..steps {
        if forward {
            pos = advance_one_visual_row(data, file_size, pos, width);
        } else {
            pos = retreat_one_visual_row(data, pos, width);
        }
        if pos == 0 && !forward {
            break;
        }
        if pos >= file_size && forward {
            break;
        }
    }
    pos.min(file_size)
}

pub fn text_status(file_size: u64, pos: u64) -> String {
    let pct = if file_size == 0 {
        100
    } else {
        ((pos as f64 / file_size as f64) * 100.0) as u32
    };
    format!("Text  {} / {} B ({pct}%)", pos, file_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_and_read_line_from_slice() {
        let data = b"ab\ncde\n";
        assert_eq!(snap_line_start(data, 3), 3);
        let (s, len) = read_logical_line(data, 0);
        assert_eq!(s, "ab");
        assert_eq!(len, 3);
    }

    #[test]
    fn wrap_to_width_splits_long_line() {
        assert_eq!(wrap_to_width("hello world", 5), vec!["hello", " worl", "d"]);
    }
}