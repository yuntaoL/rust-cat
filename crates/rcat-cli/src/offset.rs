//! Parse byte offset CLI values (decimal or `0x` hex).

/// Parse a byte offset from decimal or `0x`-prefixed hex.
pub fn parse_offset(s: &str) -> anyhow::Result<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| anyhow::anyhow!("invalid hex offset: {e}"))
    } else {
        s.parse::<u64>()
            .map_err(|e| anyhow::anyhow!("invalid offset: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_decimal() {
        assert_eq!(parse_offset("42").unwrap(), 42);
    }

    #[test]
    fn parses_hex_prefix() {
        assert_eq!(parse_offset("0x10").unwrap(), 16);
        assert_eq!(parse_offset("0Xff").unwrap(), 255);
    }

    #[test]
    fn rejects_invalid() {
        assert!(parse_offset("nope").is_err());
        assert!(parse_offset("0xzz").is_err());
    }
}