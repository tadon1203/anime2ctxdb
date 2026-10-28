use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The `kind` of an entity that can speak.
pub const CHARACTER: &str = "character";

/// The suggestion ID that made the entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub String);

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A noun in the story: a person, a place, an organization, an object, or a concept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub id: EntityId,
    pub kind: String,
    pub name: String,
    pub reading: Option<String>,
}

/// A point in the story: an episode and a time in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub episode: u32,
    pub time_ms: u32,
}

/// Writes a time as `mm:ss`, for example `12:34`.
pub fn format_clock(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Reads a time written as `mm:ss`. The seconds must be below 60.
pub fn parse_clock(text: &str) -> Result<u32> {
    let (minutes, seconds) = text
        .split_once(':')
        .with_context(|| format!("{text} is not a time in the form mm:ss"))?;
    let minutes: u32 = minutes
        .parse()
        .with_context(|| format!("{text} is not a time in the form mm:ss"))?;
    let seconds: u32 = seconds
        .parse()
        .with_context(|| format!("{text} is not a time in the form mm:ss"))?;
    if seconds >= 60 {
        bail!("{text} has more than 59 seconds");
    }
    Ok((minutes * 60 + seconds) * 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clock_text_round_trips_through_milliseconds() {
        assert_eq!(format_clock(754_000), "12:34");
        assert_eq!(parse_clock("12:34").expect("the time parses"), 754_000);
    }

    #[test]
    fn a_clock_text_with_seconds_of_sixty_is_an_error() {
        assert!(parse_clock("01:60").is_err());
    }
}
