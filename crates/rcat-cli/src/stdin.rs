//! Spool stdin into a temporary file so viewers can mmap it like a regular path.

use std::io::{self, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;

/// Copy all of stdin into a temporary file. The temp file is deleted when `guard` is dropped.
pub fn spool_stdin_to_temp() -> io::Result<(NamedTempFile, PathBuf)> {
    let mut tmp = NamedTempFile::new()?;
    let bytes = io::copy(&mut io::stdin().lock(), &mut tmp)?;
    tmp.flush()?;
    let path = tmp.path().to_path_buf();
    tracing::debug!(bytes, "spooled stdin to temporary file");
    Ok((tmp, path))
}
