//! Memory-mapped read-only file backing for viewers and dump paths.

use memmap2::Mmap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Read-only mmap of a file on disk.
#[derive(Debug)]
pub struct FileBacking {
    path: PathBuf,
    mmap: Mmap,
    size: u64,
}

impl FileBacking {
    /// Map the file read-only. Empty files map to an empty slice.
    pub fn open(path: impl AsRef<Path>) -> io::Result<Arc<Self>> {
        let path = path.as_ref().to_path_buf();
        let file = std::fs::File::open(&path)?;
        let size = file.metadata()?.len();

        // SAFETY: read-only map of a file we opened; not mutated.
        let mmap = unsafe { Mmap::map(&file)? };

        Ok(Arc::new(Self { path, mmap, size }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// Full file contents as a byte slice.
    pub fn bytes(&self) -> &[u8] {
        &self.mmap
    }

    /// Slice from `offset` for at most `len` bytes (clamped to EOF).
    pub fn slice(&self, offset: u64, len: usize) -> &[u8] {
        let start = offset.min(self.size) as usize;
        let end = (start + len).min(self.mmap.len());
        if start >= end {
            &[]
        } else {
            &self.mmap[start..end]
        }
    }
}

/// Resolve mmap backing for a `FileInfo`, using an existing map or opening the path.
pub fn backing_for_info(info: &crate::file_info::FileInfo) -> io::Result<Arc<FileBacking>> {
    if let Some(b) = &info.backing {
        return Ok(Arc::clone(b));
    }
    FileBacking::open(&info.path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn maps_file_and_slices() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "hello").unwrap();
        let backing = FileBacking::open(f.path()).unwrap();
        assert_eq!(backing.size(), 5);
        assert_eq!(backing.bytes(), b"hello");
        assert_eq!(backing.slice(2, 2), b"ll");
    }
}
