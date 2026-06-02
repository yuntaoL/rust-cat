//! Cached viewport results so idle TUI frames do not re-invoke viewers/plugins.

use rcat_core::ViewportResult;
use tracing::trace;

/// Cache key: everything that affects `render_viewport` output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ViewportCacheKey {
    pub viewer_index: usize,
    pub anchor_raw: u64,
    pub content_width: u16,
    pub max_rows: u16,
}

#[derive(Debug, Default)]
pub(crate) struct ViewportCache {
    key: Option<ViewportCacheKey>,
    viewport: Option<ViewportResult>,
}

impl ViewportCache {
    pub fn invalidate(&mut self) {
        self.key = None;
        self.viewport = None;
    }

    pub fn get(&self, key: ViewportCacheKey) -> Option<&ViewportResult> {
        if self.key == Some(key) {
            self.viewport.as_ref()
        } else {
            None
        }
    }

    pub fn store(&mut self, key: ViewportCacheKey, viewport: ViewportResult) {
        trace!(
            viewer_index = key.viewer_index,
            anchor = key.anchor_raw,
            width = key.content_width,
            rows = key.max_rows,
            "viewport cache store"
        );
        self.key = Some(key);
        self.viewport = Some(viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcat_core::{ViewAnchor, ViewportResult};

    fn sample_viewport() -> ViewportResult {
        ViewportResult {
            lines: vec!["line".to_string()],
            status: "ok".to_string(),
            anchor: ViewAnchor::Byte(0),
            source_byte: Some(0),
        }
    }

    #[test]
    fn invalidate_clears_entry() {
        let mut cache = ViewportCache::default();
        let key = ViewportCacheKey {
            viewer_index: 0,
            anchor_raw: 0,
            content_width: 80,
            max_rows: 10,
        };
        cache.store(key, sample_viewport());
        assert!(cache.get(key).is_some());
        cache.invalidate();
        assert!(cache.get(key).is_none());
    }

    #[test]
    fn different_key_is_cache_miss() {
        let mut cache = ViewportCache::default();
        let k1 = ViewportCacheKey {
            viewer_index: 0,
            anchor_raw: 0,
            content_width: 80,
            max_rows: 10,
        };
        let k2 = ViewportCacheKey {
            viewer_index: 0,
            anchor_raw: 1,
            content_width: 80,
            max_rows: 10,
        };
        cache.store(k1, sample_viewport());
        assert!(cache.get(k2).is_none());
    }
}