//! How long the talk is, as the frontmatter says, for the speaker view to
//! count down: a duration, or a deadline, the time of day it ends at. The
//! speaker view reads them the same way from the field it has for them.

use std::fmt;

/// A duration as the frontmatter has it, which YAML reads as a number of
/// minutes or as text, as `1h30m`, `45m` or `90s`.
#[derive(serde::Deserialize)]
#[cfg_attr(test, derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum Written {
    Minutes(f64),
    Text(String),
}

impl Written {
    /// How many seconds it is, if it is a duration.
    pub fn seconds(&self) -> Option<u32> {
        match self {
            Written::Minutes(minutes) => seconds(minutes * 60.0),
            Written::Text(text) => duration(text),
        }
    }
}

impl fmt::Display for Written {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Written::Minutes(minutes) => write!(f, "{minutes}"),
            Written::Text(text) => f.write_str(text),
        }
    }
}

/// A number of seconds, if it is a positive one that fits.
fn seconds(seconds: f64) -> Option<u32> {
    (seconds.is_finite() && seconds >= 1.0 && seconds <= u32::MAX as f64)
        .then(|| seconds.round() as u32)
}

/// The seconds of a duration written as a number of minutes, as `45`, or in
/// hours, minutes and seconds, as `1h30m`, `1h 30m`, `45min` or `90s`.
pub fn duration(text: &str) -> Option<u32> {
    let text = text.trim();
    if let Ok(minutes) = text.parse::<f64>() {
        return seconds(minutes * 60.0);
    }
    let mut total = 0.0;
    let mut rest = text;
    while !rest.is_empty() {
        let number = rest
            .find(|char: char| !(char.is_ascii_digit() || char == '.'))
            .unwrap_or(rest.len());
        let value: f64 = rest[..number].parse().ok()?;
        rest = rest[number..].trim_start();
        let unit = rest
            .find(|char: char| !char.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        total += value
            * match &rest[..unit] {
                "h" => 3600.0,
                "m" | "min" => 60.0,
                "s" => 1.0,
                _ => return None,
            };
        rest = rest[unit..].trim_start();
    }
    seconds(total)
}

/// The time of day a deadline is written as, as `15:30` or `9:05`, in hours
/// and minutes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Deadline {
    pub hour: u8,
    pub minute: u8,
}

impl Deadline {
    pub fn parse(text: &str) -> Option<Deadline> {
        let (hour, minute) = text.trim().split_once(':')?;
        let digits = |text: &str, lengths: std::ops::RangeInclusive<usize>| {
            let digits = lengths.contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
            digits.then(|| text.parse::<u8>().ok()).flatten()
        };
        let (hour, minute) = (digits(hour, 1..=2)?, digits(minute, 2..=2)?);
        (hour < 24 && minute < 60).then_some(Deadline { hour, minute })
    }
}

impl fmt::Display for Deadline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_duration_is_minutes_or_hours_minutes_and_seconds() {
        assert_eq!(duration("45"), Some(2700));
        assert_eq!(duration("1.5"), Some(90));
        assert_eq!(duration("45m"), Some(2700));
        assert_eq!(duration("45min"), Some(2700));
        assert_eq!(duration("1h30m"), Some(5400));
        assert_eq!(duration("1h 30m"), Some(5400));
        assert_eq!(duration("90s"), Some(90));
        assert_eq!(duration("1h30"), None);
        assert_eq!(duration("long"), None);
        assert_eq!(duration("0"), None);
        assert_eq!(duration(""), None);
        assert_eq!(Written::Minutes(20.0).seconds(), Some(1200));
        assert_eq!(Written::Minutes(-1.0).seconds(), None);
    }

    #[test]
    fn a_deadline_is_hours_and_minutes() {
        let at = |hour, minute| Some(Deadline { hour, minute });
        assert_eq!(Deadline::parse("15:30"), at(15, 30));
        assert_eq!(Deadline::parse("9:05"), at(9, 5));
        assert_eq!(Deadline::parse("24:00"), None);
        assert_eq!(Deadline::parse("9:60"), None);
        assert_eq!(Deadline::parse("9:5:0"), None);
        assert_eq!(Deadline::parse("15"), None);
        assert_eq!(at(9, 5).unwrap().to_string(), "09:05");
    }
}
