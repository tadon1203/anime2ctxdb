use super::{
    AnswerError, Prompt, PromptKind, PromptLine, entity_list, entity_name, evidence_of,
    line_number, line_ref, list_or_none, reference, speaker_text,
};
use crate::answers::answer::{Answer, ColorEntry, LabelEntry, LineEntry, NewEntity};
use crate::importance::Importance;
use crate::model::{CHARACTER, Entity, format_clock};
use crate::speakers::{LabelKind, Speaker};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, BTreeSet};

const TEMPLATE: &str = include_str!("../../../prompts/speakers.md");

const FORMAT: &str = r##"{
  "new_characters": [{ "name": "...", "reading": "...", "importance": "minor", "evidence": ["L4"] }],
  "labels": [{ "label": "...", "kind": "character", "character": "name" },
             { "label": "...", "kind": "sound" }],
  "colors": [{ "color": "#FFFF00", "character": "name" }],
  "lines": [{ "from": "L88", "to": "L92", "characters": ["name"] },
            { "from": "L95", "characters": [] }]
}"##;

/// What a Speakers prompt needs from the series.
pub struct SpeakersInput {
    pub lines: Vec<PromptLine>,
    /// The characters that are known at the episode.
    pub entities: Vec<Entity>,
    /// Labels with no meaning yet.
    pub labels: Vec<String>,
    /// Colors with no character yet.
    pub colors: Vec<u32>,
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    new_characters: Vec<ReplyCharacter>,
    #[serde(default)]
    labels: Vec<ReplyLabel>,
    #[serde(default)]
    colors: Vec<ReplyColor>,
    #[serde(default)]
    lines: Vec<ReplyLine>,
}

#[derive(Deserialize)]
struct ReplyCharacter {
    name: String,
    reading: Option<String>,
    importance: Option<Importance>,
    evidence: Vec<String>,
}

#[derive(Deserialize)]
struct ReplyLabel {
    label: String,
    kind: LabelKind,
    character: Option<String>,
}

#[derive(Deserialize)]
struct ReplyColor {
    #[serde(deserialize_with = "parse_color")]
    color: u32,
    character: String,
}

#[derive(Deserialize)]
struct ReplyLine {
    from: String,
    to: Option<String>,
    characters: Vec<String>,
}

fn parse_color<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let text = String::deserialize(deserializer)?;
    let hex = text
        .strip_prefix('#')
        .filter(|hex| hex.len() == 6)
        .ok_or_else(|| serde::de::Error::custom(format!("invalid color {text}")))?;
    u32::from_str_radix(hex, 16)
        .map_err(|_| serde::de::Error::custom(format!("invalid color {text}")))
}

/// Writes the lines of the episode, for example `L12 01:01 #FFFF00 （あんな） text [明智あんな]`.
///
/// A line that has a label or a color, and no speaker yet, has no speaker mark. The `labels` and
/// the `colors` of the answer give its speaker. `[?]` means that the answer must say the speaker.
fn transcript(lines: &[PromptLine], refs: &mut BTreeMap<String, String>) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let mut parts = vec![reference(line, refs), format_clock(line.start_ms)];
            if let Some(color) = line.color {
                parts.push(format!("#{color:06X}"));
            }
            if let Some(label) = &line.label {
                parts.push(format!("（{label}）"));
            }
            parts.push(line.text.clone());
            let solved_by_answer =
                line.speaker == Speaker::Unknown && (line.label.is_some() || line.color.is_some());
            if !solved_by_answer {
                parts.push(speaker_text(&line.speaker));
            }
            parts.join(" ")
        })
        .collect()
}

/// Builds a Speakers prompt for one episode. The short references are `L1` for lines, in order.
/// A character is named by its name.
pub fn make(episode: u32, input: &SpeakersInput) -> Prompt {
    let mut refs = BTreeMap::new();
    let character_lines = entity_list(&input.entities);
    let transcript_lines = transcript(&input.lines, &mut refs);

    let colors: Vec<String> = input
        .colors
        .iter()
        .map(|color| format!("#{color:06X}"))
        .collect();
    let text = TEMPLATE
        .replace("{{episode}}", &format!("episode {episode}"))
        .replace("{{characters}}", &list_or_none(&character_lines))
        .replace("{{transcript}}", &list_or_none(&transcript_lines))
        .replace("{{labels}}", &list_or_none(&input.labels))
        .replace("{{colors}}", &list_or_none(&colors))
        .replace("{{format}}", FORMAT);

    Prompt {
        kind: PromptKind::Speakers,
        episode,
        text,
        refs,
        known: input
            .entities
            .iter()
            .map(|entity| entity.name.clone())
            .collect(),
    }
}

/// Makes the answer from the reply. A character that the reply introduces can be used in it.
pub(super) fn parse(prompt: &Prompt, json: &str) -> Result<Answer, AnswerError> {
    let reply: Reply = serde_json::from_str(json)?;
    let declared: BTreeSet<String> = reply
        .new_characters
        .iter()
        .map(|character| character.name.clone())
        .collect();

    let mut characters = Vec::new();
    for (index, character) in reply.new_characters.into_iter().enumerate() {
        characters.push(NewEntity {
            kind: CHARACTER.to_string(),
            evidence: evidence_of(prompt, "new_characters", index, &character.evidence)?,
            name: character.name,
            reading: character.reading,
            importance: character.importance,
        });
    }

    let mut labels = Vec::new();
    for label in reply.labels {
        let character = label
            .character
            .as_deref()
            .map(|name| entity_name(prompt, &declared, name))
            .transpose()?;
        labels.push(LabelEntry {
            label: label.label,
            kind: label.kind,
            character,
        });
    }

    let mut colors = Vec::new();
    for color in reply.colors {
        colors.push(ColorEntry {
            color: color.color,
            character: entity_name(prompt, &declared, &color.character)?,
        });
    }

    let mut lines = Vec::new();
    for (index, range) in reply.lines.into_iter().enumerate() {
        let characters = range
            .characters
            .iter()
            .map(|name| entity_name(prompt, &declared, name))
            .collect::<Result<Vec<String>, _>>()?;
        let first = line_ref(prompt, &range.from)?;
        let (first_number, last_number) = match &range.to {
            Some(to) => {
                line_ref(prompt, to)?;
                (line_number(&range.from), line_number(to))
            }
            None => (line_number(&range.from), line_number(&range.from)),
        };
        if last_number < first_number {
            return Err(AnswerError::LineRange(index + 1));
        }
        for number in first_number..=last_number {
            let line = if number == first_number {
                first.clone()
            } else {
                line_ref(prompt, &format!("L{number}"))?
            };
            lines.push(LineEntry {
                line,
                characters: characters.clone(),
            });
        }
    }

    Ok(Answer::Speakers {
        episode: prompt.episode,
        characters,
        labels,
        colors,
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::answers::answer::Evidence;
    use crate::model::EntityId;
    use crate::sources::ass::LineId;
    use crate::speakers::{Speaker, SpeakerMethod};

    fn prompt() -> Prompt {
        Prompt {
            kind: PromptKind::Speakers,
            episode: 1,
            text: String::new(),
            refs: BTreeMap::from([
                ("L1".to_string(), "s:0".to_string()),
                ("L2".to_string(), "s:1".to_string()),
            ]),
            known: BTreeSet::from(["坂本みくる".to_string()]),
        }
    }

    const ANSWER: &str = r##"{
      "new_characters": [{ "name": "明智あんな", "evidence": ["L1"] }],
      "labels": [{ "label": "あんな", "kind": "character", "character": "明智あんな" }],
      "colors": [{ "color": "#FFFF00", "character": "坂本みくる" }],
      "lines": [{ "from": "L2", "characters": ["明智あんな"] }]
    }"##;

    fn parse_text(text: &str) -> Result<Answer, AnswerError> {
        super::super::parse_answer(&prompt(), text)
    }

    #[test]
    fn a_fenced_answer_and_a_plain_answer_give_the_same_answer() {
        let plain = parse_text(ANSWER).expect("the plain answer parses");
        let fenced = parse_text(&format!("Here it is:\n```json\n{ANSWER}\n```"))
            .expect("the fenced answer parses");

        assert_eq!(plain, fenced);
        assert_eq!(plain.count(), 4);
    }

    #[test]
    fn the_answer_has_real_line_ids_and_character_names() {
        let answer = parse_text(ANSWER).expect("the answer parses");

        let Answer::Speakers {
            episode,
            characters,
            labels,
            lines,
            ..
        } = answer
        else {
            panic!("a speakers answer");
        };
        assert_eq!(episode, 1);
        assert_eq!(characters[0].kind, CHARACTER);
        assert_eq!(
            characters[0].evidence,
            Evidence::new(vec![LineId("s:0".to_string())]).expect("one line")
        );
        assert_eq!(labels[0].character.as_deref(), Some("明智あんな"));
        assert_eq!(lines[0].line, LineId("s:1".to_string()));
    }

    #[test]
    fn an_unknown_line_is_an_error() {
        let error = parse_text(r#"{ "lines": [{ "from": "L9", "characters": [] }] }"#)
            .expect_err("L9 is unknown");

        assert!(matches!(error, AnswerError::UnknownRef(reference) if reference == "L9"));
    }

    #[test]
    fn a_range_gives_every_line_in_it_the_same_speakers() {
        let answer = parse_text(
            r#"{ "lines": [{ "from": "L1", "to": "L2", "characters": ["坂本みくる"] }] }"#,
        )
        .expect("the answer parses");

        let Answer::Speakers { lines, .. } = answer else {
            panic!("a speakers answer");
        };
        let ids: Vec<&str> = lines.iter().map(|entry| entry.line.0.as_str()).collect();
        assert_eq!(ids, ["s:0", "s:1"]);
        assert!(lines.iter().all(|entry| entry.characters == ["坂本みくる"]));
    }

    #[test]
    fn a_range_that_ends_before_it_starts_is_an_error() {
        let error = parse_text(r#"{ "lines": [{ "from": "L2", "to": "L1", "characters": [] }] }"#)
            .expect_err("the range is backwards");

        assert!(matches!(error, AnswerError::LineRange(1)));
    }

    #[test]
    fn an_unknown_character_is_an_error() {
        let error = parse_text(r#"{ "lines": [{ "from": "L2", "characters": ["誰か"] }] }"#)
            .expect_err("the name is unknown");

        assert!(matches!(error, AnswerError::UnknownEntity(name) if name == "誰か"));
    }

    #[test]
    fn a_new_character_needs_evidence() {
        let error = parse_text(r#"{ "new_characters": [{ "name": "x", "evidence": [] }] }"#)
            .expect_err("the evidence is missing");

        assert!(matches!(
            error,
            AnswerError::MissingEvidence {
                section: "new_characters",
                position: 1
            }
        ));
    }

    #[test]
    fn text_without_json_is_an_error() {
        let error = parse_text("I cannot tell.").expect_err("there is no JSON");

        assert!(matches!(error, AnswerError::NoJson));
    }

    #[test]
    fn make_writes_references_and_the_speaker_names_of_each_line() {
        let input = SpeakersInput {
            lines: vec![PromptLine {
                id: LineId("s:0".to_string()),
                start_ms: 61_000,
                color: Some(0x00FF_FF00),
                label: Some("あんな".to_string()),
                text: "わたし".to_string(),
                speaker: Speaker::Characters(
                    vec![EntityId("明智あんな".to_string())],
                    SpeakerMethod::Label,
                ),
            }],
            entities: vec![Entity {
                id: EntityId("明智あんな".to_string()),
                kind: CHARACTER.to_string(),
                name: "明智あんな".to_string(),
                reading: None,
                importance: Importance::Main,
            }],
            labels: vec![],
            colors: vec![],
        };

        let prompt = make(1, &input);

        assert!(
            prompt
                .text
                .contains("L1 01:01 #FFFF00 （あんな） わたし [明智あんな]")
        );
        assert_eq!(prompt.refs["L1"], "s:0");
        assert!(prompt.known.contains("明智あんな"));
    }

    #[test]
    fn only_a_line_with_no_label_and_no_color_asks_for_a_speaker() {
        let line = |label: Option<&str>, color: Option<u32>| PromptLine {
            id: LineId("s:0".to_string()),
            start_ms: 0,
            color,
            label: label.map(str::to_string),
            text: "t".to_string(),
            speaker: Speaker::Unknown,
        };
        let input = SpeakersInput {
            lines: vec![
                line(None, None),
                line(Some("あんな"), None),
                line(None, Some(0x00FF_FF00)),
            ],
            entities: vec![],
            labels: vec![],
            colors: vec![],
        };

        let prompt = make(1, &input);

        assert!(prompt.text.contains("L1 00:00 t [?]"));
        assert!(prompt.text.contains("L2 00:00 （あんな） t\n"));
        assert!(prompt.text.contains("L3 00:00 #FFFF00 t\n"));
    }
}
