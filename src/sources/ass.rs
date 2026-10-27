use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::LazyLock;
use thiserror::Error;

/// `<SourceId>:<n>`, where `n` is the index of the first merged `Dialogue`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LineId(pub String);

impl fmt::Display for LineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// One subtitle line, after the tags, the ruby, and the merge rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Index of the first merged `Dialogue`, from 0.
    pub index: u32,
    pub start_ms: u32,
    pub end_ms: u32,
    /// The text without tags, the speaker label, and the offscreen marks.
    pub text: String,
    /// RGB. `None` for the default color.
    pub color: Option<u32>,
    /// The text had the offscreen marks `⚟` and `⚞`.
    pub offscreen: bool,
    /// The label in full-width parentheses at the start of the text, for example "あんな・みくる".
    pub speaker_label: Option<String>,
}

#[derive(Debug, Error)]
pub enum AssError {
    #[error("no [Events] section")]
    MissingEventsSection,
    #[error("no Format line in [Events]")]
    MissingFormat,
    #[error("invalid Dialogue at line {line_number}")]
    InvalidDialogue { line_number: usize },
}

static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^}]*\}").expect("valid regex"));
static LINE_BREAK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\[Nnh]").expect("valid regex"));
static COLOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\c&H([0-9A-Fa-f]+)").expect("valid regex"));
static SCALE_Y: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\fscy(\d+)").expect("valid regex"));

/// Parses the `[Events]` section of an ASS file. The caller removes the BOM.
pub fn parse(text: &str) -> Result<Vec<Line>, AssError> {
    let mut in_events = false;
    let mut events_seen = false;
    let mut format: Option<Vec<&str>> = None;
    let mut dialogue_count: u32 = 0;
    let mut lines: Vec<Line> = Vec::new();

    for (number, raw) in text.lines().enumerate() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') {
            in_events = trimmed == "[Events]";
            events_seen |= in_events;
            continue;
        }
        if !in_events {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("Format:") {
            format = Some(rest.split(',').map(str::trim).collect());
        } else if let Some(rest) = trimmed.strip_prefix("Dialogue:") {
            let fields = format.as_deref().ok_or(AssError::MissingFormat)?;
            let index = dialogue_count;
            dialogue_count += 1;
            let line_number = number + 1;
            let (start_ms, end_ms, body) =
                split_dialogue(rest, fields).ok_or(AssError::InvalidDialogue { line_number })?;
            add_dialogue(&mut lines, index, start_ms, end_ms, body);
        }
    }

    if !events_seen {
        return Err(AssError::MissingEventsSection);
    }
    if format.is_none() {
        return Err(AssError::MissingFormat);
    }
    lines.sort_by_key(|line| line.start_ms);
    Ok(lines)
}

/// Splits one `Dialogue` line. `Text` is the last field, so it can hold commas.
fn split_dialogue<'a>(rest: &'a str, fields: &[&str]) -> Option<(u32, u32, &'a str)> {
    let parts: Vec<&str> = rest.splitn(fields.len(), ',').collect();
    if parts.len() != fields.len() {
        return None;
    }
    let start = fields.iter().position(|field| *field == "Start")?;
    let end = fields.iter().position(|field| *field == "End")?;
    let text = fields.iter().position(|field| *field == "Text")?;
    Some((
        parse_time(parts[start].trim())?,
        parse_time(parts[end].trim())?,
        parts[text],
    ))
}

/// Parses `H:MM:SS.cc` into milliseconds.
fn parse_time(text: &str) -> Option<u32> {
    let (clock, centis) = text.split_once('.')?;
    let mut parts = clock.split(':');
    let hours: u32 = parts.next()?.parse().ok()?;
    let minutes: u32 = parts.next()?.parse().ok()?;
    let seconds: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let centis: u32 = centis.parse().ok()?;
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + centis * 10)
}

/// Adds one `Dialogue` to `lines`. It merges into the previous line when the times and color match.
fn add_dialogue(lines: &mut Vec<Line>, index: u32, start_ms: u32, end_ms: u32, raw: &str) {
    if is_ruby(raw) {
        return;
    }
    let color = COLOR
        .captures(raw)
        .and_then(|captures| parse_bgr(&captures[1]));
    let plain = TAG.replace_all(raw, "");
    let plain = LINE_BREAK.replace_all(&plain, " ");
    let offscreen = plain.contains(['⚟', '⚞']);
    let text = plain.replace(['⚟', '⚞'], "");
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let (speaker_label, text) = split_label(text);

    if speaker_label.is_none()
        && let Some(last) = lines.last_mut()
        && last.start_ms == start_ms
        && last.end_ms == end_ms
        && last.color == color
    {
        if last.text.is_empty() {
            last.text = text;
        } else {
            last.text = format!("{} {}", last.text, text);
        }
        last.offscreen |= offscreen;
        return;
    }

    lines.push(Line {
        index,
        start_ms,
        end_ms,
        text,
        color,
        offscreen,
        speaker_label,
    });
}

/// A `Dialogue` is ruby when its first override block scales the font height below 100.
fn is_ruby(raw: &str) -> bool {
    let Some(rest) = raw.strip_prefix('{') else {
        return false;
    };
    let block = rest.split('}').next().unwrap_or("");
    SCALE_Y
        .captures(block)
        .and_then(|captures| captures[1].parse::<u32>().ok())
        .is_some_and(|scale| scale < 100)
}

/// ASS colors are `&HBBGGRR`. This returns RGB.
fn parse_bgr(hex: &str) -> Option<u32> {
    let value = u32::from_str_radix(hex, 16).ok()? & 0xFF_FFFF;
    Some(((value & 0xFF) << 16) | (value & 0xFF00) | ((value >> 16) & 0xFF))
}

/// Splits `（label）text` into the label and the text. Other parentheses stay in the text.
fn split_label(text: &str) -> (Option<String>, String) {
    if let Some(rest) = text.strip_prefix('（')
        && let Some((label, after)) = rest.split_once('）')
    {
        return (Some(label.to_string()), after.trim().to_string());
    }
    (None, text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "[Script Info]\nTitle: test\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";

    fn document(rows: &[&str]) -> String {
        let mut text = HEADER.to_string();
        for row in rows {
            text.push_str(row);
            text.push('\n');
        }
        text
    }

    fn dialogue(start: &str, end: &str, text: &str) -> String {
        format!("Dialogue: 0,{start},{end},Default,,0000,0000,0000,,{text}")
    }

    #[test]
    fn a_ruby_dialogue_is_dropped() {
        let text = document(&[
            &dialogue("0:00:01.00", "0:00:02.00", r"{\pos(1,1)\fscx50\fscy50}ruby"),
            &dialogue("0:00:01.00", "0:00:02.00", "main"),
        ]);

        let lines = parse(&text).expect("the subtitle parses");

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "main");
    }

    #[test]
    fn tags_are_removed_and_line_breaks_become_spaces() {
        let text = document(&[&dialogue(
            "0:00:01.00",
            "0:00:02.00",
            r"{\an8}first\Nsecond\hthird",
        )]);

        let lines = parse(&text).expect("the subtitle parses");

        assert_eq!(lines[0].text, "first second third");
    }

    #[test]
    fn offscreen_marks_are_removed_and_set_the_flag() {
        let text = document(&[&dialogue("0:00:01.00", "0:00:02.00", "⚟ボタン⚞")]);

        let lines = parse(&text).expect("the subtitle parses");

        assert_eq!(lines[0].text, "ボタン");
        assert!(lines[0].offscreen);
    }

    #[test]
    fn a_leading_label_is_split_and_other_parentheses_stay() {
        let text = document(&[
            &dialogue("0:00:01.00", "0:00:02.00", "（あんな）わたし"),
            &dialogue("0:00:03.00", "0:00:04.00", "わたし（笑）"),
        ]);

        let lines = parse(&text).expect("the subtitle parses");

        assert_eq!(lines[0].speaker_label.as_deref(), Some("あんな"));
        assert_eq!(lines[0].text, "わたし");
        assert_eq!(lines[1].speaker_label, None);
        assert_eq!(lines[1].text, "わたし（笑）");
    }

    #[test]
    fn rows_with_the_same_time_and_color_merge_when_they_have_no_label() {
        let text = document(&[
            &dialogue("0:00:01.00", "0:00:02.00", "（あんな）わたし"),
            &dialogue("0:00:01.00", "0:00:02.00", "持つ"),
        ]);

        let lines = parse(&text).expect("the subtitle parses");

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "わたし 持つ");
        assert_eq!(lines[0].speaker_label.as_deref(), Some("あんな"));
        assert_eq!(lines[0].index, 0);
    }

    #[test]
    fn a_labelled_row_does_not_merge() {
        let text = document(&[
            &dialogue("0:00:01.00", "0:00:02.00", "one"),
            &dialogue("0:00:01.00", "0:00:02.00", "（みくる）two"),
        ]);

        let lines = parse(&text).expect("the subtitle parses");

        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn ass_color_is_converted_from_bgr_to_rgb() {
        let text = document(&[&dialogue("0:00:01.00", "0:00:02.00", r"{\c&H0000FFFF}黄")]);

        let lines = parse(&text).expect("the subtitle parses");

        assert_eq!(lines[0].color, Some(0x00FF_FF00));
    }

    #[test]
    fn lines_are_sorted_by_start_time_and_keep_file_order_for_equal_starts() {
        let text = document(&[
            &dialogue("0:00:05.00", "0:00:06.00", "late"),
            &dialogue("0:00:01.00", "0:00:02.00", "first"),
            &dialogue("0:00:01.00", "0:00:03.00", "second"),
        ]);

        let lines = parse(&text).expect("the subtitle parses");

        let texts: Vec<&str> = lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["first", "second", "late"]);
    }

    #[test]
    fn a_time_is_read_in_milliseconds() {
        let text = document(&[&dialogue("1:02:03.45", "1:02:04.00", "t")]);

        let lines = parse(&text).expect("the subtitle parses");

        assert_eq!(lines[0].start_ms, 3_723_450);
        assert_eq!(lines[0].end_ms, 3_724_000);
    }

    #[test]
    fn a_file_without_events_is_an_error() {
        let error = parse("[Script Info]\nTitle: x\n").expect_err("there is no events section");

        assert!(matches!(error, AssError::MissingEventsSection));
    }

    #[test]
    fn a_broken_dialogue_names_its_line() {
        let text = document(&["Dialogue: broken"]);

        let error = parse(&text).expect_err("the subtitle is invalid");

        assert!(matches!(
            error,
            AssError::InvalidDialogue { line_number: 6 }
        ));
    }

    #[test]
    fn the_sample_files_parse() {
        let raw = parse(include_str!("../../testdata/raw.ass")).expect("raw.ass parses");
        let chinese =
            parse(include_str!("../../testdata/chinese.ass")).expect("chinese.ass parses");

        assert_ne!(raw.len(), 0);
        assert_ne!(chinese.len(), 0);
        assert!(raw.iter().all(|line| line.text != "あけち"));
    }
}
