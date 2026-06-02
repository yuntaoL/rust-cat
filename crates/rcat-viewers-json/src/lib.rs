//! JSON viewer — raw file bytes with syntax highlighting (no parse/reformat).
//!
//! Tier detection ([`JsonTier`]) is kept for status hints and future opt-in pretty
//! paths. The **interactive TUI always shows on-disk bytes** so key order and
//! byte offsets stay aligned with Text and Hex (M1 contract).

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

/// JSON viewer: raw bytes in the TUI; tier metadata for invalid-file hints only.
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

    fn render_raw_viewport(&self, ctx: &ViewContext, parse_error: Option<&str>) -> ViewportResult {
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
        let status = if let Some(err) = parse_error {
            tiers::status_for_tier(JsonTier::InvalidRaw, size, 0, 1, raw, Some(err))
        } else {
            Self::status_byte(ctx.info(), raw)
        };
        ViewportResult {
            lines,
            status,
            anchor,
            source_byte: Some(raw.min(size.saturating_sub(1))),
        }
    }

    fn status_byte(info: &FileInfo, pos: u64) -> String {
        let pct = if info.size == 0 {
            100
        } else {
            ((pos as f64 / info.size as f64) * 100.0) as u32
        };
        format!("JSON  {pos} / {} B ({pct}%)", info.size)
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
        let parse_error = match file.tier {
            JsonTier::InvalidRaw => file.parse_error.as_deref(),
            _ => None,
        };
        self.render_raw_viewport(ctx, parse_error)
    }

    fn advance_anchor(&self, ctx: &ViewContext, delta: i64) -> ViewAnchor {
        let raw = rcat_viewers_text::text_slice::advance_lines_bytes(
            ctx.session.bytes(),
            ctx.session.size(),
            ctx.anchor_raw(),
            delta,
            ctx.content_width,
        );
        ViewAnchor::Byte(raw)
    }

    fn render_lines(
        &self,
        info: &FileInfo,
        start_offset: u64,
        max_rows: u16,
        width: u16,
    ) -> Vec<String> {
        INNER.render_lines(info, start_offset, max_rows, width)
    }

    fn advance_lines(&self, info: &FileInfo, current: u64, delta: i64, width: u16) -> u64 {
        INNER.advance_lines(info, current, delta, width)
    }

    fn status(&self, info: &FileInfo, pos: u64) -> String {
        let file = self.cache.get_or_build(
            &info.path,
            info.size,
            rcat_core::FileSession::from_info(info.clone())
                .expect("JSON viewer needs readable file backing")
                .bytes(),
        );
        match file.tier {
            JsonTier::InvalidRaw => tiers::status_for_tier(
                JsonTier::InvalidRaw,
                info.size,
                0,
                1,
                pos,
                file.parse_error.as_deref(),
            ),
            _ => Self::status_byte(info, pos),
        }
    }

    fn dump(
        &self,
        info: &FileInfo,
        writer: &mut dyn Write,
        opts: &DumpOptions,
    ) -> std::io::Result<()> {
        dump::dump_text(info, writer, opts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn raw_view_preserves_key_order_on_first_line() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"z_first":1,"a_second":2}}"#).unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let lines = logic.render_lines(&info, 0, 5, 80);
        let joined = lines.join("\n");
        let z_pos = joined.find("z_first").expect("z_first");
        let a_pos = joined.find("a_second").expect("a_second");
        assert!(
            z_pos < a_pos,
            "file order must be preserved; got:\n{joined}"
        );
    }

    #[test]
    fn detect_may_classify_small_json_but_viewport_stays_raw() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"k":1}}"#).unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let session = rcat_core::FileSession::from_info(info.clone()).unwrap();
        assert_eq!(
            detect_tier(info.size, session.bytes()),
            JsonTier::SmallPretty
        );
        let lines = logic.render_lines(&info, 0, 5, 80);
        assert!(
            lines.join("\n").contains(r#"{"k":1}"#),
            "viewport must show raw bytes, not pretty-print"
        );
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
    fn render_viewport_preserves_raw_bytes_and_source_byte() {
        use rcat_core::session::FileSession;
        use rcat_core::view::ViewContext;

        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"z":1,"a":2}}"#).unwrap();
        let session = FileSession::open(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let ctx = ViewContext::at_byte(&session, 0, 80, 5);
        let vp = logic.render_viewport(&ctx);
        let joined = vp.lines.join("\n");
        let z = joined.find("z").expect("z");
        let a = joined.find("a").expect("a");
        assert!(z < a, "key order in viewport: {joined}");
        assert!(vp.status.starts_with("JSON  "));
        assert_eq!(vp.source_byte, Some(0));
    }

    #[test]
    fn advance_viewport_uses_byte_offsets() {
        use rcat_core::session::FileSession;
        use rcat_core::view::ViewContext;

        let mut f = NamedTempFile::new().unwrap();
        write!(f, "line0\nline1\nline2\n").unwrap();
        let session = FileSession::open(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let ctx = ViewContext::at_byte(&session, 0, 80, 1);
        let next = logic.advance_anchor(&ctx, 1);
        assert!(matches!(next, ViewAnchor::Byte(b) if b > 0));
    }

    #[test]
    fn advance_and_status_use_bytes() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "alpha\nbeta\ngamma\n").unwrap();
        let info = FileInfo::from_path(f.path()).unwrap();
        let logic = JsonViewerLogic::default();
        let pos = logic.advance_lines(&info, 0, 1, 80);
        assert!(pos > 0);
        let status = logic.status(&info, pos);
        assert!(status.contains("JSON"));
        assert!(status.contains('/'));
    }
}