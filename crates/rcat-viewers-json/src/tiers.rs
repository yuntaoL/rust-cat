//! Tiered JSON viewing: small-file pretty, NDJSON, large/invalid raw fallback.

use std::path::{Path, PathBuf};

/// Files larger than this use the raw byte view (tier L).
pub const SMALL_FILE_LIMIT: u64 = 2 * 1024 * 1024;

/// How the JSON viewer formats content for the TUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsonTier {
    /// Small single JSON value — `serde_json` pretty-printed display lines.
    SmallPretty,
    /// One JSON value per source line — per-line pretty.
    Ndjson,
    /// Large file — raw bytes (syntax colors in TUI).
    LargeRaw,
    /// Parse failed on a small file — raw bytes + error hint in status.
    InvalidRaw,
}

/// Cached display model for a file (built once per path).
#[derive(Clone, Debug)]
pub struct JsonFileCache {
    pub tier: JsonTier,
    /// Human-readable lines shown in the viewport.
    pub display_lines: Vec<String>,
    /// Source byte offset for the start of each display line.
    pub line_start_bytes: Vec<u64>,
    /// Set when tier is [`JsonTier::InvalidRaw`].
    pub parse_error: Option<String>,
}

impl JsonFileCache {
    pub fn line_count(&self) -> u64 {
        self.display_lines.len() as u64
    }

    pub fn line_index_for_byte(&self, byte: u64) -> u64 {
        if self.line_start_bytes.is_empty() {
            return 0;
        }
        let mut idx = 0u64;
        for (i, &start) in self.line_start_bytes.iter().enumerate() {
            if start <= byte {
                idx = i as u64;
            } else {
                break;
            }
        }
        idx
    }

    pub fn byte_for_line_index(&self, line: u64) -> u64 {
        self.line_start_bytes
            .get(line as usize)
            .copied()
            .unwrap_or(0)
    }

    pub fn viewport_lines(&self, start_line: u64, max_rows: u16) -> Vec<String> {
        let start = start_line as usize;
        let end = (start + max_rows as usize).min(self.display_lines.len());
        if start >= self.display_lines.len() {
            return vec!["(end of JSON view)".to_string()];
        }
        self.display_lines[start..end].to_vec()
    }

    pub fn advance_line(&self, current_line: u64, delta: i64) -> u64 {
        let max = self.line_count().saturating_sub(1);
        if delta >= 0 {
            (current_line + delta as u64).min(max)
        } else {
            current_line.saturating_sub((-delta) as u64)
        }
    }
}

/// Choose a tier from file size and content shape.
pub fn detect_tier(size: u64, data: &[u8]) -> JsonTier {
    if size > SMALL_FILE_LIMIT {
        return JsonTier::LargeRaw;
    }
    if is_ndjson(data) {
        return JsonTier::Ndjson;
    }
    if is_valid_json_document(data) {
        return JsonTier::SmallPretty;
    }
    JsonTier::InvalidRaw
}

/// Build or refresh cache for `path` using mmap/session bytes.
pub fn build_cache(_path: &Path, size: u64, data: &[u8]) -> JsonFileCache {
    let tier = detect_tier(size, data);
    match tier {
        JsonTier::LargeRaw => build_raw_cache(data, tier, None),
        JsonTier::InvalidRaw => {
            let err = parse_error_message(data);
            build_raw_cache(data, tier, Some(err))
        }
        JsonTier::Ndjson => build_ndjson_cache(data),
        JsonTier::SmallPretty => match build_small_pretty_cache(data) {
            Ok(cache) => cache,
            Err(err) => build_raw_cache(data, JsonTier::InvalidRaw, Some(err)),
        },
    }
}

fn build_raw_cache(data: &[u8], tier: JsonTier, parse_error: Option<String>) -> JsonFileCache {
    let display_lines: Vec<String> = data
        .split_inclusive(|&b| b == b'\n')
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect();
    let line_start_bytes = line_starts_from_chunks(data);
    JsonFileCache {
        tier,
        display_lines,
        line_start_bytes,
        parse_error,
    }
}

fn build_ndjson_cache(data: &[u8]) -> JsonFileCache {
    let mut display_lines = Vec::new();
    let mut line_start_bytes = Vec::new();
    let mut byte_off = 0u64;
    for line in data.split_inclusive(|&b| b == b'\n') {
        let start = byte_off;
        let trimmed = trim_bytes(line);
        if trimmed.is_empty() {
            byte_off += line.len() as u64;
            continue;
        }
        let formatted = format_ndjson_line(trimmed);
        display_lines.push(formatted);
        line_start_bytes.push(start);
        byte_off += line.len() as u64;
    }
    if display_lines.is_empty() {
        display_lines.push(String::new());
        line_start_bytes.push(0);
    }
    JsonFileCache {
        tier: JsonTier::Ndjson,
        display_lines,
        line_start_bytes,
        parse_error: None,
    }
}

fn build_small_pretty_cache(data: &[u8]) -> Result<JsonFileCache, String> {
    let trimmed = trim_bytes(data);
    let value: serde_json::Value =
        serde_json::from_slice(trimmed).map_err(|e| e.to_string())?;
    let pretty = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?;
    let display_lines: Vec<String> = pretty.lines().map(str::to_string).collect();
    let line_count = display_lines.len().max(1) as u64;
    let file_size = data.len() as u64;
    let line_start_bytes: Vec<u64> = (0..line_count)
        .map(|i| (i * file_size.saturating_sub(1)) / line_count.max(1))
        .collect();
    Ok(JsonFileCache {
        tier: JsonTier::SmallPretty,
        display_lines,
        line_start_bytes,
        parse_error: None,
    })
}

fn format_ndjson_line(line: &[u8]) -> String {
    match serde_json::from_slice::<serde_json::Value>(line) {
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| String::from_utf8_lossy(line).into_owned()),
        Err(_) => String::from_utf8_lossy(line).into_owned(),
    }
}

fn line_starts_from_chunks(data: &[u8]) -> Vec<u64> {
    let mut starts = Vec::new();
    let mut off = 0u64;
    for chunk in data.split_inclusive(|&b| b == b'\n') {
        starts.push(off);
        off += chunk.len() as u64;
    }
    if starts.is_empty() {
        starts.push(0);
    }
    starts
}

fn trim_bytes(data: &[u8]) -> &[u8] {
    let start = data
        .iter()
        .position(|&b| !b.is_ascii_whitespace())
        .unwrap_or(data.len());
    let end = data
        .iter()
        .rposition(|&b| !b.is_ascii_whitespace())
        .map(|i| i + 1)
        .unwrap_or(start);
    &data[start..end]
}

fn is_valid_json_document(data: &[u8]) -> bool {
    let trimmed = trim_bytes(data);
    if trimmed.is_empty() {
        return false;
    }
    serde_json::from_slice::<serde_json::Value>(trimmed).is_ok()
}

fn is_ndjson(data: &[u8]) -> bool {
    let mut lines = Vec::new();
    for line in data.split(|&b| b == b'\n') {
        let t = trim_bytes(line);
        if !t.is_empty() {
            lines.push(t);
        }
    }
    if lines.len() < 2 {
        return false;
    }
    lines
        .iter()
        .all(|l| serde_json::from_slice::<serde_json::Value>(l).is_ok())
}

fn parse_error_message(data: &[u8]) -> String {
    let trimmed = trim_bytes(data);
    serde_json::from_slice::<serde_json::Value>(trimmed)
        .err()
        .map(|e| e.to_string())
        .unwrap_or_else(|| "invalid JSON".to_string())
}

pub fn status_for_tier(
    tier: JsonTier,
    info_size: u64,
    line: u64,
    line_count: u64,
    byte: u64,
    parse_error: Option<&str>,
) -> String {
    let pct = if info_size == 0 {
        100
    } else {
        ((byte as f64 / info_size as f64) * 100.0) as u32
    };
    match tier {
        JsonTier::SmallPretty => format!(
            "JSON pretty  line {}/{} @ {byte} / {info_size} B ({pct}%)",
            line + 1,
            line_count.max(1)
        ),
        JsonTier::Ndjson => format!(
            "JSON ndjson  line {}/{} @ {byte} / {info_size} B ({pct}%)",
            line + 1,
            line_count.max(1)
        ),
        JsonTier::LargeRaw => format!("JSON raw  {byte} / {info_size} B ({pct}%)"),
        JsonTier::InvalidRaw => {
            let hint = parse_error.unwrap_or("invalid JSON");
            format!("JSON invalid ({hint})  {byte} / {info_size} B ({pct}%)")
        }
    }
}

/// In-process cache keyed by path (one entry per opened file in the TUI).
#[derive(Default)]
pub struct JsonTierCache {
    entries: std::sync::RwLock<std::collections::HashMap<PathBuf, JsonFileCache>>,
}

impl JsonTierCache {
    pub fn get_or_build(&self, path: &Path, size: u64, data: &[u8]) -> JsonFileCache {
        if let Ok(guard) = self.entries.read()
            && let Some(c) = guard.get(path)
        {
            return c.clone();
        }
        let built = build_cache(path, size, data);
        if let Ok(mut guard) = self.entries.write() {
            guard.insert(path.to_path_buf(), built.clone());
        }
        built
    }

    #[cfg(test)]
    pub fn clear(&self) {
        if let Ok(mut guard) = self.entries.write() {
            guard.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_large_tier_by_size() {
        assert_eq!(
            detect_tier(SMALL_FILE_LIMIT + 1, br#"{"a":1}"#),
            JsonTier::LargeRaw
        );
    }

    #[test]
    fn detect_small_pretty_for_object() {
        assert_eq!(detect_tier(10, br#"{"a":1}"#), JsonTier::SmallPretty);
    }

    #[test]
    fn detect_ndjson_for_multiple_lines() {
        let data = b"{\"a\":1}\n{\"b\":2}\n";
        assert_eq!(detect_tier(data.len() as u64, data), JsonTier::Ndjson);
    }

    #[test]
    fn detect_invalid_for_garbage() {
        assert_eq!(detect_tier(5, b"not json"), JsonTier::InvalidRaw);
    }

    #[test]
    fn ndjson_preserves_source_line_byte_starts() {
        let data = b"{\"x\":1}\n{\"y\":2}\n";
        let cache = build_ndjson_cache(data);
        assert_eq!(cache.tier, JsonTier::Ndjson);
        assert_eq!(cache.line_start_bytes, vec![0, 8]);
        assert!(cache.display_lines[0].contains('x'));
    }

    #[test]
    fn small_pretty_produces_indented_lines() {
        let cache = build_small_pretty_cache(br#"{"k":1}"#).unwrap();
        assert_eq!(cache.tier, JsonTier::SmallPretty);
        assert!(cache.display_lines.len() > 1);
        assert!(cache.display_lines.iter().any(|l| l.contains("k")));
    }
}