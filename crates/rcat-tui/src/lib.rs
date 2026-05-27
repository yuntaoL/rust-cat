//! rcat-tui — interactive terminal UI for rcat using Ratatui + crossterm.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use rcat_core::file_info::FileInfo;
use std::io::{self, stdout};

pub struct TuiConfig {
    pub info: FileInfo,
    pub viewer: Box<dyn rcat_core::FileViewer>,
    pub initial_offset: u64,
}

pub fn run_tui(config: TuiConfig) -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App {
        info: config.info,
        viewer: config.viewer,
        offset: config.initial_offset,
    };

    let res = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
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
        total_lines: usize,
    }

    impl Default for TestViewer {
        fn default() -> Self {
            Self { total_lines: 100 }
        }
    }

    impl rcat_core::FileViewer for TestViewer {
        fn name(&self) -> &'static str {
            "Test"
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
        }
    }

    #[test]
    fn app_new_and_offset() {
        let info = make_test_file_info();
        let viewer = Box::new(TestViewer::default());
        let app = App::new(info, viewer, 42);
        assert_eq!(app.offset(), 42);
    }

    #[test]
    fn apply_scroll_changes_offset() {
        let info = make_test_file_info();
        let viewer = Box::new(TestViewer::default());
        let mut app = App::new(info, viewer, 5);

        app.apply(TuiAction::ScrollDown(3), 80);
        assert_eq!(app.offset(), 8);

        app.apply(TuiAction::ScrollUp(2), 80);
        assert_eq!(app.offset(), 6);
    }

    #[test]
    fn render_app_produces_expected_lines() {
        let info = make_test_file_info();
        let viewer = Box::new(TestViewer { total_lines: 10 });
        let app = App::new(info, viewer, 2);

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
        let viewer = Box::new(TestViewer { total_lines: 50 });
        let mut app = App::new(info, viewer, 10);

        app.apply(TuiAction::GoToEnd, 80);
        assert_eq!(app.offset(), 49);

        app.apply(TuiAction::GoToStart, 80);
        assert_eq!(app.offset(), 0);
    }

    #[test]
    fn render_respects_content_width() {
        let info = make_test_file_info();
        let viewer = Box::new(TestViewer { total_lines: 5 });
        let app = App::new(info, viewer, 0);

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
        let viewer = Box::new(TestViewer { total_lines: 100 });
        let mut app = App::new(info, viewer, 50);

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
        let viewer = Box::new(TestViewer { total_lines: 100 });
        let mut app = App::new(info, viewer, 10);

        app.apply(TuiAction::ScrollDown(5), 80);
        assert_eq!(app.offset(), 15);

        app.apply(TuiAction::ScrollUp(3), 80);
        assert_eq!(app.offset(), 12);
    }

    #[test]
    fn page_down_and_page_up_use_the_provided_size() {
        let info = make_test_file_info();
        let viewer = Box::new(TestViewer { total_lines: 200 });
        let mut app = App::new(info, viewer, 10);

        // Page size of 30
        app.apply(TuiAction::PageDown(30), 80);
        assert_eq!(app.offset(), 40);

        app.apply(TuiAction::PageUp(30), 80);
        assert_eq!(app.offset(), 10);
    }

    #[test]
    fn go_to_start_and_go_to_end() {
        let info = make_test_file_info();
        let viewer = Box::new(TestViewer { total_lines: 80 });
        let mut app = App::new(info, viewer, 35);

        app.apply(TuiAction::GoToStart, 80);
        assert_eq!(app.offset(), 0);

        app.apply(TuiAction::GoToEnd, 80);
        assert_eq!(app.offset(), 79); // clamped by TestViewer
    }

    #[test]
    fn navigation_clamps_at_boundaries() {
        let info = make_test_file_info();
        let viewer = Box::new(TestViewer { total_lines: 30 });
        let mut app = App::new(info, viewer, 5);

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
        let viewer = Box::new(TestViewer { total_lines: 1000 });
        let mut app = App::new(info, viewer, 3);

        // Big page jump
        app.apply(TuiAction::PageDown(100), 80);
        assert_eq!(app.offset(), 103);
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
}

struct App {
    info: FileInfo,
    viewer: Box<dyn rcat_core::FileViewer>,
    offset: u64,
}

impl App {
    /// Creates a new App. Useful for tests and the real TUI.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(info: FileInfo, viewer: Box<dyn rcat_core::FileViewer>, offset: u64) -> Self {
        Self {
            info,
            viewer,
            offset,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// Applies a high-level action to the app state.
    /// `width` is passed to width-aware viewers for correct scrolling inside wrapped lines.
    pub fn apply(&mut self, action: TuiAction, width: u16) {
        match action {
            TuiAction::Quit => {}
            TuiAction::ScrollDown(n) => {
                self.offset = self
                    .viewer
                    .advance_lines(&self.info, self.offset, n as i64, width);
            }
            TuiAction::ScrollUp(n) => {
                self.offset =
                    self.viewer
                        .advance_lines(&self.info, self.offset, -(n as i64), width);
            }
            TuiAction::PageDown(n) => {
                self.offset = self
                    .viewer
                    .advance_lines(&self.info, self.offset, n as i64, width);
            }
            TuiAction::PageUp(n) => {
                self.offset =
                    self.viewer
                        .advance_lines(&self.info, self.offset, -(n as i64), width);
            }
            TuiAction::GoToStart => self.offset = 0,
            TuiAction::GoToEnd => {
                self.offset = self
                    .viewer
                    .advance_lines(&self.info, self.info.size, -8, width);
            }
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
            let action = match key.code {
                KeyCode::Char('q') | KeyCode::Esc => TuiAction::Quit,
                KeyCode::Char('j') | KeyCode::Down => TuiAction::ScrollDown(1),
                KeyCode::Char('k') | KeyCode::Up => TuiAction::ScrollUp(1),
                KeyCode::PageDown => TuiAction::PageDown(page_size),
                KeyCode::PageUp => TuiAction::PageUp(page_size),
                KeyCode::Char('g') => TuiAction::GoToStart,
                KeyCode::Char('G') => TuiAction::GoToEnd,
                _ => continue,
            };

            if matches!(action, TuiAction::Quit) {
                return Ok(());
            }

            app.apply(action, usable_width);
        }
    }
}

/// Extracted pure rendering logic. This is the key enabler for testing the UI
/// with `ratatui::backend::TestBackend` without any real terminal or crossterm.
fn render_app(f: &mut ratatui::Frame, app: &App, content_width: u16) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Header
            Constraint::Min(0),    // Main content
            Constraint::Length(2), // Footer
        ])
        .split(f.area());

    // Header
    let header_text = format!(
        " {}  ·  {} bytes  ·  {}",
        app.info.path.display(),
        app.info.size,
        app.viewer.name()
    );
    let header = Paragraph::new(header_text)
        .style(Style::default().fg(Color::Cyan).bold())
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(header, chunks[0]);

    // Content
    let max_rows = chunks[1].height.saturating_sub(1).max(1);
    let lines: Vec<Line> = app
        .viewer
        .render_lines(&app.info, app.offset, max_rows, content_width)
        .into_iter()
        .map(Line::from)
        .collect();

    let content = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(content, chunks[1]);

    // Footer
    let status = app.viewer.status(&app.info, app.offset);
    let footer_text = format!(
        " {}   │   q quit   ↑↓/jk scroll   PgUp/PgDn   g/G   │   {}",
        status,
        app.viewer.name()
    );
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Gray))
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(footer, chunks[2]);
}
