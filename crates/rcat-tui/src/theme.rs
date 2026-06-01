//! Shared color theme for the rcat TUI.

use ratatui::style::{Color, Modifier, Style};

/// Terminal color palette used across header, content, footer, and overlays.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub header: Style,
    pub footer: Style,
    pub border: Style,
    pub label: Style,
    pub value: Style,
    pub text_body: Style,
    pub text_comment: Style,
    pub text_system: Style,
    pub json_bracket: Style,
    pub json_key: Style,
    pub json_value: Style,
    pub hex_address: Style,
    pub hex_null: Color,
    pub hex_printable: Color,
    pub hex_nonprintable: Color,
    pub hex_ascii: Color,
    pub help_border: Style,
    pub help_body: Style,
    pub sidebar_title: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            header: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            footer: Style::default().fg(Color::Gray),
            border: Style::default().fg(Color::DarkGray),
            label: Style::default().fg(Color::Yellow),
            value: Style::default().fg(Color::White),
            text_body: Style::default().fg(Color::White),
            text_comment: Style::default().fg(Color::DarkGray),
            text_system: Style::default().fg(Color::Yellow),
            json_bracket: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            json_key: Style::default().fg(Color::Magenta),
            json_value: Style::default().fg(Color::Green),
            hex_address: Style::default().fg(Color::Gray),
            hex_null: Color::DarkGray,
            hex_printable: Color::Green,
            hex_nonprintable: Color::Red,
            hex_ascii: Color::Cyan,
            help_border: Style::default().fg(Color::Yellow),
            help_body: Style::default().fg(Color::White).bg(Color::Black),
            sidebar_title: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        }
    }
}
