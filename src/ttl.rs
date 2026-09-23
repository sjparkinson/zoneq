const MAX_TTL: u64 = 2_147_483_647;

/// Parses a TTL as plain seconds (`86400`) or BIND units (`1h`, `3W`,
/// `1h30m`). Returns `None` for anything else, including values over 2^31 - 1.
pub fn parse_ttl(raw: &str) -> Option<u32> {
    if raw.is_empty() {
        return None;
    }
    if raw.bytes().all(|b| b.is_ascii_digit()) {
        return raw
            .parse::<u64>()
            .ok()
            .filter(|&n| n <= MAX_TTL)
            .map(|n| n as u32);
    }

    let mut total: u64 = 0;
    let mut digits = String::new();
    for c in raw.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit = match c.to_ascii_lowercase() {
            'w' => 604_800,
            'd' => 86_400,
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => return None,
        };
        let n: u64 = digits.parse().ok()?;
        total = total.checked_add(n.checked_mul(unit)?)?;
        digits.clear();
    }
    // Every number needs a unit once units are in play.
    if !digits.is_empty() || total > MAX_TTL {
        return None;
    }
    Some(total as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_seconds() {
        assert_eq!(parse_ttl("86400"), Some(86400));
        assert_eq!(parse_ttl("0"), Some(0));
        assert_eq!(parse_ttl("2147483647"), Some(2147483647));
    }

    #[test]
    fn units() {
        assert_eq!(parse_ttl("1h"), Some(3600));
        assert_eq!(parse_ttl("1D"), Some(86400));
        assert_eq!(parse_ttl("3W"), Some(3 * 604800));
        assert_eq!(parse_ttl("1h30m"), Some(5400));
        assert_eq!(
            parse_ttl("1w2d3h4m5s"),
            Some(604800 + 2 * 86400 + 3 * 3600 + 4 * 60 + 5)
        );
    }

    #[test]
    fn overflow() {
        assert_eq!(parse_ttl("2147483648"), None);
        assert_eq!(parse_ttl("99999999999999999999"), None);
        assert_eq!(parse_ttl("4000w"), None);
    }

    #[test]
    fn junk() {
        for raw in ["", "IN", "A", "h", "1h30", "1x", "-1", "1.5h"] {
            assert_eq!(parse_ttl(raw), None, "{raw}");
        }
    }
}
