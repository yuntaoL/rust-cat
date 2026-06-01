//! rcat-tui — interactive terminal UI for rcat using Ratatui + crossterm.

mod metadata;
mod styling;
mod terminal;
mod theme;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use metadata::{build_metadata_lines, position_percent};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use rcat_core::file_info::FileInfo;
use std::io::{self, stdout};
use styling::render_content_lines;
use terminal::TerminalGuard;
use theme::Theme;
use tracing::{debug, trace};

/// Width of the metadata sidebar when visible.
const SIDEBAR_WIDTH: u16 = 28;

pub struct TuiConfig {
    pub info: FileInfo,
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
        file = %config.info.path.display(),
        viewer = initial_name,
        viewer_count = config.viewers.len(),
        "starting TUI"
    );
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(
        config.info,
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
    use std::path::PathBuf;

    /// A controllable viewer used only in tests.
    /// It returns a predictable grid of lines based on the requested offset and width.
    struct TestViewer {
        name: &'static str,
        total_lines: usize,
    }

    impl TestViewer {
        fn new(name: &'static str, total_lines: usize) -> Self {
            Self { name, total_lines }
        }
    }

    impl Default for TestViewer {
        fn default() -> Self {
            Self::new("Test", 100)
        }
    }

    impl rcat_core::FileViewer for TestViewer {
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
    }

    fn make_test_file_info() -> FileInfo {
        // We only need a few fields for the viewer methods above
        FileInfo {
            path: PathBuf::from("/tmp/test.txt"),
            absolute_path: None,
            size: 1000,
            kind: rcat_core::file_info::ContentKind::Text,
            type_description: "text".to_string(),
            extension: Some("txt".to_string()),
            detected: rcat_core::detection::PreliminaryDetection::default(),
            backing: None,
        }
    }

    #[test]
    fn app_new_and_offset() {
        let info = make_test_file_info();
        let app = App::new(info, vec![Box::new(TestViewer::default())], 0, 42);
        assert_eq!(app.offset(), 42);
    }

    #[test]
    fn apply_scroll_changes_offset() {
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::default())], 0, 5);

        app.apply(TuiAction::ScrollDown(3), 80);
        assert_eq!(app.offset(), 8);

        app.apply(TuiAction::ScrollUp(2), 80);
        assert_eq!(app.offset(), 6);
    }

    #[test]
    fn toggle_viewer_cycles_and_preserves_offset() {
        let info = make_test_file_info();
        let mut app = App::new(
            info,
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
        let info = make_test_file_info();
        let app = App::new(info, vec![Box::new(TestViewer::new("Test", 10))], 0, 2);

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_app(f, &app, 36);
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
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::new("Test", 50))], 0, 10);

        app.apply(TuiAction::GoToEnd, 80);
        assert_eq!(app.offset(), 49);

        app.apply(TuiAction::GoToStart, 80);
        assert_eq!(app.offset(), 0);
    }

    #[test]
    fn render_respects_content_width() {
        let info = make_test_file_info();
        let app = App::new(info, vec![Box::new(TestViewer::new("Test", 5))], 0, 0);

        let backend = TestBackend::new(20, 6);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal
            .draw(|f| {
                render_app(f, &app, 16); // narrow width
            })
            .unwrap();

        // Just verify it renders without panic and contains expected text
        let buffer = terminal.backend().buffer();
        let content: String = buffer.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("LINE 000"));
    }

    #[test]
    fn multiple_actions_sequence() {
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::new("Test", 100))], 0, 50);

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
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::new("Test", 100))], 0, 10);

        app.apply(TuiAction::ScrollDown(5), 80);
        assert_eq!(app.offset(), 15);

        app.apply(TuiAction::ScrollUp(3), 80);
        assert_eq!(app.offset(), 12);
    }

    #[test]
    fn page_down_and_page_up_use_the_provided_size() {
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::new("Test", 200))], 0, 10);

        // Page size of 30
        app.apply(TuiAction::PageDown(30), 80);
        assert_eq!(app.offset(), 40);

        app.apply(TuiAction::PageUp(30), 80);
        assert_eq!(app.offset(), 10);
    }

    #[test]
    fn go_to_start_and_go_to_end() {
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::new("Test", 80))], 0, 35);

        app.apply(TuiAction::GoToStart, 80);
        assert_eq!(app.offset(), 0);

        app.apply(TuiAction::GoToEnd, 80);
        assert_eq!(app.offset(), 79); // clamped by TestViewer
    }

    #[test]
    fn navigation_clamps_at_boundaries() {
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::new("Test", 30))], 0, 5);

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
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::new("Test", 1000))], 0, 3);

        // Big page jump
        app.apply(TuiAction::PageDown(100), 80);
        assert_eq!(app.offset(), 103);
    }

    #[test]
    fn toggle_metadata_flips_sidebar_flag() {
        let info = make_test_file_info();
        let mut app = App::new(info, vec![Box::new(TestViewer::default())], 0, 0);
        assert!(!app.show_metadata());
        app.apply(TuiAction::ToggleMetadata, 80);
        assert!(app.show_metadata());
        app.apply(TuiAction::ToggleMetadata, 80);
        assert!(!app.show_metadata());
    }

    #[test]
    fn render_with_metadata_sidebar_shows_mime() {
        let info = FileInfo {
            path: PathBuf::from("/tmp/test.json"),
            absolute_path: None,
            size: 100,
            kind: rcat_core::file_info::ContentKind::Text,
            type_description: "JSON".to_string(),
            extension: Some("json".to_string()),
            detected: rcat_core::detection::PreliminaryDetection {
                mime_type: Some("application/json".to_string()),
                ..Default::default()
            },
            backing: None,
        };
        let mut app = App::new(info, vec![Box::new(TestViewer::default())], 0, 0);
        app.apply(TuiAction::ToggleMetadata, 80);

        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render_app(f, &app, 60)).unwrap();
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
    info: FileInfo,
    viewers: Vec<Box<dyn rcat_core::FileViewer>>,
    viewer_index: usize,
    offset: u64,
    show_help: bool,
    show_metadata: bool,
    theme: Theme,
}

impl App {
    /// Creates a new App. Useful for tests and the real TUI.
    pub fn new(
        info: FileInfo,
        viewers: Vec<Box<dyn rcat_core::FileViewer>>,
        viewer_index: usize,
        offset: u64,
    ) -> Self {
        let viewer_index = if viewers.is_empty() {
            0
        } else {
            viewer_index.min(viewers.len() - 1)
        };
        Self {
            info,
            viewers,
            viewer_index,
            offset,
            show_help: false,
            show_metadata: false,
            theme: Theme::default(),
        }
    }

    #[cfg(test)]
    fn show_metadata(&self) -> bool {
        self.show_metadata
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
        self.offset
    }

    /// Applies a high-level action to the app state.
    /// `width` is passed to width-aware viewers for correct scrolling inside wrapped lines.
    pub fn apply(&mut self, action: TuiAction, width: u16) {
        let old_offset = self.offset;
        let old_viewer = self.active_viewer().name().to_string();
        match action {
            TuiAction::Quit => {}
            TuiAction::ScrollDown(n) => {
                self.offset =
                    self.active_viewer()
                        .advance_lines(&self.info, self.offset, n as i64, width);
            }
            TuiAction::ScrollUp(n) => {
                self.offset =
                    self.active_viewer()
                        .advance_lines(&self.info, self.offset, -(n as i64), width);
            }
            TuiAction::PageDown(n) => {
                self.offset =
                    self.active_viewer()
                        .advance_lines(&self.info, self.offset, n as i64, width);
            }
            TuiAction::PageUp(n) => {
                self.offset =
                    self.active_viewer()
                        .advance_lines(&self.info, self.offset, -(n as i64), width);
            }
            TuiAction::GoToStart => self.offset = 0,
            TuiAction::GoToEnd => {
                self.offset =
                    self.active_viewer()
                        .advance_lines(&self.info, self.info.size, -8, width);
            }
            TuiAction::ToggleHelp => {
                self.show_help = !self.show_help;
            }
            TuiAction::ToggleViewer => {
                if self.viewers.len() > 1 {
                    self.viewer_index = (self.viewer_index + 1) % self.viewers.len();
                    debug!(
                        viewer = self.active_viewer().name(),
                        offset = self.offset,
                        "switched viewer (offset preserved)"
                    );
                }
            }
            TuiAction::ToggleMetadata => {
                self.show_metadata = !self.show_metadata;
            }
        }
        if self.offset != old_offset {
            trace!(old = old_offset, new = self.offset, "offset changed");
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
        let usable_width = term_size.width.saturating_sub(4).max(20);
        // Compute a sensible page size based on terminal height.
        // Leave a small overlap (2 lines) so the user doesn't lose context, which is standard pager behavior.
        let page_size = term_size.height.saturating_sub(7).max(5);

        terminal
            .draw(|f| {
                render_app(f, app, usable_width);
            })
            .expect("failed to draw");

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
                        continue;
                    }
                    KeyCode::Char('q') => return Ok(()), // allow explicit quit even from help
                    _ => {
                        app.show_help = false; // any other key closes help
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
    }
}

/// Extracted pure rendering logic. This is the key enabler for testing the UI
/// with `ratatui::backend::TestBackend` without any real terminal or crossterm.
fn render_app(f: &mut ratatui::Frame, app: &App, usable_width: u16) {
    let theme = app.theme;
    let viewer = app.active_viewer();

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
            viewer.name(),
            app.viewer_index + 1,
            app.viewers.len()
        )
    } else {
        viewer.name().to_string()
    };
    let pct = position_percent(app.offset, app.info.size);
    let header_text = format!(
        " {}  ·  {}  ·  {}  ·  offset 0x{:X} ({pct}%)",
        app.info.path.display(),
        metadata::format_file_size(app.info.size),
        viewer_label,
        app.offset
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
        render_metadata_sidebar(f, split[1], &app.info, theme);
        let width = split[0].width.saturating_sub(4).max(8).min(usable_width);
        (split[0], width)
    } else {
        let width = main_area.width.saturating_sub(4).max(8).min(usable_width);
        (main_area, width)
    };

    let max_rows = content_area.height.saturating_sub(1).max(1);
    let raw_lines = viewer.render_lines(&app.info, app.offset, max_rows, content_width);
    let content_lines = render_content_lines(viewer.name(), raw_lines, &theme);

    let content = Paragraph::new(content_lines)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(theme.border),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(content, content_area);

    let viewer_status = viewer.status(&app.info, app.offset);
    let pct = position_percent(app.offset, app.info.size);
    let mut hints = vec!["q quit", "m meta", "? help"];
    if app.viewers.len() > 1 {
        hints.insert(1, "Tab/h viewer");
    }
    let footer_text = format!(
        " {viewer_status}  ·  0x{:X} ({pct}%)  │  {}",
        app.offset,
        hints.join("  ")
    );
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
