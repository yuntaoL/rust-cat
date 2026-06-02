//! Spool stdin into a temporary file so viewers can mmap it like a regular path.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;

/// Copy all bytes from `reader` into a temporary file.
pub fn spool_reader_to_temp(mut reader: impl Read) -> io::Result<(NamedTempFile, PathBuf)> {
    let mut tmp = NamedTempFile::new()?;
    let bytes = io::copy(&mut reader, &mut tmp)?;
    tmp.flush()?;
    let path = tmp.path().to_path_buf();
    tracing::debug!(bytes, "spooled input to temporary file");
    Ok((tmp, path))
}

/// Copy all of stdin into a temporary file. The temp file is deleted when `guard` is dropped.
pub fn spool_stdin_to_temp() -> io::Result<(NamedTempFile, PathBuf)> {
    spool_reader_to_temp(io::stdin().lock())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn spool_reader_to_temp_preserves_bytes() {
        let data = b"{\"stdin\":true}\n";
        let (_guard, path) = spool_reader_to_temp(Cursor::new(data)).unwrap();
        let on_disk = std::fs::read(&path).unwrap();
        assert_eq!(on_disk, data);
    }
}
