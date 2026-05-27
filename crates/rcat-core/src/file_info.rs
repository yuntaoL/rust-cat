//! File information and metadata.
//!
//! `FileInfo` is the central description of a file that all viewers and
//! the dump logic operate on.

use std::path::{Path, PathBuf};

/// High-level classification of the file content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentKind {
    /// Likely human-readable text (UTF-8 or common encodings).
    #[default]
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

    /// Result of the core's first-pass detection using `infer` + heuristics.
    /// Viewers can (and should) rely on this for most common formats.
    pub detected: crate::detection::PreliminaryDetection,
}

impl FileInfo {
    /// Create a `FileInfo` by inspecting the file on disk.
    ///
    /// This now performs a proper first-pass detection using the `infer` crate
    /// (magic bytes). The result is stored in `detected` so that viewers can
    /// rely on it instead of doing redundant work.
    pub fn from_path(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let metadata = std::fs::metadata(&path)?;

        let size = metadata.len();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase());

        // Use the new infer-based first-pass detection
        let detected = crate::detection::detect_file(&path, size)?;

        let type_description = detected
            .format
            .clone()
            .unwrap_or_else(|| match detected.kind {
                ContentKind::Empty => "empty file".to_string(),
                ContentKind::Text => "text file".to_string(),
                ContentKind::Binary => "binary data".to_string(),
            });

        let absolute_path = std::fs::canonicalize(&path).ok();

        Ok(Self {
            path,
            absolute_path,
            size,
            kind: detected.kind,
            type_description,
            extension,
            detected,
        })
    }

    /// Returns true if this file should be treated as text by default.
    pub fn is_text(&self) -> bool {
        matches!(self.kind, ContentKind::Text | ContentKind::Empty)
    }
}
