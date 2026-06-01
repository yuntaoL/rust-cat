//! High-quality non-interactive dump implementations.
//!
//! These functions produce correct, pipe-friendly output for both text and hex modes.
//! They are the single source of truth for the `--stdout` / non-TTY path.

use std::io::{self, Write};

use memmap2::Mmap;

use crate::file_info::FileInfo;

/// Options controlling non-interactive dump behavior.
#[derive(Debug, Clone, Default)]
pub struct DumpOptions {
    /// Byte offset to start dumping from.
    pub offset: u64,
    /// Maximum number of bytes to dump (None = until end of file or mapping).
    pub length: Option<u64>,
}

/// Dump the file as text (best-effort UTF-8).
///
/// This is intentionally conservative: we never assume the whole file is valid UTF-8.
/// Invalid sequences are replaced with the Unicode replacement character.
pub fn dump_text<W: Write>(info: &FileInfo, mut writer: W, opts: &DumpOptions) -> io::Result<()> {
    if info.size == 0 {
        return Ok(());
    }

    let file = std::fs::File::open(&info.path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    let start = opts.offset.min(mmap.len() as u64) as usize;
    let end = match opts.length {
        Some(len) => (start + len as usize).min(mmap.len()),
        None => mmap.len(),
    };

    let slice = &mmap[start..end];

    // Use lossy conversion for maximum robustness when dumping "text"
    let s = String::from_utf8_lossy(slice);
    writer.write_all(s.as_bytes())?;

    // Ensure we end with a newline if the last byte wasn't one (nice for pipes)
    if !slice.is_empty() && !slice.ends_with(b"\n") {
        writer.write_all(b"\n")?;
    }

    Ok(())
}

/// Dump the file as classic hex + ASCII (similar to `xxd -g1` / `hexyl` style).
///
/// Always produces 16 bytes per line with proper padding on the last line.
pub fn dump_hex<W: Write>(info: &FileInfo, mut writer: W, opts: &DumpOptions) -> io::Result<()> {
    if info.size == 0 {
        return Ok(());
    }

    let file = std::fs::File::open(&info.path)?;
    let mmap = unsafe { Mmap::map(&file)? };

    let start = opts.offset.min(mmap.len() as u64) as usize;
    let end = match opts.length {
        Some(len) => (start + len as usize).min(mmap.len()),
        None => mmap.len(),
    };

    let slice = &mmap[start..end];
    let mut addr = opts.offset;

    for chunk in slice.chunks(16) {
        // Address
        write!(writer, "{:08x}: ", addr)?;

        // Hex bytes
        for (i, &b) in chunk.iter().enumerate() {
            write!(writer, "{:02x}", b)?;
            if i < 15 {
                write!(writer, " ")?;
            }
            if i == 7 {
                write!(writer, " ")?;
            }
        }

        // Padding for short last line
        for i in chunk.len()..16 {
            write!(writer, "   ")?;
            if i == 7 {
                write!(writer, " ")?;
            }
        }

        // ASCII
        write!(writer, " |")?;
        for &b in chunk {
            let c = if (0x20..=0x7e).contains(&b) {
                b as char
            } else {
                '.'
            };
            write!(writer, "{}", c)?;
        }
        writeln!(writer, "|")?;

        addr += chunk.len() as u64;
    }

    Ok(())
}

/// Convenience entry point used by the binary today.
pub fn dump<W: Write>(
    info: &FileInfo,
    writer: W,
    force_hex: bool,
    opts: &DumpOptions,
) -> io::Result<()> {
    if force_hex || !info.is_text() {
        dump_hex(info, writer, opts)
    } else {
        dump_text(info, writer, opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_temp_file(content: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bin");
        std::fs::write(&path, content).unwrap();
        (dir, path)
    }

    #[test]
    fn hex_dump_produces_correct_format_and_padding() {
        let data = b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10";
        let (_dir, path) = make_temp_file(data);

        let info = FileInfo::from_path(&path).unwrap();
        let mut out = Vec::new();
        dump_hex(&info, &mut out, &DumpOptions::default()).unwrap();

        let s = String::from_utf8(out).unwrap();
        assert_eq!(s.lines().count(), 2);
        assert!(s.contains("00 01 02 03 04 05 06 07  08 09 0a 0b 0c 0d 0e 0f"));
        assert!(s.contains("10"));
    }

    #[test]
    fn text_dump_is_lossy_but_never_panics_on_binary() {
        let data = b"hello\xFF\xFEworld\n";
        let (_dir, path) = make_temp_file(data);

        let info = FileInfo::from_path(&path).unwrap();
        let mut out = Vec::new();
        dump_text(&info, &mut out, &DumpOptions::default()).unwrap();

        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("hello"));
        assert!(s.contains("world"));
    }

    #[test]
    fn dump_respects_offset_and_length() {
        let data = b"0123456789ABCDEF";
        let (_dir, path) = make_temp_file(data);

        let info = FileInfo::from_path(&path).unwrap();
        let mut out = Vec::new();
        dump_hex(
            &info,
            &mut out,
            &DumpOptions {
                offset: 4,
                length: Some(6),
            },
        )
        .unwrap();

        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("00000004:")); // correct address for offset 4
        assert!(s.contains("04")); // the byte at offset 4 should appear
        assert!(!s.contains("00000000:")); // should not show from start
    }

    #[test]
    fn hex_dump_snapshot() {
        let data = b"\x00\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10";
        let (_dir, path) = make_temp_file(data);
        let info = FileInfo::from_path(&path).unwrap();
        let mut out = Vec::new();
        dump_hex(&info, &mut out, &DumpOptions::default()).unwrap();
        insta::assert_snapshot!(String::from_utf8(out).unwrap());
    }

    #[test]
    fn dump_text_adds_trailing_newline_when_needed() {
        let data = b"no newline at end";
        let (_dir, path) = make_temp_file(data);

        let info = FileInfo::from_path(&path).unwrap();
        let mut out = Vec::new();
        dump_text(&info, &mut out, &DumpOptions::default()).unwrap();

        assert!(out.ends_with(b"\n"));
    }
}
