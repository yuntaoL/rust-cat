//! File information and metadata.
//!
//! `FileInfo` is the central description of a file that all viewers and
//! the dump logic operate on.

use std::path::{Path, PathBuf};

/// High-level classification of the file content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentKind {
    /// Likely human-readable text (UTF-8 or common encodings).
    Text,
    /// Binary data (contains null bytes, invalid UTF-8 in sampling, etc.).
    Binary,
    /// Empty file.
    Empty,
}

/// Information about a file we are about to view or dump.
#[derive(Debug, Clone)]
pub struct FileInfo {
    /// Original path provided by the user (may be relative).
    pub path: PathBuf,
    /// Canonical absolute path (best effort).
    pub absolute_path: Option<PathBuf>,
    /// File size in bytes.
    pub size: u64,
    /// Best guess at the kind of content.
    pub kind: ContentKind,
    /// Human-friendly type description (e.g. "UTF-8 text", "ELF binary", "PNG image").
    pub type_description: String,
    /// Detected extension (without the dot), if any.
    pub extension: Option<String>,
}

impl FileInfo {
    /// Create a `FileInfo` by inspecting the file on disk.
    pub fn from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let metadata = std::fs::metadata(&path)?;

        let size = metadata.len();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());

        let kind = if size == 0 {
            ContentKind::Empty
        } else {
            // We do a cheap content probe. For the real implementation we will
            // use the more sophisticated logic in `detection`.
            crate::detection::quick_classify(&path, size)?
        };

        let type_description = match kind {
            ContentKind::Empty => "empty file".to_string(),
            ContentKind::Text => "text file".to_string(),
            ContentKind::Binary => "binary data".to_string(),
        };

        let absolute_path = std::fs::canonicalize(&path).ok();

        Ok(Self {
            path,
            absolute_path,
            size,
            kind,
            type_description,
            extension,
        })
    }

    /// Returns true if this file should be treated as text by default.
    pub fn is_text(&self) -> bool {
        matches!(self.kind, ContentKind::Text | ContentKind::Empty)
    }
}
