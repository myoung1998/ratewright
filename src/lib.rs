//! Converts between two ways people write rate limits down.
//!
//! "Shorthand" is the nginx `limit_req_zone` style: `10r/s`, `600r/m`.
//! "Window" is the style used by things like the IETF RateLimit-Limit
//! draft header and most public API docs: `10/1s`, `600/60s`.
//!
//! Both describe the same thing, a count of requests allowed per some
//! span of time, so they convert losslessly through a shared struct.

use std::fmt;

/// A rate limit normalized to "N requests per M seconds".
///
/// This is the canonical form both text formats parse into and format
/// out of. Keeping it as plain integers (rather than a float rate)
/// means round-tripping a spec never introduces drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: u64,
    pub window_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    Second,
    Minute,
    Hour,
    Day,
}

impl Unit {
    fn seconds(self) -> u64 {
        match self {
            Unit::Second => 1,
            Unit::Minute => 60,
            Unit::Hour => 3_600,
            Unit::Day => 86_400,
        }
    }

    fn from_char(c: char) -> Option<Unit> {
        match c {
            's' => Some(Unit::Second),
            'm' => Some(Unit::Minute),
            'h' => Some(Unit::Hour),
            'd' => Some(Unit::Day),
            _ => None,
        }
    }

    fn to_char(self) -> char {
        match self {
            Unit::Second => 's',
            Unit::Minute => 'm',
            Unit::Hour => 'h',
            Unit::Day => 'd',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    MissingSeparator,
    InvalidNumber(String),
    InvalidUnit(String),
    ZeroLimit,
    ZeroWindow,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Empty => write!(f, "input is empty"),
            ParseError::MissingSeparator => write!(f, "missing '/' separator"),
            ParseError::InvalidNumber(s) => write!(f, "'{s}' is not a valid whole number"),
            ParseError::InvalidUnit(s) => write!(f, "'{s}' is not a valid time unit (use s, m, h, or d)"),
            ParseError::ZeroLimit => write!(f, "request limit must be greater than zero"),
            ParseError::ZeroWindow => write!(f, "window must be greater than zero"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses nginx-style shorthand, e.g. `"10r/s"` or `"600r/m"`.
pub fn parse_shorthand(input: &str) -> Result<RateLimit, ParseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ParseError::Empty);
    }
    let (count_part, unit_part) = input.split_once('/').ok_or(ParseError::MissingSeparator)?;
    let count_part = count_part
        .strip_suffix('r')
        .or_else(|| count_part.strip_suffix('R'))
        .unwrap_or(count_part);
    let limit: u64 = count_part
        .parse()
        .map_err(|_| ParseError::InvalidNumber(count_part.to_string()))?;
    if limit == 0 {
        return Err(ParseError::ZeroLimit);
    }
    let unit_char = unit_part
        .trim()
        .chars()
        .next()
        .ok_or_else(|| ParseError::InvalidUnit(unit_part.to_string()))?;
    let unit = Unit::from_char(unit_char.to_ascii_lowercase())
        .ok_or_else(|| ParseError::InvalidUnit(unit_part.to_string()))?;
    Ok(RateLimit {
        limit,
        window_secs: unit.seconds(),
    })
}

/// Renders a [`RateLimit`] as nginx-style shorthand, picking the
/// largest whole unit that divides the window evenly so the output
/// stays readable. Falls back to seconds if nothing bigger fits.
pub fn format_shorthand(rate: &RateLimit) -> String {
    let unit = largest_exact_unit(rate.window_secs);
    let scaled_limit = rate.limit / (rate.window_secs / unit.seconds());
    format!("{}r/{}", scaled_limit, unit.to_char())
}

/// Parses count/window notation, e.g. `"10/1s"`, `"600/60s"`, or a
/// bare `"600/60"` which is assumed to be seconds.
pub fn parse_window(input: &str) -> Result<RateLimit, ParseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ParseError::Empty);
    }
    let (limit_part, window_part) = input.split_once('/').ok_or(ParseError::MissingSeparator)?;
    let limit: u64 = limit_part
        .trim()
        .parse()
        .map_err(|_| ParseError::InvalidNumber(limit_part.to_string()))?;
    if limit == 0 {
        return Err(ParseError::ZeroLimit);
    }

    let window_part = window_part.trim();
    let (digits, unit_suffix): (&str, &str) = match window_part.find(|c: char| !c.is_ascii_digit()) {
        Some(idx) => window_part.split_at(idx),
        None => (window_part, ""),
    };
    let magnitude: u64 = digits
        .parse()
        .map_err(|_| ParseError::InvalidNumber(digits.to_string()))?;
    let unit = if unit_suffix.is_empty() {
        Unit::Second
    } else {
        let unit_char = unit_suffix.chars().next().unwrap().to_ascii_lowercase();
        Unit::from_char(unit_char).ok_or_else(|| ParseError::InvalidUnit(unit_suffix.to_string()))?
    };
    let window_secs = magnitude * unit.seconds();
    if window_secs == 0 {
        return Err(ParseError::ZeroWindow);
    }
    Ok(RateLimit { limit, window_secs })
}

/// Renders a [`RateLimit`] as count/window notation in raw seconds,
/// e.g. `RateLimit { limit: 10, window_secs: 60 }` becomes `"10/60s"`.
pub fn format_window(rate: &RateLimit) -> String {
    format!("{}/{}s", rate.limit, rate.window_secs)
}

/// Effective steady-state throughput, useful for comparing two specs
/// written in different units (e.g. is `10r/s` stricter than `500/60s`?).
pub fn requests_per_second(rate: &RateLimit) -> f64 {
    rate.limit as f64 / rate.window_secs as f64
}

/// Convenience round trip: shorthand text straight to window text.
pub fn shorthand_to_window(input: &str) -> Result<String, ParseError> {
    parse_shorthand(input).map(|rate| format_window(&rate))
}

/// Convenience round trip: window text straight to shorthand text.
pub fn window_to_shorthand(input: &str) -> Result<String, ParseError> {
    parse_window(input).map(|rate| format_shorthand(&rate))
}

/// Largest unit that divides `window_secs` with no remainder, falling
/// back to seconds if nothing bigger fits.
fn largest_exact_unit(window_secs: u64) -> Unit {
    for unit in [Unit::Day, Unit::Hour, Unit::Minute] {
        if window_secs % unit.seconds() == 0 {
            return unit;
        }
    }
    Unit::Second
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_shorthand() {
        assert_eq!(
            parse_shorthand("10r/s").unwrap(),
            RateLimit { limit: 10, window_secs: 1 }
        );
        assert_eq!(
            parse_shorthand("600r/m").unwrap(),
            RateLimit { limit: 600, window_secs: 60 }
        );
    }

    #[test]
    fn shorthand_is_case_insensitive_on_unit() {
        assert_eq!(parse_shorthand("5r/H").unwrap().window_secs, 3_600);
    }

    #[test]
    fn rejects_zero_limit_and_bad_unit() {
        assert_eq!(parse_shorthand("0r/s"), Err(ParseError::ZeroLimit));
        assert!(matches!(parse_shorthand("10r/x"), Err(ParseError::InvalidUnit(_))));
        assert_eq!(parse_shorthand(""), Err(ParseError::Empty));
        assert_eq!(parse_shorthand("10r-s"), Err(ParseError::MissingSeparator));
    }

    #[test]
    fn parses_window_with_and_without_unit_suffix() {
        assert_eq!(
            parse_window("10/60s").unwrap(),
            RateLimit { limit: 10, window_secs: 60 }
        );
        assert_eq!(
            parse_window("10/60").unwrap(),
            RateLimit { limit: 10, window_secs: 60 }
        );
        assert_eq!(
            parse_window("10/1m").unwrap(),
            RateLimit { limit: 10, window_secs: 60 }
        );
    }

    #[test]
    fn formats_pick_the_largest_exact_unit() {
        assert_eq!(format_shorthand(&RateLimit { limit: 3600, window_secs: 3600 }), "1r/h");
        assert_eq!(format_shorthand(&RateLimit { limit: 100, window_secs: 60 }), "100r/m");
        assert_eq!(format_shorthand(&RateLimit { limit: 7, window_secs: 13 }), "7r/s");
    }

    #[test]
    fn round_trips_between_formats() {
        assert_eq!(shorthand_to_window("10r/s").unwrap(), "10/1s");
        assert_eq!(window_to_shorthand("600/60s").unwrap(), "600r/m");
    }

    #[test]
    fn requests_per_second_matches_expectation() {
        let rate = RateLimit { limit: 30, window_secs: 60 };
        assert!((requests_per_second(&rate) - 0.5).abs() < f64::EPSILON);
    }
}
