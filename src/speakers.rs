use crate::model::EntityId;
use crate::sources::ass::{self, LineId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelKind {
    Character,
    /// A person who is not a character, for example 女の子.
    Extra,
    Sound,
    Note,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeakerMethod {
    Label,
    Color,
    PreviousLine,
    Ai,
}

/// Who speaks one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Speaker {
    /// The rules cannot tell.
    Unknown,
    /// A sound, an extra, or an approved empty list.
    Nobody,
    Characters(Vec<EntityId>, SpeakerMethod),
}

/// An approved speaker label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    pub kind: LabelKind,
    pub character: Option<EntityId>,
}

/// The largest gap, in ms, from the end of an earlier line to the start of a line that copies its speaker.
const PREVIOUS_LINE_GAP_MS: i64 = 10_000;

/// Finds the speaker of each line of one episode. The lines are in time order.
///
/// The result has the same order as `lines`.
pub fn resolve(
    lines: &[(LineId, &ass::Line)],
    labels: &HashMap<String, Label>,
    colors: &HashMap<u32, EntityId>,
    line_speakers: &HashMap<LineId, Vec<EntityId>>,
) -> Vec<Speaker> {
    let mut speakers: Vec<Speaker> = Vec::with_capacity(lines.len());
    for (position, (id, line)) in lines.iter().enumerate() {
        let speaker = match resolve_one(id, line, labels, colors, line_speakers) {
            Some(speaker) => speaker,
            None => previous_line(lines, &speakers, position),
        };
        speakers.push(speaker);
    }
    speakers
}

/// The rules that need the line only. `None` means the line has no label and no color.
/// A line with a `Note` label also gives `None`, so the previous-line rule applies.
fn resolve_one(
    id: &LineId,
    line: &ass::Line,
    labels: &HashMap<String, Label>,
    colors: &HashMap<u32, EntityId>,
    line_speakers: &HashMap<LineId, Vec<EntityId>>,
) -> Option<Speaker> {
    if let Some(characters) = line_speakers.get(id) {
        return Some(if characters.is_empty() {
            Speaker::Nobody
        } else {
            Speaker::Characters(characters.clone(), SpeakerMethod::Ai)
        });
    }
    if let Some(label_text) = line.speaker_label.as_deref()
        && let Some(speaker) = label_speaker(label_text, labels)
    {
        return Some(speaker);
    }
    let color = line.color?;
    Some(match colors.get(&color) {
        Some(character) => Speaker::Characters(vec![character.clone()], SpeakerMethod::Color),
        None => Speaker::Unknown,
    })
}

/// Resolves a label such as `あんな・みくる`. `None` when a part is a `Note`.
fn label_speaker(label_text: &str, labels: &HashMap<String, Label>) -> Option<Speaker> {
    let found: Vec<Option<&Label>> = label_text
        .split('・')
        .map(|part| labels.get(part))
        .collect();
    if found.iter().any(Option::is_none) {
        return Some(Speaker::Unknown);
    }
    let found: Vec<&Label> = found.into_iter().flatten().collect();
    if found.iter().any(|label| label.kind == LabelKind::Note) {
        return None;
    }
    let mut characters: Vec<EntityId> = Vec::new();
    for label in found
        .iter()
        .filter(|label| label.kind == LabelKind::Character)
    {
        match &label.character {
            Some(character) => characters.push(character.clone()),
            None => return Some(Speaker::Unknown),
        }
    }
    Some(if characters.is_empty() {
        Speaker::Nobody
    } else {
        Speaker::Characters(characters, SpeakerMethod::Label)
    })
}

/// Copies the speaker of the nearest earlier line without a color.
///
/// The copy happens only when that speaker came from a label or from an earlier line,
/// and when the gap is at most 10 seconds.
fn previous_line(lines: &[(LineId, &ass::Line)], speakers: &[Speaker], position: usize) -> Speaker {
    let line = lines[position].1;
    let Some(earlier) = (0..position)
        .rev()
        .find(|&earlier| lines[earlier].1.color.is_none())
    else {
        return Speaker::Unknown;
    };
    let Speaker::Characters(characters, method) = &speakers[earlier] else {
        return Speaker::Unknown;
    };
    if !matches!(method, SpeakerMethod::Label | SpeakerMethod::PreviousLine) {
        return Speaker::Unknown;
    }
    let gap = i64::from(line.start_ms) - i64::from(lines[earlier].1.end_ms);
    if gap <= PREVIOUS_LINE_GAP_MS {
        Speaker::Characters(characters.clone(), SpeakerMethod::PreviousLine)
    } else {
        Speaker::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start_ms: u32, end_ms: u32, color: Option<u32>, label: Option<&str>) -> ass::Line {
        ass::Line {
            index: start_ms,
            start_ms,
            end_ms,
            text: "text".to_string(),
            color,
            offscreen: false,
            speaker_label: label.map(str::to_string),
        }
    }

    fn character(id: &str) -> EntityId {
        EntityId(id.to_string())
    }

    fn labels() -> HashMap<String, Label> {
        HashMap::from([
            (
                "あんな".to_string(),
                Label {
                    kind: LabelKind::Character,
                    character: Some(character("c1")),
                },
            ),
            (
                "みくる".to_string(),
                Label {
                    kind: LabelKind::Character,
                    character: Some(character("c2")),
                },
            ),
            (
                "窓".to_string(),
                Label {
                    kind: LabelKind::Sound,
                    character: None,
                },
            ),
            (
                "注".to_string(),
                Label {
                    kind: LabelKind::Note,
                    character: None,
                },
            ),
        ])
    }

    fn id(name: &str) -> LineId {
        LineId(name.to_string())
    }

    #[test]
    fn the_speaker_rules_run_in_order() {
        let colors = HashMap::from([(0x00FF_FF00, character("c3"))]);
        let line_speakers = HashMap::from([(id("s:4"), vec![character("c4")])]);
        let lines = [
            (id("s:0"), line(0, 2_000, None, Some("あんな"))),
            (id("s:1"), line(2_000, 3_000, None, Some("あんな・みくる"))),
            (id("s:2"), line(3_000, 4_000, None, Some("窓"))),
            (id("s:3"), line(4_000, 5_000, Some(0x00FF_FF00), None)),
            (id("s:4"), line(5_000, 6_000, None, Some("あんな"))),
            (id("s:5"), line(6_000, 7_000, None, Some("未知"))),
            (id("s:6"), line(7_000, 8_000, None, None)),
            (id("s:7"), line(30_000, 31_000, None, None)),
            (
                id("s:8"),
                line(31_000, 32_000, Some(0x0012_3456), Some("注")),
            ),
        ];
        let refs: Vec<(LineId, &ass::Line)> =
            lines.iter().map(|(id, line)| (id.clone(), line)).collect();

        let speakers = resolve(&refs, &labels(), &colors, &line_speakers);

        assert_eq!(
            speakers[0],
            Speaker::Characters(vec![character("c1")], SpeakerMethod::Label)
        );
        assert_eq!(
            speakers[1],
            Speaker::Characters(vec![character("c1"), character("c2")], SpeakerMethod::Label)
        );
        assert_eq!(speakers[2], Speaker::Nobody);
        assert_eq!(
            speakers[3],
            Speaker::Characters(vec![character("c3")], SpeakerMethod::Color)
        );
        assert_eq!(
            speakers[4],
            Speaker::Characters(vec![character("c4")], SpeakerMethod::Ai)
        );
        assert_eq!(speakers[5], Speaker::Unknown);
        assert_eq!(speakers[6], Speaker::Unknown);
        assert_eq!(speakers[7], Speaker::Unknown);
        assert_eq!(speakers[8], Speaker::Unknown);
    }

    #[test]
    fn an_extra_label_is_nobody_even_with_a_character_attached() {
        let mut labels = labels();
        labels.insert(
            "女の子".to_string(),
            Label {
                kind: LabelKind::Extra,
                character: Some(character("c1")),
            },
        );
        let lines = [(id("s:0"), line(0, 2_000, None, Some("女の子")))];
        let refs: Vec<(LineId, &ass::Line)> =
            lines.iter().map(|(id, line)| (id.clone(), line)).collect();

        let speakers = resolve(&refs, &labels, &HashMap::new(), &HashMap::new());

        assert_eq!(speakers[0], Speaker::Nobody);
    }

    #[test]
    fn a_line_without_color_or_label_takes_the_previous_speaker_within_ten_seconds() {
        let lines = [
            (id("s:0"), line(0, 2_000, None, Some("あんな"))),
            (id("s:1"), line(10_000, 11_000, None, None)),
            (id("s:2"), line(30_000, 31_000, None, None)),
        ];
        let refs: Vec<(LineId, &ass::Line)> =
            lines.iter().map(|(id, line)| (id.clone(), line)).collect();

        let speakers = resolve(&refs, &labels(), &HashMap::new(), &HashMap::new());

        assert_eq!(
            speakers[1],
            Speaker::Characters(vec![character("c1")], SpeakerMethod::PreviousLine)
        );
        assert_eq!(speakers[2], Speaker::Unknown);
    }
}
