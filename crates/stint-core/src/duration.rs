/// Duration in minutes, parsed from strings like "4h", "30m", "1.5h".
use std::fmt;
use std::str::FromStr;

use thiserror::Error;

/// A duration stored internally as whole minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(u32);

/// Error returned when a duration string cannot be parsed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DurationError {
    /// The input string has an unrecognised format.
    #[error("invalid duration string: {0:?}")]
    InvalidFormat(String),
}

impl Duration {
    /// Construct a duration from a raw minute count.
    pub fn from_minutes(minutes: u32) -> Self {
        Duration(minutes)
    }

    /// Return the total number of minutes.
    pub fn minutes(self) -> u32 {
        self.0
    }

    /// Add two durations, returning the sum.
    pub fn add(self, other: Duration) -> Duration {
        Duration(self.0 + other.0)
    }
}

impl FromStr for Duration {
    type Err = DurationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.is_empty() {
            return Err(DurationError::InvalidFormat(s.to_owned()));
        }

        if let Some(hours_str) = s.strip_suffix('h') {
            let hours: f64 = hours_str
                .parse()
                .map_err(|_| DurationError::InvalidFormat(s.to_owned()))?;
            if hours < 0.0 {
                return Err(DurationError::InvalidFormat(s.to_owned()));
            }
            let minutes = (hours * 60.0).round() as u32;
            return Ok(Duration(minutes));
        }

        if let Some(minutes_str) = s.strip_suffix('m') {
            let minutes: f64 = minutes_str
                .parse()
                .map_err(|_| DurationError::InvalidFormat(s.to_owned()))?;
            if minutes < 0.0 {
                return Err(DurationError::InvalidFormat(s.to_owned()));
            }
            return Ok(Duration(minutes.round() as u32));
        }

        Err(DurationError::InvalidFormat(s.to_owned()))
    }
}

impl fmt::Display for Duration {
    /// Serialise to canonical form: "Xh" if evenly divisible by 60, else "Xm".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            write!(f, "0m")
        } else if self.0 % 60 == 0 {
            write!(f, "{}h", self.0 / 60)
        } else {
            write!(f, "{}m", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_whole_hours() {
        assert_eq!("4h".parse::<Duration>().unwrap(), Duration(240));
    }

    #[test]
    fn parse_minutes() {
        assert_eq!("30m".parse::<Duration>().unwrap(), Duration(30));
    }

    #[test]
    fn parse_fractional_hours() {
        assert_eq!("1.5h".parse::<Duration>().unwrap(), Duration(90));
    }

    #[test]
    fn parse_90m() {
        assert_eq!("90m".parse::<Duration>().unwrap(), Duration(90));
    }

    #[test]
    fn parse_zero() {
        assert_eq!("0h".parse::<Duration>().unwrap(), Duration(0));
        assert_eq!("0m".parse::<Duration>().unwrap(), Duration(0));
    }

    #[test]
    fn display_whole_hours() {
        assert_eq!(Duration(240).to_string(), "4h");
    }

    #[test]
    fn display_minutes() {
        assert_eq!(Duration(30).to_string(), "30m");
    }

    #[test]
    fn display_non_round_hours() {
        assert_eq!(Duration(90).to_string(), "90m");
    }

    #[test]
    fn display_zero() {
        assert_eq!(Duration(0).to_string(), "0m");
    }

    #[test]
    fn add_durations() {
        assert_eq!(Duration(60).add(Duration(30)), Duration(90));
    }

    #[test]
    fn invalid_format() {
        assert!("4x".parse::<Duration>().is_err());
        assert!("".parse::<Duration>().is_err());
        assert!("abc".parse::<Duration>().is_err());
    }
}
