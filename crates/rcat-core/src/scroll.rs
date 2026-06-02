//! Scroll position math shared by the TUI and viewers.

/// Parse total display-line count from a status string like `JSON  line 221/439 (50%)`.
pub fn parse_display_line_extent_from_status(status: &str) -> Option<u64> {
    for token in status.split_whitespace() {
        let Some((_current, rest)) = token.split_once('/') else {
            continue;
        };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(total_lines) = digits.parse::<u64>() {
            return Some(total_lines.saturating_sub(1).max(1));
        }
    }
    None
}

/// Normalized scroll position in `[0.0, 1.0]`.
pub fn scroll_fraction(anchor: u64, extent: u64) -> f64 {
    if extent == 0 {
        0.0
    } else {
        (anchor as f64 / extent as f64).clamp(0.0, 1.0)
    }
}

/// Map a fraction back to an anchor value in `[0, extent]`.
pub fn anchor_from_fraction(fraction: f64, extent: u64) -> u64 {
    if extent == 0 {
        return 0;
    }
    let f = fraction.clamp(0.0, 1.0);
    ((f * extent as f64).round() as u64).min(extent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_style_status() {
        let extent = parse_display_line_extent_from_status("JSON  line 221/439 (50%)").unwrap();
        assert_eq!(extent, 438);
    }

    #[test]
    fn fraction_round_trip() {
        assert_eq!(anchor_from_fraction(0.5, 1000), 500);
        let frac = scroll_fraction(221, 438);
        let mapped = anchor_from_fraction(frac, 999);
        assert!((500..=510).contains(&mapped), "mapped was {mapped}");
    }
}
