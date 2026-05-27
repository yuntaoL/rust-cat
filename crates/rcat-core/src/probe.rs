//! FileProbe abstraction for viewer detection phase.
//!
//! This allows viewers (plugins) to read limited raw data during `can_handle`
//! without opening files themselves, with built-in caching and safety limits.

use std::io::{self, Read};
use std::path::Path;

use crate::FileInfo;
use crate::detection::PreliminaryDetection;
use crate::file_info::ContentKind;

/// Maximum number of bytes we are willing to provide to plugins during detection.
/// This is a deliberate limit to keep detection fast and safe.
pub const DETECTION_READ_LIMIT: usize = 16 * 1024; // 16 KiB

/// Provides controlled access to file content during the viewer selection phase.
///
/// Viewers should use this instead of opening files directly when deciding
/// whether they can handle a file.
pub trait FileProbe {
    /// Returns the total size of the file in bytes.
    fn file_size(&self) -> u64;

    /// Reads up to `len` bytes starting at `offset`.
    ///
    /// Returns a slice that remains valid until the next call to `read_bytes`
    /// that would invalidate the buffer (simple implementations may return
    /// the same buffer on every call).
    ///
    /// Implementations are expected to enforce `DETECTION_READ_LIMIT`.
    fn read_bytes(&mut self, offset: u64, len: usize) -> io::Result<&[u8]>;

    /// Returns the result of the core's first-pass detection (powered by `infer`).
    ///
    /// Most viewers should be able to make a good decision just by looking at this
    /// instead of reading raw bytes themselves.
    fn preliminary(&self) -> &PreliminaryDetection;
}

/// A simple `FileProbe` implementation that reads the first `DETECTION_READ_LIMIT`
/// bytes into memory once and serves all requests from that buffer.
///
/// This is efficient for detection (one read) and safe (hard limit).
pub struct PrefixProbe {
    data: Vec<u8>,
    file_size: u64,
}

impl PrefixProbe {
    /// Create a new probe by reading up to `DETECTION_READ_LIMIT` bytes from the file.
    pub fn from_path(path: &Path) -> io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let file_size = file.metadata()?.len();

        let to_read = std::cmp::min(file_size as usize, DETECTION_READ_LIMIT);
        let mut data = vec![0u8; to_read];
        file.read_exact(&mut data)?;

        Ok(Self { data, file_size })
    }

    /// Create a probe from an already-read prefix + known file size.
    /// Useful for testing or when you already have the data.
    pub fn from_prefix(data: Vec<u8>, file_size: u64) -> Self {
        Self { data, file_size }
    }
}

impl FileProbe for PrefixProbe {
    fn file_size(&self) -> u64 {
        self.file_size
    }

    fn read_bytes(&mut self, offset: u64, len: usize) -> io::Result<&[u8]> {
        if offset >= self.file_size {
            return Ok(&[]);
        }

        let start = offset as usize;
        let available = self.data.len().saturating_sub(start);
        let to_take = std::cmp::min(len, available);

        if to_take == 0 {
            return Ok(&[]);
        }

        // We only serve data we pre-read (within DETECTION_READ_LIMIT)
        Ok(&self.data[start..start + to_take])
    }

    fn preliminary(&self) -> &PreliminaryDetection {
        // PrefixProbe doesn't have preliminary info by itself.
        // Use FileProbeWithInfo for the common case.
        static EMPTY: PreliminaryDetection = PreliminaryDetection {
            mime_type: None,
            extension: None,
            format: None,
            kind: ContentKind::Binary,
        };
        &EMPTY
    }
}

/// A convenience probe that combines a `FileInfo` (with preliminary classification)
/// and raw byte access via a `PrefixProbe`.
///
/// This is the type we will typically pass to `can_handle` during the transition period.
pub struct FileProbeWithInfo<'a> {
    pub info: &'a FileInfo,
    probe: PrefixProbe,
}

impl<'a> FileProbeWithInfo<'a> {
    pub fn new(info: &'a FileInfo, prefix_probe: PrefixProbe) -> Self {
        Self {
            info,
            probe: prefix_probe,
        }
    }
}

impl FileProbe for FileProbeWithInfo<'_> {
    fn file_size(&self) -> u64 {
        self.probe.file_size()
    }

    fn read_bytes(&mut self, offset: u64, len: usize) -> io::Result<&[u8]> {
        self.probe.read_bytes(offset, len)
    }

    fn preliminary(&self) -> &PreliminaryDetection {
        &self.info.detected
    }
}
