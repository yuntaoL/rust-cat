//! rcat-tui — interactive terminal UI for rcat using Ratatui + crossterm.

mod metadata;
mod styling;
mod terminal;
mod theme;
mod viewport_cache;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use metadata::build_metadata_lines;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use rcat_core::file_info::FileInfo;
use rcat_core::{FileSession, PositionKind, ViewAnchor, ViewContext, ViewportResult};
use viewport_cache::{ViewportCache, ViewportCacheKey};
use std::io::{self, stdout};
use styling::render_content_lines;
use terminal::TerminalGuard;
use theme::Theme;
use tracing::{debug, trace};

/// Width of the metadata sidebar when visible.
const SIDEBAR_WIDTH: u16 = 28;

pub struct TuiConfig {
    /// Host-owned file session (mmap + metadata).
    pub session: FileSession,
    /// All registered viewers (built-in + plugins), in registration order.
    pub viewers: Vec<Box<dyn rcat_core::FileViewer>>,
    /// Index into `viewers` for the initially active viewer.
    pub initial_viewer_index: usize,
    pub initial_offset: u64,
}

pub fn run_tui(config: TuiConfig) -> Result<()> {
    let initial_name = config
        .viewers
        .get(config.initial_viewer_index)
        .map(|v| v.name())
        .unwrap_or("?");
    debug!(
        file = %config.session.path().display(),
        viewer = initial_name,
        viewer_count = config.viewers.len(),
        "starting TUI"
    );
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(
        config.session,
        config.viewers,
        config.initial_viewer_index,
        config.initial_offset,
    );

    if let Err(err) = run_app(&mut terminal, &mut app) {
        println!("{err:?}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rcat_core::file_info::FileInfo;
    /// A controllable viewer used only in tests.
    /// It returns a predictable grid of lines based on the requested offset and width.
    struct TestViewer {
        name: &'static str,
        total_lines: usize,
        render_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl TestViewer {
        fn new(name: &'static str, total_lines: usize) -> Self {
            Self {
                name,
                total_lines,
                render_calls: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            }
        }

        fn render_calls_handle(&self) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
            std::sync::Arc::clone(&self.render_calls)
        }
    }

    impl Default for TestViewer {
        fn default() -> Self {
            Self::new("Test", 100)
        }
    }

    impl rcat_core::FileViewer for TestViewer {
        fn position_kind(&self) -> rcat_core::PositionKind {
            rcat_core::PositionKind::DisplayLine
        }

        fn scroll_extent(&self, _info: &FileInfo) -> u64 {
            self.total_lines.saturating_sub(1).max(1) as u64
        }

        fn source_byte_for_anchor(&self, info: &FileInfo, anchor: ViewAnchor) -> Option<u64> {
            match anchor {
                ViewAnchor::Byte(b) => Some(b.min(info.size.saturating_sub(1))),
                ViewAnchor::DisplayLine(line) => {
                    let extent = self.scroll_extent(info);
                    Some(rcat_core::anchor_from_fraction(
                        rcat_core::scroll_fraction(line, extent),
                        info.size.saturating_sub(1),
                    ))
                }
                ViewAnchor::Frame(_) => None,
            }
        }

        fn display_line_for_byte(&self, info: &FileInfo, byte: u64) -> Option<u64> {
            let extent = self.scroll_extent(info);
            let file_extent = info.size.saturating_sub(1).max(1);
            Some(rcat_core::anchor_from_fraction(
                rcat_core::scroll_fraction(byte.min(file_extent), file_extent),
                extent,
            ))
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn can_handle(&self, _probe: &mut dyn rcat_core::FileProbe) -> rcat_core::ViewerPriority {
            rcat_core::ViewerPriority::Normal
        }

        fn dump(
            &self,
            _info: &FileInfo,
            _writer: &mut dyn std::io::Write,
            _opts: &rcat_core::dump::DumpOptions,
        ) -> std::io::Result<()> {
            Ok(())
        }

        fn render_lines(
            &self,
            _info: &FileInfo,
            start_offset: u64,
            max_rows: u16,
            _width: u16,
        ) -> Vec<String> {
            let start = start_offset as usize;
            (0..max_rows as usize)
                .map(|i| {
                    let line_num = start + i;
                    if line_num < self.total_lines {
                        format!("LINE {:03}", line_num)
                    } else {
                        "(end)".to_string()
                    }
                })
                .collect()
        }

        fn advance_lines(&self, _info: &FileInfo, current: u64, delta: i64, _width: u16) -> u64 {
            // Simple model: each "line" is one row for this test viewer
            let new_pos = (current as i64 + delta).max(0) as u64;
            new_pos.min((self.total_lines - 1) as u64)
        }

        fn status(&self, _info: &FileInfo, pos: u64) -> String {
            format!("Test @ {}", pos)
        }

        fn render_viewport(&self, ctx: &ViewContext) -> ViewportResult {
            self.render_calls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let anchor = ctx.anchor;
            let raw = ctx.anchor_raw();
            let lines = self.render_lines(ctx.info(), raw, ctx.max_rows, ctx.content_width);
            let status = self.status(ctx.info(), raw);
            ViewportResult {
                lines,
                status,
                anchor,
                source_byte: None,
            }
        }
    }

    fn make_test_session() -> FileSession {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, b"hello world\n").unwrap();
        FileSession::open(f.path()).unwrap()
    }

    #[test]
    fn viewport_cache_reuses_render_on_identical_request() {
        let session = make_test_session();
        let viewer = TestViewer::default();
        let calls = viewer.render_calls_handle();
        let mut app = App::new(session, vec![Box::new(viewer)], 0, 0);
        let _ = app.cached_viewport(80, 10);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        let _ = app.cached_viewport(80, 10);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "second identical request must hit cache"
        );
    }

    #[test]
    fn viewport_cache_miss_after_scroll() {
        let session = make_test_session();
        let viewer = TestViewer::default();
        let calls = viewer.render_calls_handle();
        let mut app = App::new(session, vec![Box::new(viewer)], 0, 0);
        let _ = app.cached_viewport(80, 10);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        app.apply(TuiAction::ScrollDown(1), 80);
        let _ = app.cached_viewport(80, 10);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            2,
            "scroll must invalidate cache"
        );
    }

    #[test]
    fn toggle_help_redraws_without_extra_viewport_render() {
        let session = make_test_session();
        let viewer = TestViewer::default();
        let calls = viewer.render_calls_handle();
        let mut app = App::new(session, vec![Box::new(viewer)], 0, 0);
        let _ = app.cached_viewport(80, 10);
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        app.apply(TuiAction::ToggleHelp, 80);
        assert!(app.needs_redraw());
        let _ = app.cached_viewport(80, 10);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            1,
            "help toggle should not invalidate viewport cache"
        );
    }

    #[test]
    fn app_new_and_offset() {
        let session = make_test_session();
        let app = App::new(session, vec![Box::new(TestViewer::default())], 0, 42);
        assert_eq!(app.offset(), 42);
    }

    #[test]
    fn apply_scroll_changes_offset() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::default())], 0, 5);

        app.apply(TuiAction::ScrollDown(3), 80);
        assert_eq!(app.offset(), 8);

        app.apply(TuiAction::ScrollUp(2), 80);
        assert_eq!(app.offset(), 6);
    }

    fn write_multi_line_json(path: &std::path::Path) {
        let mut obj = String::from("{");
        for i in 0..40 {
            if i > 0 {
                obj.push(',');
            }
            obj.push_str(&format!("\n  \"entry{i:02}\": {i}"));
        }
        obj.push_str("\n}\n");
        std::fs::write(path, obj).unwrap();
    }

    #[test]
    fn toggle_text_to_json_preserves_mid_file_position() {
        let f = tempfile::NamedTempFile::new().unwrap();
        write_multi_line_json(f.path());
        let raw = std::fs::read(f.path()).unwrap();
        let mid_byte = (raw.len() / 2) as u64;

        let session = FileSession::open(f.path()).unwrap();
        let mut app = App::new(
            session,
            vec![
                Box::new(rcat_viewers_text::TextViewer),
                Box::new(rcat_viewers_json::JsonViewerLogic),
            ],
            0,
            mid_byte,
        );

        assert_eq!(app.offset(), mid_byte);
        app.apply(TuiAction::ToggleViewer, 80);
        assert_eq!(app.active_viewer_name(), "JSON");
        assert_eq!(
            app.offset(),
            mid_byte,
            "JSON raw view must keep the same byte offset as Text"
        );
    }

    #[test]
    fn toggle_hex_to_json_preserves_mid_file_position() {
        let f = tempfile::NamedTempFile::new().unwrap();
        write_multi_line_json(f.path());
        let raw = std::fs::read(f.path()).unwrap();
        let mid_byte = (raw.len() / 2) as u64;

        let session = FileSession::open(f.path()).unwrap();
        let mut app = App::new(
            session,
            vec![
                Box::new(rcat_viewers_hex::HexViewer),
                Box::new(rcat_viewers_json::JsonViewerLogic),
            ],
            0,
            mid_byte,
        );

        app.apply(TuiAction::ToggleViewer, 80);
        assert_eq!(app.active_viewer_name(), "JSON");
        assert_eq!(
            app.offset(),
            mid_byte,
            "JSON raw view must keep the same byte offset as Hex"
        );
    }

    #[test]
    fn toggle_viewer_maps_display_line_fraction_to_byte_offset() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let data = vec![0u8; 1000];
        std::io::Write::write_all(&mut f, &data).unwrap();
        let session = FileSession::open(f.path()).unwrap();

        let mut app = App::new(
            session,
            vec![
                Box::new(TestViewer::new("Line", 100)),
                Box::new(rcat_viewers_hex::HexViewer),
            ],
            0,
            50,
        );

        app.apply(TuiAction::ToggleViewer, 80);
        let byte_pos = app.offset();
        // line 50 of 99 ≈ 50% → ~500 bytes in a 1000-byte file
        assert!(
            (450..=550).contains(&byte_pos),
            "expected ~50% byte offset, got {byte_pos}"
        );
    }

    #[test]
    fn toggle_viewer_cycles_and_preserves_offset() {
        let session = make_test_session();
        let mut app = App::new(
            session,
            vec![
                Box::new(TestViewer::new("A", 10)),
                Box::new(TestViewer::new("B", 10)),
            ],
            0,
            7,
        );
        assert_eq!(app.active_viewer_name(), "A");
        assert_eq!(app.offset(), 7);

        app.apply(TuiAction::ToggleViewer, 80);
        assert_eq!(app.active_viewer_name(), "B");
        assert_eq!(app.offset(), 7);

        app.apply(TuiAction::ToggleViewer, 80);
        assert_eq!(app.active_viewer_name(), "A");
    }

    #[test]
    fn render_app_produces_expected_lines() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 10))], 0, 2);

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_app(f, &mut app, 36);
            })
            .unwrap();

        let buffer = terminal.backend().buffer();

        // Check that some expected content appears
        let content = buffer
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();

        assert!(content.contains("LINE 002"));
        assert!(content.contains("LINE 003"));
    }

    #[test]
    fn go_to_end_and_start() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 50))], 0, 10);

        app.apply(TuiAction::GoToEnd, 80);
        assert_eq!(app.offset(), 49);

        app.apply(TuiAction::GoToStart, 80);
        assert_eq!(app.offset(), 0);
    }

    #[test]
    fn render_respects_content_width() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 5))], 0, 0);

        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_app(f, &mut app, 16); // narrow width
            })
            .unwrap();

        // Just verify it renders without panic and contains expected text
        let buffer = terminal.backend().buffer();
        let content: String = buffer.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("LINE 000"));
    }

    #[test]
    fn multiple_actions_sequence() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 100))], 0, 50);

        app.apply(TuiAction::ScrollDown(10), 80);
        app.apply(TuiAction::PageUp(20), 80);
        app.apply(TuiAction::ScrollUp(5), 80);

        // Should have moved around reasonably
        assert!(app.offset() < 60);
    }

    // ============================================================
    // Comprehensive navigation / scrolling action tests
    // ============================================================

    #[test]
    fn scroll_down_and_up_work() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 100))], 0, 10);

        app.apply(TuiAction::ScrollDown(5), 80);
        assert_eq!(app.offset(), 15);

        app.apply(TuiAction::ScrollUp(3), 80);
        assert_eq!(app.offset(), 12);
    }

    #[test]
    fn page_down_and_page_up_use_the_provided_size() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 200))], 0, 10);

        // Page size of 30
        app.apply(TuiAction::PageDown(30), 80);
        assert_eq!(app.offset(), 40);

        app.apply(TuiAction::PageUp(30), 80);
        assert_eq!(app.offset(), 10);
    }

    #[test]
    fn go_to_start_and_go_to_end() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 80))], 0, 35);

        app.apply(TuiAction::GoToStart, 80);
        assert_eq!(app.offset(), 0);

        app.apply(TuiAction::GoToEnd, 80);
        assert_eq!(app.offset(), 79); // clamped by TestViewer
    }

    #[test]
    fn navigation_clamps_at_boundaries() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 30))], 0, 5);

        // Can't go below 0
        app.apply(TuiAction::ScrollUp(20), 80);
        assert_eq!(app.offset(), 0);

        app.apply(TuiAction::PageUp(50), 80);
        assert_eq!(app.offset(), 0);

        // Can't go past the end (TestViewer clamps)
        app.apply(TuiAction::GoToEnd, 80);
        let end = app.offset();

        app.apply(TuiAction::ScrollDown(100), 80);
        assert_eq!(app.offset(), end);

        app.apply(TuiAction::PageDown(50), 80);
        assert_eq!(app.offset(), end);
    }

    #[test]
    fn large_page_down_from_near_start() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::new("Test", 1000))], 0, 3);

        // Big page jump
        app.apply(TuiAction::PageDown(100), 80);
        assert_eq!(app.offset(), 103);
    }

    #[test]
    fn toggle_metadata_flips_sidebar_flag() {
        let session = make_test_session();
        let mut app = App::new(session, vec![Box::new(TestViewer::default())], 0, 0);
        assert!(!app.show_metadata());
        app.apply(TuiAction::ToggleMetadata, 80);
        assert!(app.show_metadata());
        app.apply(TuiAction::ToggleMetadata, 80);
        assert!(!app.show_metadata());
    }

    #[test]
    fn render_with_metadata_sidebar_shows_mime() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, br#"{"a":1}"#).unwrap();
        let mut info = FileInfo::from_path(f.path()).unwrap();
        info.type_description = "JSON".to_string();
        info.extension = Some("json".to_string());
        info.detected.mime_type = Some("application/json".to_string());
        let session = FileSession::from_info(info).unwrap();
        let mut app = App::new(session, vec![Box::new(TestViewer::default())], 0, 0);
        app.apply(TuiAction::ToggleMetadata, 80);

        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_app(f, &mut app, 60)).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("MIME"));
        assert!(content.contains("application/json"));
    }
}

/// High-level actions the TUI can perform.
/// This makes the state machine explicit and easy to test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuiAction {
    Quit,
    ScrollDown(u16),
    ScrollUp(u16),
    PageDown(u16),
    PageUp(u16),
    GoToStart,
    GoToEnd,
    ToggleHelp,
    /// Cycle to the next registered viewer (Text → Hex → plugins, wrap).
    ToggleViewer,
    /// Toggle the metadata sidebar.
    ToggleMetadata,
}

struct App {
    session: FileSession,
    viewers: Vec<Box<dyn rcat_core::FileViewer>>,
    viewer_index: usize,
    anchor: ViewAnchor,
    /// Canonical source-file byte position for Text/Hex/JSON sync.
    byte_position: u64,
    show_help: bool,
    show_metadata: bool,
    theme: Theme,
    /// Last terminal size (detect resize → invalidate viewport cache).
    last_term_size: (u16, u16),
    /// Screen needs redraw (input, resize, or viewport invalidated).
    needs_redraw: bool,
    viewport_cache: ViewportCache,
}

impl App {
    /// Creates a new App. Useful for tests and the real TUI.
    pub fn new(
        session: FileSession,
        viewers: Vec<Box<dyn rcat_core::FileViewer>>,
        viewer_index: usize,
        offset: u64,
    ) -> Self {
        let viewer_index = if viewers.is_empty() {
            0
        } else {
            viewer_index.min(viewers.len() - 1)
        };
        let kind = viewers
            .get(viewer_index)
            .map(|v| v.position_kind())
            .unwrap_or(rcat_core::PositionKind::Byte);
        let byte_position = match kind {
            rcat_core::PositionKind::Byte => offset,
            _ => 0,
        };
        Self {
            session,
            viewers,
            viewer_index,
            anchor: ViewAnchor::from_raw(kind, offset),
            byte_position,
            show_help: false,
            show_metadata: false,
            theme: Theme::default(),
            last_term_size: (0, 0),
            needs_redraw: true,
            viewport_cache: ViewportCache::default(),
        }
    }

    fn invalidate_viewport_cache(&mut self) {
        self.viewport_cache.invalidate();
    }

    /// Returns a cached viewport or calls `render_viewport` on the active viewer.
    fn cached_viewport(&mut self, content_width: u16, max_rows: u16) -> &ViewportResult {
        let key = ViewportCacheKey {
            viewer_index: self.viewer_index,
            anchor_raw: self.anchor.raw(),
            content_width,
            max_rows,
        };
        if self.viewport_cache.get(key).is_none() {
            let ctx = ViewContext::new(&self.session, self.anchor, content_width, max_rows);
            let viewer = self.viewers[self.viewer_index].as_ref();
            let viewport = viewer.render_viewport(&ctx);
            self.viewport_cache.store(key, viewport);
        } else {
            trace!("viewport cache hit");
        }
        self.viewport_cache
            .get(key)
            .expect("cache populated above")
    }

    fn sync_byte_position(&mut self) {
        let viewer = &self.viewers[self.viewer_index];
        let info = self.session.info();
        if let Some(b) = viewer.source_byte_for_anchor(info, self.anchor) {
            self.byte_position = b;
        }
    }

    fn anchor_for_byte_in_viewer(&self, viewer_index: usize, byte: u64) -> ViewAnchor {
        let viewer = &self.viewers[viewer_index];
        let info = self.session.info();
        let byte = byte.min(self.session.size().saturating_sub(1));
        match viewer.position_kind() {
            rcat_core::PositionKind::Byte | rcat_core::PositionKind::Frame => {
                ViewAnchor::from_raw(viewer.position_kind(), byte)
            }
            rcat_core::PositionKind::DisplayLine => viewer
                .display_line_for_byte(info, byte)
                .map(ViewAnchor::DisplayLine)
                .unwrap_or_else(|| {
                    let extent = viewer.scroll_extent(info);
                    ViewAnchor::DisplayLine(rcat_core::anchor_from_fraction(
                        rcat_core::scroll_fraction(byte, self.session.size().saturating_sub(1)),
                        extent,
                    ))
                }),
        }
    }

    fn info(&self) -> &FileInfo {
        self.session.info()
    }

    fn scroll_ctx(&self, width: u16) -> ViewContext<'_> {
        ViewContext::new(&self.session, self.anchor, width, 1)
    }

    #[cfg(test)]
    fn show_metadata(&self) -> bool {
        self.show_metadata
    }

    #[cfg(test)]
    fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    fn active_viewer(&self) -> &dyn rcat_core::FileViewer {
        self.viewers[self.viewer_index].as_ref()
    }

    #[cfg(test)]
    fn active_viewer_name(&self) -> &str {
        self.active_viewer().name()
    }

    #[cfg(test)]
    pub fn offset(&self) -> u64 {
        self.anchor.raw()
    }

    /// Applies a high-level action to the app state.
    /// `width` is passed to width-aware viewers for correct scrolling inside wrapped lines.
    pub fn apply(&mut self, action: TuiAction, width: u16) {
        let old_anchor = self.anchor;
        let old_viewer = self.active_viewer().name().to_string();
        let mut invalidate_viewport = false;
        match action {
            TuiAction::Quit => {}
            TuiAction::ScrollDown(n) => {
                invalidate_viewport = true;
                self.anchor = self
                    .active_viewer()
                    .advance_anchor(&self.scroll_ctx(width), n as i64);
            }
            TuiAction::ScrollUp(n) => {
                invalidate_viewport = true;
                self.anchor = self
                    .active_viewer()
                    .advance_anchor(&self.scroll_ctx(width), -(n as i64));
            }
            TuiAction::PageDown(n) => {
                invalidate_viewport = true;
                self.anchor = self
                    .active_viewer()
                    .advance_anchor(&self.scroll_ctx(width), n as i64);
            }
            TuiAction::PageUp(n) => {
                invalidate_viewport = true;
                self.anchor = self
                    .active_viewer()
                    .advance_anchor(&self.scroll_ctx(width), -(n as i64));
            }
            TuiAction::GoToStart => {
                invalidate_viewport = true;
                let kind = self.active_viewer().position_kind();
                self.anchor = ViewAnchor::from_raw(kind, 0);
                self.byte_position = 0;
            }
            TuiAction::GoToEnd => {
                invalidate_viewport = true;
                let kind = self.active_viewer().position_kind();
                // Byte viewers: start from EOF. Line/frame viewers: start past the end
                // so advance_lines clamps to the last display row (same as legacy behavior).
                let jump = match kind {
                    PositionKind::Byte => self.session.size(),
                    PositionKind::DisplayLine | PositionKind::Frame => u64::MAX / 2,
                };
                let end_ctx =
                    ViewContext::new(&self.session, ViewAnchor::from_raw(kind, jump), width, 1);
                self.anchor = self.active_viewer().advance_anchor(&end_ctx, -8);
            }
            TuiAction::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            TuiAction::ToggleViewer => {
                invalidate_viewport = true;
                if self.viewers.len() > 1 {
                    self.sync_byte_position();

                    let new_index = (self.viewer_index + 1) % self.viewers.len();
                    let to_name = self.viewers[new_index].name();
                    self.viewer_index = new_index;
                    self.anchor = self.anchor_for_byte_in_viewer(new_index, self.byte_position);
                    debug!(
                        viewer = to_name,
                        anchor = self.anchor.raw(),
                        byte = self.byte_position,
                        "switched viewer (anchor mapped via source byte)"
                    );
                }
            }
            TuiAction::ToggleMetadata => {
                invalidate_viewport = true;
                self.show_metadata = !self.show_metadata;
            }
        }
        if invalidate_viewport {
            self.invalidate_viewport_cache();
        }
        if !matches!(action, TuiAction::Quit) {
            self.needs_redraw = true;
        }
        if self.anchor != old_anchor {
            self.sync_byte_position();
            trace!(
                old = old_anchor.raw(),
                new = self.anchor.raw(),
                byte = self.byte_position,
                "anchor changed"
            );
        }
        if self.active_viewer().name() != old_viewer.as_str() {
            trace!(
                old = %old_viewer,
                new = self.active_viewer().name(),
                "viewer changed"
            );
        }
    }
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        let term_size = terminal.size().unwrap_or_default();
        if term_size.width != app.last_term_size.0 || term_size.height != app.last_term_size.1 {
            app.last_term_size = (term_size.width, term_size.height);
            app.invalidate_viewport_cache();
            app.needs_redraw = true;
        }
        let usable_width = term_size.width.saturating_sub(4).max(20);
        // Compute a sensible page size based on terminal height.
        // Leave a small overlap (2 lines) so the user doesn't lose context, which is standard pager behavior.
        let page_size = term_size.height.saturating_sub(7).max(5);

        // Input: map raw keys → TuiAction → apply (much easier to test)
        if event::poll(std::time::Duration::from_millis(50))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            trace!(?key.code, "key pressed");

            // When help is open, most keys close the help overlay instead of quitting
            if app.show_help {
                match key.code {
                    KeyCode::Char('?') | KeyCode::Esc => {
                        app.show_help = false;
                        app.needs_redraw = true;
                        continue;
                    }
                    KeyCode::Char('q') => return Ok(()), // allow explicit quit even from help
                    _ => {
                        app.show_help = false; // any other key closes help
                        app.needs_redraw = true;
                        continue;
                    }
                }
            }

            let action = match key.code {
                KeyCode::Char('q') | KeyCode::Esc => TuiAction::Quit,
                KeyCode::Char('j') | KeyCode::Down => TuiAction::ScrollDown(1),
                KeyCode::Char('k') | KeyCode::Up => TuiAction::ScrollUp(1),
                KeyCode::PageDown => TuiAction::PageDown(page_size),
                KeyCode::PageUp => TuiAction::PageUp(page_size),
                KeyCode::Char('d')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    TuiAction::PageDown(page_size)
                }
                KeyCode::Char('u')
                    if key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    TuiAction::PageUp(page_size)
                }
                KeyCode::Char('g') | KeyCode::Home => TuiAction::GoToStart,
                KeyCode::Char('G') | KeyCode::End => TuiAction::GoToEnd,
                KeyCode::Char('?') => TuiAction::ToggleHelp,
                KeyCode::Tab | KeyCode::Char('h') => TuiAction::ToggleViewer,
                KeyCode::Char('m') => TuiAction::ToggleMetadata,
                _ => continue,
            };

            debug!(?action, "applying TUI action");
            if matches!(action, TuiAction::Quit) {
                return Ok(());
            }

            app.apply(action, usable_width);
        }

        if app.needs_redraw {
            terminal
                .draw(|f| {
                    render_app(f, app, usable_width);
                })
                .expect("failed to draw");
            app.needs_redraw = false;
        }
    }
}

/// Extracted pure rendering logic. This is the key enabler for testing the UI
/// with `ratatui::backend::TestBackend` without any real terminal or crossterm.
fn render_app(f: &mut ratatui::Frame, app: &mut App, usable_width: u16) {
    let theme = app.theme;
    let viewer_name = app.active_viewer().name();

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(f.area());

    let viewer_label = if app.viewers.len() > 1 {
        format!(
            "{} ({}/{})",
            viewer_name,
            app.viewer_index + 1,
            app.viewers.len()
        )
    } else {
        viewer_name.to_string()
    };
    let anchor_raw = app.anchor.raw();
    let scroll_extent = app.active_viewer().scroll_extent(app.info());
    let pos_label = metadata::format_header_position(
        app.active_viewer().position_kind(),
        anchor_raw,
        app.session.size(),
        scroll_extent,
    );
    let header_text = format!(
        " {}  ·  {}  ·  {}  ·  {pos_label}",
        app.session.path().display(),
        metadata::format_file_size(app.session.size()),
        viewer_label,
    );
    let header = Paragraph::new(header_text).style(theme.header).block(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(theme.border),
    );
    f.render_widget(header, vertical[0]);

    let main_area = vertical[1];
    let (content_area, content_width) = if app.show_metadata {
        let split = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(20), Constraint::Length(SIDEBAR_WIDTH)])
            .split(main_area);
        render_metadata_sidebar(f, split[1], app.info(), theme);
        let width = split[0].width.saturating_sub(4).max(8).min(usable_width);
        (split[0], width)
    } else {
        let width = main_area.width.saturating_sub(4).max(8).min(usable_width);
        (main_area, width)
    };

    let max_rows = content_area.height.saturating_sub(1).max(1);
    let (content_lines, viewer_status, source_byte) = {
        let viewport = app.cached_viewport(content_width, max_rows);
        (
            render_content_lines(viewer_name, viewport.lines.clone(), &theme),
            viewport.status.clone(),
            viewport.source_byte,
        )
    };

    let content = Paragraph::new(content_lines)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(theme.border),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(content, content_area);

    if let Some(b) = source_byte {
        app.byte_position = b;
    }
    let mut hints = vec!["q quit", "m meta", "? help"];
    if app.viewers.len() > 1 {
        hints.insert(1, "Tab/h viewer");
    }
    // Viewer status already has position (dec for Text, hex+dec for Hex, lines for JSON).
    let footer_text = format!(" {viewer_status}  │  {}", hints.join("  "));
    let footer = Paragraph::new(footer_text).style(theme.footer).block(
        Block::default()
            .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
            .border_style(theme.border),
    );
    f.render_widget(footer, vertical[2]);

    if app.show_help {
        render_help_overlay(f, app, theme);
    }
}

fn render_metadata_sidebar(f: &mut ratatui::Frame, area: Rect, info: &FileInfo, theme: Theme) {
    let lines = build_metadata_lines(info, &theme);
    let sidebar = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Info ")
                .borders(Borders::ALL)
                .border_style(theme.border),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(sidebar, area);
}

fn render_help_overlay(f: &mut ratatui::Frame, app: &App, theme: Theme) {
    let mut help_text = vec![
        Line::from("rcat — Keyboard Shortcuts"),
        Line::from(""),
        Line::from("q / Esc     Quit"),
        Line::from("j / ↓       Scroll down"),
        Line::from("k / ↑       Scroll up"),
        Line::from("Ctrl-d      Page down"),
        Line::from("Ctrl-u      Page up"),
        Line::from("PgUp/PgDn   Page up / down"),
        Line::from("g / Home    Go to start"),
        Line::from("G / End     Go to end"),
        Line::from("m           Toggle metadata sidebar"),
    ];
    if app.viewers.len() > 1 {
        let names: Vec<_> = app.viewers.iter().map(|v| v.name()).collect();
        help_text.push(Line::from(format!(
            "Tab / h     Cycle viewer ({})",
            names.join(" → ")
        )));
    }
    help_text.extend([
        Line::from("?           Toggle this help"),
        Line::from(""),
        Line::from("Esc / ?     Close help"),
    ]);

    let help = Paragraph::new(help_text)
        .block(
            Block::default()
                .title("Help")
                .borders(Borders::ALL)
                .border_style(theme.help_border),
        )
        .style(theme.help_body);

    let help_area = centered_rect(50, 60, f.area());
    f.render_widget(Clear, help_area);
    f.render_widget(help, help_area);
}

/// Helper to create a centered rect (for overlays like Help)
fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
