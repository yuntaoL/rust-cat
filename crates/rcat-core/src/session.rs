//! Host-owned file session: one mmap and metadata shared by every viewer.

use std::io;
use std::path::Path;
use std::sync::Arc;

use crate::backing::{self, FileBacking};
use crate::file_info::FileInfo;

/// A single open file as seen by the host.
///
/// All viewers (built-in and plugins) receive a [`crate::view::ViewContext`] that
/// references this session. The host is the only owner of the memory map; viewers
/// must not re-open the path for interactive rendering when a session is available.
#[derive(Debug)]
pub struct FileSession {
    info: FileInfo,
}

impl FileSession {
    /// Inspect the file on disk and attach a read-only mmap.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Self::from_info(FileInfo::from_path(path)?)
    }

    /// Ensure `info` has mmap backing (opens the file if needed).
    pub fn from_info(info: FileInfo) -> io::Result<Self> {
        let backing = backing::backing_for_info(&info)?;
        Ok(Self {
            info: info.with_backing(backing),
        })
    }

    /// File metadata (detection, size, paths). Backing is always set on a session.
    pub fn info(&self) -> &FileInfo {
        &self.info
    }

    pub fn size(&self) -> u64 {
        self.info.size
    }

    pub fn path(&self) -> &Path {
        &self.info.path
    }

    pub fn backing(&self) -> &Arc<FileBacking> {
        self.info
            .backing
            .as_ref()
            .expect("FileSession always has backing")
    }

    /// Full file as bytes (zero-copy mmap).
    pub fn bytes(&self) -> &[u8] {
        self.backing().bytes()
    }

    /// Byte slice from `offset` for at most `len` bytes (clamped to EOF).
    pub fn slice(&self, offset: u64, len: usize) -> &[u8] {
        self.backing().slice(offset, len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn session_maps_and_slices() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "hello").unwrap();
        let session = FileSession::open(f.path()).unwrap();
        assert_eq!(session.size(), 5);
        assert_eq!(session.bytes(), b"hello");
        assert_eq!(session.slice(1, 3), b"ell");
        assert!(session.info().backing.is_some());
    }
}
