//! Content type detection.
//!
//! The core performs a fast first-pass detection (primarily using the `infer` crate).
//! This result is made available to all viewers via `FileProbe` so that most plugins
//! can simply trust the core's result instead of doing their own heavy detection.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::file_info::ContentKind;

/// Rich result from the core's first-pass detection.
/// Viewers can rely on this and only do additional work when they need to.
#[derive(Debug, Clone, Default)]
pub struct PreliminaryDetection {
    /// MIME type guessed by the core (e.g. "image/jpeg", "application/json").
    pub mime_type: Option<String>,
    /// File extension inferred from magic (e.g. "jpg", "json").
    pub extension: Option<String>,
    /// Human-friendly format description (e.g. "JPEG image", "ELF executable").
    pub format: Option<String>,
    /// Coarse classification still useful for quick decisions.
    pub kind: ContentKind,
}

/// Perform first-pass detection using `infer` (magic bytes) + fallback heuristics.
pub fn detect_file(path: &Path, size: u64) -> std::io::Result<PreliminaryDetection> {
    if size == 0 {
        return Ok(PreliminaryDetection {
            kind: ContentKind::Empty,
            ..Default::default()
        });
    }

    // 1. Try magic byte detection with infer (primary source of truth)
    if let Ok(Some(kind)) = infer::get_from_path(path) {
        let mime = kind.mime_type().to_string();
        let ext = kind.extension().to_string();

        // Derive coarse ContentKind from MIME
        let coarse_kind =
            if mime.starts_with("text/") || mime == "application/json" || mime == "application/xml"
            {
                ContentKind::Text
            } else {
                ContentKind::Binary
            };

        return Ok(PreliminaryDetection {
            mime_type: Some(mime),
            extension: Some(ext.clone()),
            format: Some(kind.to_string()),
            kind: coarse_kind,
        });
    }

    // 2. Fallback to our old cheap heuristic (null bytes + UTF-8 sampling)
    let kind = quick_classify_fallback(path, size)?;

    Ok(PreliminaryDetection {
        kind,
        ..Default::default()
    })
}

/// Fallback classification (used when infer doesn't recognize the file).
fn quick_classify_fallback(path: &Path, size: u64) -> std::io::Result<ContentKind> {
    if size == 0 {
        return Ok(ContentKind::Empty);
    }

    let mut file = std::fs::File::open(path)?;

    let mut head = [0u8; 4096];
    let n = file.read(&mut head)?;
    let head = &head[..n];

    if contains_null(head) {
        return Ok(ContentKind::Binary);
    }

    if std::str::from_utf8(head).is_ok() {
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

    Ok(ContentKind::Binary)
}

fn contains_null(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

// Keep old name as alias for backward compatibility inside the crate for now
pub fn quick_classify(path: &Path, size: u64) -> std::io::Result<ContentKind> {
    quick_classify_fallback(path, size)
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
