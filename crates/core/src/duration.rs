use std::time::Duration;

/// Parses `90`, `45s`, `30m`, `2h`, `1h30m`, `1h 30m 15s`. A bare number means minutes.
pub fn parse_duration(input: &str) -> Result<Duration, String> {
    let s: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return Err("empty duration".into());
    }
    if let Ok(minutes) = s.parse::<u64>() {
        return checked(minutes.checked_mul(60), input);
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
            "invalid duration \"{input}\": missing unit after {digits}"
        ));
    }
    checked(Some(total), input)
}

fn checked(secs: Option<u64>, input: &str) -> Result<Duration, String> {
    match secs {
        Some(0) => Err("duration must be greater than zero".into()),
        Some(s) => Ok(Duration::from_secs(s)),
        None => Err(format!("duration \"{input}\" is too large")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        let secs = |s| parse_duration(s).unwrap().as_secs();
        assert_eq!(secs("30m"), 1800);
        assert_eq!(secs("90"), 5400);
        assert_eq!(secs("1h30m"), 5400);
        assert_eq!(secs("1h 30m 15s"), 5415);
        assert_eq!(secs("45S"), 45);
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "0", "0m", "m", "5x", "1h30", "-5m", "1.5h"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }
}
