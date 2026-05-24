//! Interval parsing — shared between `/loop <interval> …` and
//! `/schedule add --every …`.
//!
//! Supported forms:
//!
//! - Plain numeric + unit: `5m`, `1h`, `30s`, `2d`.
//! - Bare integer → seconds (`60` == `60s`).
//! - 5-field cron: `* * * * *` — currently only the minute slot is honored
//!   (interval == stride). Full cron is future work; minute-stride covers
//!   the common "every N minutes" case and preserves on-disk compatibility
//!   with the Bun reference's `scheduled_tasks.json`.
//!
//! Cron rejection messages are intentionally specific so users understand
//! why their expression was only partially respected.

use std::fmt;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    seconds: u64,
}

impl Interval {
    pub fn from_seconds(seconds: u64) -> Self {
        Self { seconds }
    }

    pub fn seconds(self) -> u64 {
        self.seconds
    }

    pub fn human(self) -> String {
        let s = self.seconds;
        if s.is_multiple_of(86_400) && s >= 86_400 {
            return format!("{}d", s / 86_400);
        }
        if s.is_multiple_of(3_600) && s >= 3_600 {
            return format!("{}h", s / 3_600);
        }
        if s.is_multiple_of(60) && s >= 60 {
            return format!("{}m", s / 60);
        }
        format!("{}s", s)
    }
}

impl fmt::Display for Interval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.human())
    }
}

#[derive(Debug, Error)]
pub enum IntervalParseError {
    #[error("interval must not be empty")]
    Empty,
    #[error("interval '{0}' is not a recognized form (try 5m, 1h, 30s, 2d, or '*/5 * * * *')")]
    Malformed(String),
    #[error("interval '{0}' must be positive")]
    NonPositive(String),
    #[error("interval '{0}' is larger than the 1-year cap")]
    TooLarge(String),
    #[error(
        "cron expression '{0}' is only partially supported — only the minute stride \
             ('*/N * * * *' or 'N * * * *') is honored right now"
    )]
    CronUnsupported(String),
}

const ONE_YEAR_SECS: u64 = 365 * 86_400;

pub fn parse_interval(raw: &str) -> Result<Interval, IntervalParseError> {
    let input = raw.trim();
    if input.is_empty() {
        return Err(IntervalParseError::Empty);
    }

    if input.contains(' ') || input.contains('/') {
        return parse_cron_like(input);
    }

    parse_duration(input)
}

fn parse_duration(input: &str) -> Result<Interval, IntervalParseError> {
    // Split trailing alpha suffix from leading digits.
    let (num_part, unit_part) = split_numeric_suffix(input);
    if num_part.is_empty() {
        return Err(IntervalParseError::Malformed(input.to_string()));
    }

    let value: u64 = num_part
        .parse()
        .map_err(|_| IntervalParseError::Malformed(input.to_string()))?;

    if value == 0 {
        return Err(IntervalParseError::NonPositive(input.to_string()));
    }

    let seconds = match unit_part {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => value,
        "m" | "min" | "mins" | "minute" | "minutes" => value
            .checked_mul(60)
            .ok_or_else(|| IntervalParseError::TooLarge(input.to_string()))?,
        "h" | "hr" | "hrs" | "hour" | "hours" => value
            .checked_mul(3_600)
            .ok_or_else(|| IntervalParseError::TooLarge(input.to_string()))?,
        "d" | "day" | "days" => value
            .checked_mul(86_400)
            .ok_or_else(|| IntervalParseError::TooLarge(input.to_string()))?,
        _ => return Err(IntervalParseError::Malformed(input.to_string())),
    };

    if seconds > ONE_YEAR_SECS {
        return Err(IntervalParseError::TooLarge(input.to_string()));
    }

    Ok(Interval::from_seconds(seconds))
}

fn parse_cron_like(input: &str) -> Result<Interval, IntervalParseError> {
    let fields: Vec<&str> = input.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(IntervalParseError::Malformed(input.to_string()));
    }
    let (minute_field, rest) = (fields[0], &fields[1..]);
    // Require the other four fields to be wildcards — anything else is a
    // cron feature we don't implement yet.
    if !rest.iter().all(|f| *f == "*") {
        return Err(IntervalParseError::CronUnsupported(input.to_string()));
    }

    // `*/N` → every N minutes; bare integer → interpret as "every N minutes"
    // too, so `15 * * * *` doesn't silently become "once an hour at minute 15"
    // without warning (the Bun reference handles this the same way).
    let minutes: u64 = if let Some(stride) = minute_field.strip_prefix("*/") {
        stride
            .parse()
            .map_err(|_| IntervalParseError::Malformed(input.to_string()))?
    } else if minute_field == "*" {
        1
    } else {
        minute_field
            .parse()
            .map_err(|_| IntervalParseError::CronUnsupported(input.to_string()))?
    };

    if minutes == 0 {
        return Err(IntervalParseError::NonPositive(input.to_string()));
    }

    let seconds = minutes
        .checked_mul(60)
        .ok_or_else(|| IntervalParseError::TooLarge(input.to_string()))?;
    if seconds > ONE_YEAR_SECS {
        return Err(IntervalParseError::TooLarge(input.to_string()));
    }
    Ok(Interval::from_seconds(seconds))
}

fn split_numeric_suffix(input: &str) -> (&str, &str) {
    let split = input
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit())
        .map(|(i, _)| i)
        .unwrap_or(input.len());
    input.split_at(split)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_seconds() {
        assert_eq!(parse_interval("60").unwrap().seconds(), 60);
    }

    #[test]
    fn parses_duration_suffixes() {
        assert_eq!(parse_interval("30s").unwrap().seconds(), 30);
        assert_eq!(parse_interval("5m").unwrap().seconds(), 300);
        assert_eq!(parse_interval("1h").unwrap().seconds(), 3_600);
        assert_eq!(parse_interval("2d").unwrap().seconds(), 172_800);
        assert_eq!(parse_interval("  5m  ").unwrap().seconds(), 300);
        assert_eq!(parse_interval("5minutes").unwrap().seconds(), 300);
        assert_eq!(parse_interval("1hr").unwrap().seconds(), 3_600);
    }

    #[test]
    fn parses_simple_cron() {
        assert_eq!(parse_interval("*/5 * * * *").unwrap().seconds(), 300);
        assert_eq!(parse_interval("* * * * *").unwrap().seconds(), 60);
    }

    #[test]
    fn rejects_zero_interval() {
        assert!(matches!(
            parse_interval("0s"),
            Err(IntervalParseError::NonPositive(_))
        ));
        assert!(matches!(
            parse_interval("*/0 * * * *"),
            Err(IntervalParseError::NonPositive(_))
        ));
    }

    #[test]
    fn rejects_empty() {
        assert!(matches!(
            parse_interval("   "),
            Err(IntervalParseError::Empty)
        ));
    }

    #[test]
    fn rejects_bad_suffix() {
        assert!(matches!(
            parse_interval("5z"),
            Err(IntervalParseError::Malformed(_))
        ));
    }

    #[test]
    fn rejects_unsupported_cron() {
        assert!(matches!(
            parse_interval("* */2 * * *"),
            Err(IntervalParseError::CronUnsupported(_))
        ));
        assert!(matches!(
            parse_interval("0 0 1 * *"),
            Err(IntervalParseError::CronUnsupported(_))
        ));
    }

    #[test]
    fn rejects_over_one_year() {
        let two_years = 2 * 365;
        assert!(matches!(
            parse_interval(&format!("{}d", two_years)),
            Err(IntervalParseError::TooLarge(_))
        ));
    }

    #[test]
    fn human_roundtrips_major_units() {
        assert_eq!(Interval::from_seconds(60).human(), "1m");
        assert_eq!(Interval::from_seconds(3_600).human(), "1h");
        assert_eq!(Interval::from_seconds(86_400).human(), "1d");
        assert_eq!(Interval::from_seconds(45).human(), "45s");
    }
}
