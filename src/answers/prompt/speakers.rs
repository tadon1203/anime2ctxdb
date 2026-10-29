use super::{
    AnswerError, Prompt, PromptKind, PromptLine, entity_list, entity_name, evidence_of, line_ref,
    list_or_none, transcript,
};
use crate::answers::answer::{Answer, ColorEntry, LabelEntry, LineEntry, NewEntity};
use crate::model::{CHARACTER, Entity};
use crate::speakers::LabelKind;
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, BTreeSet};

const TEMPLATE: &str = include_str!("../../../prompts/speakers.md");

const FORMAT: &str = r##"{
  "new_characters": [{ "name": "...", "reading": "...", "evidence": ["L4"] }],
  "labels": [{ "label": "...", "kind": "character", "character": "name" },
             { "label": "...", "kind": "sound" }],
  "colors": [{ "color": "#FFFF00", "character": "name" }],
  "lines": [{ "line": "L88", "characters": ["name"] },
            { "line": "L90", "characters": [] }]
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
    line: String,
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
    for line in reply.lines {
        lines.push(LineEntry {
            line: line_ref(prompt, &line.line)?,
            characters: line
                .characters
                .iter()
                .map(|name| entity_name(prompt, &declared, name))
                .collect::<Result<Vec<String>, _>>()?,
        });
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
      "lines": [{ "line": "L2", "characters": ["明智あんな"] }]
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
        let error = parse_text(r#"{ "lines": [{ "line": "L9", "characters": [] }] }"#)
            .expect_err("L9 is unknown");

        assert!(matches!(error, AnswerError::UnknownRef(reference) if reference == "L9"));
    }

    #[test]
    fn an_unknown_character_is_an_error() {
        let error = parse_text(r#"{ "lines": [{ "line": "L2", "characters": ["誰か"] }] }"#)
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
                episode: 1,
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
            }],
            labels: vec![],
            colors: vec![],
        };

        let prompt = make(1, &input);

        assert!(
            prompt
                .text
                .contains("L1 E1 01:01 #FFFF00 （あんな） わたし [明智あんな]")
        );
        assert_eq!(prompt.refs["L1"], "s:0");
        assert!(prompt.known.contains("明智あんな"));
    }
}
