//! JSON viewer — tiered display with safe fallbacks.
//!
//! - **Small** valid JSON: pretty-printed lines (cached per path).
//! - **NDJSON**: one formatted value per source line.
//! - **Large** or **invalid**: raw file bytes + syntax highlighting in the TUI.
//!
//! Scroll anchors stay **byte-based** so Text/Hex/JSON stay in sync when toggling viewers.

mod tiers;

use std::io::Write;

pub use tiers::{JsonFileCache, JsonTier, JsonTierCache, SMALL_FILE_LIMIT, detect_tier};

use rcat_core::dump::{self, DumpOptions};
use rcat_core::file_info::FileInfo;
use rcat_core::probe::FileProbe;
use rcat_core::view::{PositionKind, ViewAnchor, ViewContext, ViewportResult};
use rcat_core::viewer::{FileViewer, ViewerPriority};
use rcat_viewers_text::TextViewer;

const INNER: TextViewer = TextViewer;

/// JSON viewer with per-path tier cache (pretty / NDJSON / raw).
#[derive(Default)]
pub struct JsonViewerLogic {
    cache: JsonTierCache,
}

impl JsonViewerLogic {
    fn cache_for_session(&self, ctx: &ViewContext) -> JsonFileCache {
        let path = ctx.session.info().path.clone();
        let size = ctx.session.size();
        let data = ctx.session.bytes();
        self.cache.get_or_build(&path, size, data)
    }

    fn cache_for_info(&self, info: &FileInfo) -> JsonFileCache {
        let session = rcat_core::FileSession::from_info(info.clone())
            .expect("JSON viewer needs readable file backing");
        let data = session.bytes();
        self.cache
            .get_or_build(&info.path, info.size, data)
    }

    fn render_raw_viewport(&self, ctx: &ViewContext) -> ViewportResult {
        let anchor = ctx.anchor;
        let raw = ctx.anchor_raw();
        let data = ctx.session.bytes();
        let size = ctx.session.size();
        let lines = rcat_viewers_text::text_slice::render_display_rows(
            data,
            size,
            raw,
            ctx.max_rows,
            ctx.content_width,
        );
        let status = tiers::status_for_tier(
            JsonTier::LargeRaw,
            size,
            0,
            1,
            raw,
            None,
        );
        ViewportResult {
            lines,
            status,
            anchor,
            source_byte: Some(raw.min(size.saturating_sub(1))),
        }
    }

    fn render_cached_viewport(&self, ctx: &ViewContext, file: &JsonFileCache) -> ViewportResult {
        let anchor = ctx.anchor;
        let byte = ctx.anchor_raw().min(ctx.session.size().saturating_sub(1));
        let line = file.line_index_for_byte(byte);
        let lines = file.viewport_lines(line, ctx.max_rows);
        let status = tiers::status_for_tier(
            file.tier,
            ctx.session.size(),
            line,
            file.line_count(),
            byte,
            file.parse_error.as_deref(),
        );
        let source_byte = file.byte_for_line_index(line);
        ViewportResult {
            lines,
            status,
            anchor,
            source_byte: Some(source_byte.min(ctx.session.size().saturating_sub(1))),
        }
    }

    fn status_byte(info: &FileInfo, pos: u64) -> String {
        let pct = if info.size == 0 {
            100
        } else {
            ((pos as f64 / info.size as f64) * 100.0) as u32
        };
        format!("JSON raw  {pos} / {} B ({pct}%)", info.size)
    }
}

impl FileViewer for JsonViewerLogic {
    fn name(&self) -> &'static str {
        "JSON"
    }

    fn position_kind(&self) -> PositionKind {
        PositionKind::Byte
    }

    fn can_handle(&self, probe: &mut dyn FileProbe) -> ViewerPriority {
        let prelim = probe.preliminary();
        if prelim.extension.as_deref() == Some("json")
            || prelim.mime_type.as_deref() == Some("application/json")
        {
            ViewerPriority::Preferred
        } else {
            ViewerPriority::None
        }
    }

    fn render_viewport(&self, ctx: &ViewContext) -> ViewportResult {
        let file = self.cache_for_session(ctx);
        match file.tier {
            JsonTier::LargeRaw => self.render_raw_viewport(ctx),
            JsonTier::InvalidRaw => {
                let mut vp = self.render_raw_viewport(ctx);
                vp.status = tiers::status_for_tier(
                    JsonTier::InvalidRaw,
                    ctx.session.size(),
                    0,
                    1,
                    ctx.anchor_raw(),
                    file.parse_error.as_deref(),
                );
                vp
            }
            JsonTier::SmallPretty | JsonTier::Ndjson => self.render_cached_viewport(ctx, &file),
        }
    }

    fn advance_anchor(&self, ctx: &ViewContext, delta: i64) -> ViewAnchor {
        let file = self.cache_for_session(ctx);
        match file.tier {
            JsonTier::LargeRaw | JsonTier::InvalidRaw => {
                let raw = rcat_viewers_text::text_slice::advance_lines_bytes(
                    ctx.session.bytes(),
                    ctx.session.size(),
                    ctx.anchor_raw(),
                    delta,
                    ctx.content_width,
                );
                ViewAnchor::Byte(raw)
            }
            JsonTier::SmallPretty | JsonTier::Ndjson => {
                let line = file.line_index_for_byte(ctx.anchor_raw());
                let new_line = file.advance_line(line, delta);
                ViewAnchor::Byte(file.byte_for_line_index(new_line))
            }
        }
    }

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        width: u16,
    ) -> Vec<String> {
        let file = self.cache_for_info(info);
        match file.tier {
            JsonTier::LargeRaw | JsonTier::InvalidRaw => {
                INNER.render_lines(info, start_offset, max_rows, width)
            }
            JsonTier::SmallPretty | JsonTier::Ndjson => {
                let line = file.line_index_for_byte(start_offset);
                file.viewport_lines(line, max_rows)
            }
        }
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, width: u16) -> u64 {
        let file = self.cache_for_info(info);
        match file.tier {
            JsonTier::LargeRaw | JsonTier::InvalidRaw => {
                INNER.advance_lines(info, current, delta, width)
            }
            JsonTier::SmallPretty | JsonTier::Ndjson => {
                let line = file.line_index_for_byte(current);
                file.byte_for_line_index(file.advance_line(line, delta))
            }
        }
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        let file = self.cache_for_info(info);
        match file.tier {
            JsonTier::LargeRaw => Self::status_byte(info, pos),
            JsonTier::InvalidRaw => tiers::status_for_tier(
                JsonTier::InvalidRaw,
                info.size,
                0,
                1,
                pos,
                file.parse_error.as_deref(),
            ),
            JsonTier::SmallPretty | JsonTier::Ndjson => {
                let line = file.line_index_for_byte(pos);
                tiers::status_for_tier(
                    file.tier,
                    info.size,
                    line,
                    file.line_count(),
                    pos,
                    None,
                )
            }
        }
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        let file = self.cache_for_info(info);
        match file.tier {
            JsonTier::SmallPretty | JsonTier::Ndjson => {
                for line in &file.display_lines {
                    writeln!(writer, "{line}")?;
                }
                Ok(())
            }
            JsonTier::LargeRaw | JsonTier::InvalidRaw => dump::dump_text(info, writer, opts),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Keys must appear in file order in **raw** tier — never serde re-serialize for large files.
    #[test]
    fn raw_tier_preserves_key_order_on_first_line() {
        let mut f = NamedTempFile::new().unwrap();
        let mut data = vec![b' '];
        data.extend_from_slice(br#"{"z_first":1,"a_second":2}"#);
        data.resize((SMALL_FILE_LIMIT + 1) as usize, b'\n');
        f.write_all(&data).unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let file = logic.cache_for_info(&info);
        assert_eq!(file.tier, JsonTier::LargeRaw);
        let lines = logic.render_lines(&info, 0, 5, 80);
        let joined = lines.join("\n");
        let z_pos = joined.find("z_first").expect("z_first");
        let a_pos = joined.find("a_second").expect("a_second");
        assert!(z_pos < a_pos, "file order must be preserved; got:\n{joined}");
    }

    #[test]
    fn small_file_uses_pretty_tier() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"k":1}}"#).unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let file = logic.cache_for_info(&info);
        assert_eq!(file.tier, JsonTier::SmallPretty);
        assert!(file.display_lines.len() > 1);
    }

    #[test]
    fn can_handle_prefers_json_extension_from_probe() {
        use rcat_core::probe::{FileProbeWithInfo, PrefixProbe};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.json");
        std::fs::write(&path, br#"{"k":1}"#).unwrap();
        let info = FileInfo::from_path(&path).unwrap();
        let prefix = PrefixProbe::from_path(&path).unwrap();
        let mut probe = FileProbeWithInfo::new(&info, prefix);
        let logic = JsonViewerLogic::default();
        assert_eq!(logic.can_handle(&mut probe), ViewerPriority::Preferred);
    }

    #[test]
    fn uses_byte_position_kind() {
        let logic = JsonViewerLogic::default();
        assert_eq!(logic.position_kind(), PositionKind::Byte);
    }

    #[test]
    fn render_viewport_small_pretty_shows_formatted_json() {
        use rcat_core::session::FileSession;
        use rcat_core::view::ViewContext;

        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"only":true}}"#).unwrap();
        let session = FileSession::open(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let ctx = ViewContext::at_byte(&session, 0, 80, 10);
        let vp = logic.render_viewport(&ctx);
        assert!(vp.lines.iter().any(|l| l.contains("only")));
        assert!(vp.status.contains("pretty"));
        assert_eq!(vp.source_byte, Some(0));
    }

    #[test]
    fn render_viewport_large_raw_uses_bytes() {
        use rcat_core::session::FileSession;
        use rcat_core::view::ViewContext;

        let mut f = NamedTempFile::new().unwrap();
        let mut data = br#"{"only":true}"#.to_vec();
        data.resize((SMALL_FILE_LIMIT + 1) as usize, b'\n');
        f.write_all(&data).unwrap();
        let session = FileSession::open(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let ctx = ViewContext::at_byte(&session, 0, 80, 5);
        let vp = logic.render_viewport(&ctx);
        assert!(vp.lines.iter().any(|l| l.contains("only")));
        assert!(vp.status.contains("raw"));
    }

    #[test]
    fn advance_and_status_use_bytes() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "line0\nline1\nline2\n").unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let pos = logic.advance_lines(&info, 0, 1, 80);
        assert!(pos > 0);
        let status = logic.status(&info, pos);
        assert!(status.contains("ndjson") || status.contains("invalid") || status.contains("raw"));
    }
}