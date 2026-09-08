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
    MissingBurst,
    InvalidBurst(String),
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
            ParseError::MissingBurst => write!(f, "missing ';burst=N' segment"),
            ParseError::InvalidBurst(s) => write!(f, "'{s}' is not a valid burst (expected 'burst=N')"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses nginx-style shorthand, e.g. `"10r/s"` or `"600r/m"`.
///
/// The unit may carry a leading magnitude, e.g. `"7r/2m"` for 7
/// requests per 2 minutes. This isn't valid nginx config syntax (nginx
/// only accepts a bare `s` or `m`), but it's what [`format_shorthand`]
/// falls back to when a window can't be expressed as a whole count of
/// a single unit, so parsing has to accept it back.
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
    let (magnitude, unit) = parse_magnitude_and_unit(unit_part)?;
    let window_secs = magnitude * unit.seconds();
    if window_secs == 0 {
        return Err(ParseError::ZeroWindow);
    }
    Ok(RateLimit { limit, window_secs })
}

/// Renders a [`RateLimit`] as nginx-style shorthand, picking the
/// largest whole unit that divides the window evenly so the output
/// stays readable. When the window isn't a whole multiple of that unit
/// (e.g. a 13 second window), the count is left untouched and the unit
/// carries a magnitude instead (`"7r/13s"`), rather than rounding the
/// count and silently changing the rate.
pub fn format_shorthand(rate: &RateLimit) -> String {
    let unit = largest_exact_unit(rate.window_secs);
    let magnitude = rate.window_secs / unit.seconds();
    if magnitude == 1 {
        format!("{}r/{}", rate.limit, unit.to_char())
    } else {
        format!("{}r/{}{}", rate.limit, magnitude, unit.to_char())
    }
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
    let (magnitude, unit) = parse_magnitude_and_unit(window_part)?;
    let window_secs = magnitude * unit.seconds();
    if window_secs == 0 {
        return Err(ParseError::ZeroWindow);
    }
    Ok(RateLimit { limit, window_secs })
}

/// Splits a string like `"60s"`, `"600"`, or `"s"` into a magnitude and
/// a unit. A missing digit run defaults the magnitude to 1 (so a bare
/// unit letter, as in plain shorthand, means "one of these"); a
/// missing unit suffix defaults to seconds (so a bare number, as in
/// plain window notation, means "this many seconds").
fn parse_magnitude_and_unit(s: &str) -> Result<(u64, Unit), ParseError> {
    let s = s.trim();
    let (digits, unit_suffix): (&str, &str) = match s.find(|c: char| !c.is_ascii_digit()) {
        Some(idx) => s.split_at(idx),
        None => (s, ""),
    };
    let magnitude: u64 = if digits.is_empty() {
        1
    } else {
        digits.parse().map_err(|_| ParseError::InvalidNumber(digits.to_string()))?
    };
    let unit = if unit_suffix.is_empty() {
        Unit::Second
    } else {
        let unit_char = unit_suffix.chars().next().unwrap().to_ascii_lowercase();
        Unit::from_char(unit_char).ok_or_else(|| ParseError::InvalidUnit(unit_suffix.to_string()))?
    };
    Ok((magnitude, unit))
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

/// Parses a rate spec in whichever of the three notations ratewright
/// understands: shorthand (`10r/s`), window (`10/60s`), or a
/// token-bucket descriptor (`10r/s;burst=20`, burst is ignored). Used
/// by [`equivalent`] callers that need to accept either side of a
/// comparison in whatever spelling it shows up in.
///
/// This works because [`parse_shorthand`] already treats the `r`
/// suffix as optional, so it happens to accept window notation too;
/// the only case that needs a separate branch is the `;burst=` tail.
pub fn parse_rate(input: &str) -> Result<RateLimit, ParseError> {
    match input.split_once(';') {
        Some(_) => parse_token_bucket(input).map(|bucket| bucket.rate),
        None => parse_shorthand(input),
    }
}

/// True if two rate limits allow the same steady-state throughput,
/// e.g. `10r/s` and `36000/1h` are equivalent even though neither
/// parses to the same `RateLimit` value. Compared by cross-multiplying
/// (`a.limit * b.window_secs == b.limit * a.window_secs`) rather than
/// via [`requests_per_second`], so the result is exact and never
/// subject to floating point rounding.
pub fn equivalent(a: &RateLimit, b: &RateLimit) -> bool {
    a.limit as u128 * b.window_secs as u128 == b.limit as u128 * a.window_secs as u128
}

/// A token bucket: refills at a steady `rate` and holds up to `burst`
/// extra requests worth of capacity on top of that. This is the model
/// behind nginx's `limit_req zone=...; burst=N;` directive.
///
/// It's also just GCRA (the generic cell rate algorithm used by things
/// like redis-cell and Envoy's rate limiter) under a different name: a
/// GCRA limiter with emission interval `T` and delay variation tolerance
/// `tau` accepts exactly the same traffic as a token bucket refilling at
/// `1/T` with burst `tau/T`. See [`gcra_emission_interval`] and
/// [`gcra_delay_variation_tolerance`] for that conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenBucket {
    pub rate: RateLimit,
    pub burst: u64,
}

/// Parses a token-bucket descriptor, e.g. `"10r/s;burst=20"`. The rate
/// half accepts anything [`parse_shorthand`] does, including the
/// magnitude-on-unit fallback (`"7r/2m;burst=5"`).
pub fn parse_token_bucket(input: &str) -> Result<TokenBucket, ParseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ParseError::Empty);
    }
    let (rate_part, burst_part) = input.split_once(';').ok_or(ParseError::MissingBurst)?;
    let rate = parse_shorthand(rate_part)?;
    let burst_part = burst_part.trim();
    let digits = burst_part
        .strip_prefix("burst=")
        .ok_or_else(|| ParseError::InvalidBurst(burst_part.to_string()))?;
    let burst: u64 = digits
        .parse()
        .map_err(|_| ParseError::InvalidBurst(burst_part.to_string()))?;
    Ok(TokenBucket { rate, burst })
}

/// Renders a [`TokenBucket`] back to descriptor form, e.g.
/// `"10r/s;burst=20"`.
pub fn format_token_bucket(bucket: &TokenBucket) -> String {
    format!("{};burst={}", format_shorthand(&bucket.rate), bucket.burst)
}

/// GCRA's emission interval: the steady-state time between conforming
/// requests, in seconds. Equivalent to `1 / requests_per_second`.
pub fn gcra_emission_interval(rate: &RateLimit) -> f64 {
    rate.window_secs as f64 / rate.limit as f64
}

/// GCRA's delay variation tolerance: how far a burst of requests is
/// allowed to run ahead of the steady-state schedule before being
/// throttled, in seconds. A token bucket with `burst` extra capacity on
/// top of `rate` tolerates exactly `burst` emission intervals of slack.
pub fn gcra_delay_variation_tolerance(bucket: &TokenBucket) -> f64 {
    bucket.burst as f64 * gcra_emission_interval(&bucket.rate)
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
        assert_eq!(format_shorthand(&RateLimit { limit: 3600, window_secs: 3600 }), "3600r/h");
        assert_eq!(format_shorthand(&RateLimit { limit: 100, window_secs: 60 }), "100r/m");
    }

    #[test]
    fn formats_fall_back_to_a_magnitude_when_no_unit_divides_evenly() {
        // 13 seconds isn't a whole minute/hour/day, so the count stays
        // untouched and the unit carries the leftover magnitude instead
        // of silently rounding the count and changing the rate.
        assert_eq!(format_shorthand(&RateLimit { limit: 7, window_secs: 13 }), "7r/13s");
        // 120 seconds is a whole number of minutes (2), so it reduces to
        // that unit with an explicit magnitude, leaving the count of 7
        // untouched rather than truncating it down to 3 per minute.
        assert_eq!(format_shorthand(&RateLimit { limit: 7, window_secs: 120 }), "7r/2m");
    }

    #[test]
    fn parses_shorthand_with_a_magnitude_on_the_unit() {
        assert_eq!(
            parse_shorthand("7r/2m").unwrap(),
            RateLimit { limit: 7, window_secs: 120 }
        );
        assert_eq!(parse_shorthand("10r/0s"), Err(ParseError::ZeroWindow));
    }

    #[test]
    fn round_trips_between_formats() {
        assert_eq!(shorthand_to_window("10r/s").unwrap(), "10/1s");
        assert_eq!(window_to_shorthand("600/60s").unwrap(), "600r/m");
        assert_eq!(shorthand_to_window("7r/2m").unwrap(), "7/120s");
        assert_eq!(window_to_shorthand("7/120s").unwrap(), "7r/2m");
        assert_eq!(window_to_shorthand("10/13s").unwrap(), "10r/13s");
    }

    #[test]
    fn requests_per_second_matches_expectation() {
        let rate = RateLimit { limit: 30, window_secs: 60 };
        assert!((requests_per_second(&rate) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn parses_and_formats_token_bucket() {
        assert_eq!(
            parse_token_bucket("10r/s;burst=20").unwrap(),
            TokenBucket { rate: RateLimit { limit: 10, window_secs: 1 }, burst: 20 }
        );
        assert_eq!(format_token_bucket(&TokenBucket {
            rate: RateLimit { limit: 10, window_secs: 1 },
            burst: 20,
        }), "10r/s;burst=20");
        // Round-trips through the magnitude-on-unit fallback too.
        assert_eq!(
            parse_token_bucket("7r/2m;burst=3").unwrap(),
            TokenBucket { rate: RateLimit { limit: 7, window_secs: 120 }, burst: 3 }
        );
    }

    #[test]
    fn rejects_malformed_token_buckets() {
        assert_eq!(parse_token_bucket(""), Err(ParseError::Empty));
        assert_eq!(parse_token_bucket("10r/s"), Err(ParseError::MissingBurst));
        assert!(matches!(parse_token_bucket("10r/s;burst=abc"), Err(ParseError::InvalidBurst(_))));
        assert!(matches!(parse_token_bucket("10r/s;max=5"), Err(ParseError::InvalidBurst(_))));
        assert!(matches!(parse_token_bucket("0r/s;burst=5"), Err(ParseError::ZeroLimit)));
    }

    #[test]
    fn parse_rate_accepts_all_three_notations() {
        assert_eq!(parse_rate("10r/s").unwrap(), RateLimit { limit: 10, window_secs: 1 });
        assert_eq!(parse_rate("10/60s").unwrap(), RateLimit { limit: 10, window_secs: 60 });
        assert_eq!(
            parse_rate("10r/s;burst=20").unwrap(),
            RateLimit { limit: 10, window_secs: 1 }
        );
        assert_eq!(parse_rate(""), Err(ParseError::Empty));
    }

    #[test]
    fn equivalent_matches_same_rate_written_differently() {
        // 10 requests/second written three different ways.
        let a = RateLimit { limit: 10, window_secs: 1 };
        let b = RateLimit { limit: 600, window_secs: 60 };
        let c = RateLimit { limit: 36_000, window_secs: 3_600 };
        assert!(equivalent(&a, &b));
        assert!(equivalent(&b, &c));
        assert!(equivalent(&a, &c));

        let different = RateLimit { limit: 599, window_secs: 60 };
        assert!(!equivalent(&a, &different));
    }

    #[test]
    fn equivalent_handles_large_values_without_overflow() {
        let huge = RateLimit { limit: u64::MAX, window_secs: u64::MAX };
        assert!(equivalent(&huge, &huge));
        assert!(!equivalent(&huge, &RateLimit { limit: u64::MAX, window_secs: 1 }));
    }

    #[test]
    fn gcra_parameters_match_token_bucket() {
        let bucket = TokenBucket { rate: RateLimit { limit: 10, window_secs: 1 }, burst: 20 };
        assert!((gcra_emission_interval(&bucket.rate) - 0.1).abs() < f64::EPSILON);
        assert!((gcra_delay_variation_tolerance(&bucket) - 2.0).abs() < f64::EPSILON);

        let slow = TokenBucket { rate: RateLimit { limit: 1, window_secs: 60 }, burst: 5 };
        assert!((gcra_emission_interval(&slow.rate) - 60.0).abs() < f64::EPSILON);
        assert!((gcra_delay_variation_tolerance(&slow) - 300.0).abs() < f64::EPSILON);
    }
}
