//! Viewer-specific line styling for the main content area.

use ratatui::style::Color;
use ratatui::text::{Line, Span};

use crate::theme::Theme;

/// Style content lines based on the active viewer.
pub fn render_content_lines(
    viewer_name: &str,
    raw_lines: Vec<String>,
    theme: &Theme,
) -> Vec<Line<'static>> {
    match viewer_name {
        "Hex" => render_hex_styled_lines(raw_lines, theme),
        "JSON" => render_json_styled_lines(raw_lines, theme),
        _ => render_text_styled_lines(raw_lines, theme),
    }
}

fn render_text_styled_lines(raw_lines: Vec<String>, theme: &Theme) -> Vec<Line<'static>> {
    raw_lines
        .into_iter()
        .map(|line| {
            let trimmed = line.trim_start();
            let style = if trimmed.starts_with('(') {
                theme.text_system
            } else if trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("--")
            {
                theme.text_comment
            } else {
                theme.text_body
            };
            Line::styled(line, style)
        })
        .collect()
}

fn render_json_styled_lines(raw_lines: Vec<String>, theme: &Theme) -> Vec<Line<'static>> {
    raw_lines
        .into_iter()
        .map(|line| style_json_line(&line, theme))
        .collect()
}

fn style_json_line(line: &str, theme: &Theme) -> Line<'static> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('{')
        || trimmed.starts_with('}')
        || trimmed.starts_with('[')
        || trimmed.starts_with(']')
    {
        return Line::styled(line.to_string(), theme.json_bracket);
    }

    if trimmed.starts_with('(') {
        return Line::styled(line.to_string(), theme.text_system);
    }

    if let Some(colon_idx) = line.find(':') {
        let (key_part, value_part) = line.split_at(colon_idx + 1);
        return Line::from(vec![
            Span::styled(key_part.to_string(), theme.json_key),
            Span::styled(value_part.to_string(), theme.json_value),
        ]);
    }

    Line::styled(line.to_string(), theme.text_body)
}

/// Render Hex viewer output with theme colors.
pub fn render_hex_styled_lines(raw_lines: Vec<String>, theme: &Theme) -> Vec<Line<'static>> {
    raw_lines
        .into_iter()
        .map(|line| {
            if let Some((addr_part, rest)) = line.split_once(": ") {
                if let Some((hex_part, ascii_part)) = rest.split_once(" |") {
                    let mut spans = vec![Span::styled(format!("{addr_part}: "), theme.hex_address)];

                    for byte_str in hex_part.split_whitespace() {
                        let byte = u8::from_str_radix(byte_str, 16).unwrap_or(0);
                        let color = if byte == 0 {
                            theme.hex_null
                        } else if (0x20..=0x7e).contains(&byte) {
                            theme.hex_printable
                        } else {
                            theme.hex_nonprintable
                        };
                        spans.push(Span::styled(
                            format!("{byte_str} "),
                            ratatui::style::Style::default().fg(color),
                        ));
                    }

                    spans.push(Span::styled(
                        "|",
                        ratatui::style::Style::default().fg(Color::DarkGray),
                    ));
                    spans.push(Span::styled(
                        ascii_part.to_string(),
                        ratatui::style::Style::default().fg(theme.hex_ascii),
                    ));

                    Line::from(spans)
                } else {
                    Line::from(line)
                }
            } else {
                Line::from(line)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_styling_preserves_line_count() {
        let raw = vec![
            "00000000: 00 01 02 03 04 05 06 07  08 09 0a 0b 0c 0d 0e 0f |................|"
                .to_string(),
        ];
        let theme = Theme::default();
        let styled = render_hex_styled_lines(raw.clone(), &theme);
        assert_eq!(styled.len(), raw.len());
    }

    #[test]
    fn json_bracket_line_gets_bracket_style() {
        let theme = Theme::default();
        let line = style_json_line("  {", &theme);
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn json_key_value_line_splits_key_and_value_spans() {
        let theme = Theme::default();
        let line = style_json_line(r#"  "name": 42,"#, &theme);
        assert!(
            line.spans.len() >= 2,
            "expected separate key/value spans, got {}",
            line.spans.len()
        );
    }
}
