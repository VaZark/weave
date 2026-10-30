use std::time::Duration;

/// Parses `45s`, `30m`, `2h`, `1h30m`, `1h 30m 15s`. Every number needs a unit, so a bare
/// `90` is an error rather than a guess between seconds and minutes. Zero is an error too:
/// use this for delays and timers.
pub fn parse_duration(input: &str) -> Result<Duration, String> {
    let d = parse_span(input)?;
    if d.is_zero() {
        return Err("duration must be greater than zero".into());
    }
    Ok(d)
}

/// Like `parse_duration` but allows zero (`0s`), for positions such as "the start".
pub fn parse_span(input: &str) -> Result<Duration, String> {
    let s: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return Err("empty duration".into());
    }
    let mut total: u64 = 0;
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit = match c.to_ascii_lowercase() {
            'h' => 3600,
            'm' => 60,
            's' => 1,
            _ => {
                return Err(format!(
                    "invalid duration \"{input}\" (try 30m, 1h30m, 45s)"
                ));
            }
        };
        let n: u64 = digits
            .parse()
            .map_err(|_| format!("invalid duration \"{input}\" (try 30m, 1h30m, 45s)"))?;
        digits.clear();
        total = n
            .checked_mul(unit)
            .and_then(|v| total.checked_add(v))
            .ok_or_else(|| format!("duration \"{input}\" is too large"))?;
    }
    if !digits.is_empty() {
        return Err(format!(
            "invalid duration \"{input}\": missing unit after {digits} (try {digits}s, {digits}m or {digits}h)"
        ));
    }
    Ok(Duration::from_secs(total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        let secs = |s| parse_duration(s).unwrap().as_secs();
        assert_eq!(secs("30m"), 1800);
        assert_eq!(secs("90m"), 5400);
        assert_eq!(secs("1h30m"), 5400);
        assert_eq!(secs("1h 30m 15s"), 5415);
        assert_eq!(secs("45S"), 45);
    }

    #[test]
    fn spans_allow_zero_but_durations_do_not() {
        assert_eq!(parse_span("0s").unwrap(), Duration::ZERO);
        assert_eq!(parse_span("1m30s").unwrap().as_secs(), 90);
        assert!(parse_span("0").is_err(), "a unit is still required");
        assert!(parse_duration("0s").is_err());
    }

    #[test]
    fn a_bare_number_names_the_missing_unit() {
        let err = parse_duration("90").unwrap_err();
        assert!(err.contains("missing unit") && err.contains("90"), "{err}");
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "", "0", "0m", "m", "5x", "1h30", "-5m", "1.5h", "90", "30", "1h 30",
        ] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }
}
