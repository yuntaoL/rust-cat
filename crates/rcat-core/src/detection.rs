//! Content type detection (text vs binary).
//!
//! We use a combination of cheap signals:
//! - Presence of null bytes → strongly binary
//! - UTF-8 validity of a sample from the beginning and middle of the file
//! - Common binary file signatures (ELF, PNG, etc.) in the future

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::file_info::ContentKind;

/// Quick and cheap classification used by `FileInfo`.
pub fn quick_classify(path: &Path, size: u64) -> std::io::Result<ContentKind> {
    if size == 0 {
        return Ok(ContentKind::Empty);
    }

    let mut file = std::fs::File::open(path)?;

    // Read a sample from the beginning (most important for text detection)
    let mut head = [0u8; 4096];
    let n = file.read(&mut head)?;
    let head = &head[..n];

    if contains_null(head) {
        return Ok(ContentKind::Binary);
    }

    // Check UTF-8 validity of the head
    if std::str::from_utf8(head).is_ok() {
        // For larger files, also sample somewhere in the middle to be safer
        if size > 8192 {
            let mid_offset = (size / 2).saturating_sub(1024);
            file.seek(SeekFrom::Start(mid_offset))?;
            let mut mid = [0u8; 2048];
            let n = file.read(&mut mid)?;
            if std::str::from_utf8(&mid[..n]).is_err() {
                return Ok(ContentKind::Binary);
            }
        }
        return Ok(ContentKind::Text);
    }

    // Not valid UTF-8 in the head → treat as binary for safety in v0.1
    Ok(ContentKind::Binary)
}

fn contains_null(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_null_as_binary() {
        let data = b"hello\x00world";
        assert!(contains_null(data));
    }

    #[test]
    fn pure_ascii_is_not_null() {
        assert!(!contains_null(b"just normal text"));
    }
}
