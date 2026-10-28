use super::{
    AnswerError, Numbering, Prompt, PromptId, PromptKind, PromptLine, entity_list, entity_ref,
    episodes_text, evidence_of, line_ref, list_or_none, transcript,
};
use crate::model::{CHARACTER, Entity, EntityId};
use crate::speakers::LabelKind;
use crate::suggestions::suggestion::{Content, Suggestion};
use serde::{Deserialize, Deserializer};
use std::collections::{BTreeMap, HashMap};

const TEMPLATE: &str = include_str!("../../../prompts/speakers.md");

const FORMAT: &str = r##"{
  "new_characters": [{ "ref": "N1", "name": "...", "reading": "...", "evidence": ["L4"] }],
  "labels": [{ "label": "...", "kind": "character", "character": "N1" },
             { "label": "...", "kind": "sound" }],
  "colors": [{ "color": "#FFFF00", "character": "C1" }],
  "lines": [{ "line": "L88", "characters": ["C2"] },
            { "line": "L90", "characters": [] }]
}"##;

/// What a Speakers prompt needs from the series.
pub struct SpeakersInput {
    pub lines: Vec<PromptLine>,
    /// The approved entities of kind `character`.
    pub entities: Vec<Entity>,
    /// Labels with no approved decision.
    pub labels: Vec<String>,
    /// Colors with no approved decision.
    pub colors: Vec<u32>,
}

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    new_characters: Vec<NewCharacter>,
    #[serde(default)]
    labels: Vec<AnswerLabel>,
    #[serde(default)]
    colors: Vec<AnswerColor>,
    #[serde(default)]
    lines: Vec<AnswerLine>,
}

#[derive(Deserialize)]
struct NewCharacter {
    #[serde(rename = "ref")]
    reference: String,
    name: String,
    reading: Option<String>,
    evidence: Vec<String>,
}

#[derive(Deserialize)]
struct AnswerLabel {
    label: String,
    kind: LabelKind,
    character: Option<String>,
}

#[derive(Deserialize)]
struct AnswerColor {
    #[serde(deserialize_with = "parse_color")]
    color: u32,
    character: String,
}

#[derive(Deserialize)]
struct AnswerLine {
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

/// Builds a Speakers prompt. The short references are `L1` for lines and `C1` for characters, in order.
pub fn make(id: PromptId, episodes: &[u32], input: &SpeakersInput) -> Prompt {
    let mut refs = BTreeMap::new();
    let (character_lines, entity_refs) = entity_list(&input.entities, &mut refs);
    let transcript_lines = transcript(&input.lines, &entity_refs, &mut refs);

    let colors: Vec<String> = input
        .colors
        .iter()
        .map(|color| format!("#{color:06X}"))
        .collect();
    let text = TEMPLATE
        .replace("{{episodes}}", &episodes_text(episodes))
        .replace("{{characters}}", &list_or_none(&character_lines))
        .replace("{{transcript}}", &list_or_none(&transcript_lines))
        .replace("{{labels}}", &list_or_none(&input.labels))
        .replace("{{colors}}", &list_or_none(&colors))
        .replace("{{format}}", FORMAT);

    Prompt {
        id,
        kind: PromptKind::Speakers,
        episodes: episodes.to_vec(),
        text,
        refs,
    }
}

/// Makes the suggestions of a Speakers answer. A new character `N1` becomes the suggestion of its own.
pub(super) fn parse(prompt: &Prompt, json: &str) -> Result<Vec<Suggestion>, AnswerError> {
    let answer: Answer = serde_json::from_str(json)?;
    let mut numbering = Numbering::new(prompt.id);
    let mut suggestions = Vec::new();

    let mut new_refs: HashMap<String, EntityId> = HashMap::new();
    for (index, character) in answer.new_characters.into_iter().enumerate() {
        let evidence = evidence_of(prompt, "new_characters", index, &character.evidence)?;
        let id = numbering.next_id();
        new_refs.insert(character.reference, EntityId(id.0.clone()));
        suggestions.push(Suggestion {
            id,
            content: Content::Entity {
                kind: CHARACTER.to_string(),
                name: character.name,
                reading: character.reading,
                evidence,
            },
        });
    }

    for label in answer.labels {
        let character = label
            .character
            .as_deref()
            .map(|reference| entity_ref(prompt, &new_refs, reference))
            .transpose()?
            .map(|id| id.0);
        suggestions.push(Suggestion {
            id: numbering.next_id(),
            content: Content::Label {
                label: label.label,
                kind: label.kind,
                character,
            },
        });
    }

    for color in answer.colors {
        let character = entity_ref(prompt, &new_refs, &color.character)?.0;
        suggestions.push(Suggestion {
            id: numbering.next_id(),
            content: Content::Color {
                color: color.color,
                character,
            },
        });
    }

    for line in answer.lines {
        let line_id = line_ref(prompt, &line.line)?;
        let characters = line
            .characters
            .iter()
            .map(|reference| entity_ref(prompt, &new_refs, reference).map(|id| id.0))
            .collect::<Result<Vec<String>, _>>()?;
        suggestions.push(Suggestion {
            id: numbering.next_id(),
            content: Content::LineSpeaker {
                line: line_id,
                characters,
            },
        });
    }

    Ok(suggestions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EntityId;
    use crate::sources::ass::LineId;
    use crate::speakers::{Speaker, SpeakerMethod};
    use crate::suggestions::suggestion::Evidence;

    fn prompt() -> Prompt {
        Prompt {
            id: PromptId(3),
            kind: PromptKind::Speakers,
            episodes: vec![1],
            text: String::new(),
            refs: BTreeMap::from([
                ("L1".to_string(), "s:0".to_string()),
                ("L2".to_string(), "s:1".to_string()),
                ("C1".to_string(), "2-1".to_string()),
            ]),
        }
    }

    const ANSWER: &str = r##"{
      "new_characters": [{ "ref": "N1", "name": "明智あんな", "evidence": ["L1"] }],
      "labels": [{ "label": "あんな", "kind": "character", "character": "N1" }],
      "colors": [{ "color": "#FFFF00", "character": "C1" }],
      "lines": [{ "line": "L2", "characters": ["N1"] }]
    }"##;

    #[test]
    fn a_fenced_answer_and_a_plain_answer_give_the_same_suggestions() {
        let plain = super::super::parse_answer(&prompt(), ANSWER).expect("the plain answer parses");
        let fenced =
            super::super::parse_answer(&prompt(), &format!("Here it is:\n```json\n{ANSWER}\n```"))
                .expect("the fenced answer parses");

        assert_eq!(plain, fenced);
        assert_eq!(plain.len(), 4);
    }

    #[test]
    fn new_characters_are_numbered_first_then_the_other_entries_in_order() {
        let suggestions = super::super::parse_answer(&prompt(), ANSWER).expect("the answer parses");

        let ids: Vec<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.id.0.as_str())
            .collect();
        assert_eq!(ids, ["3-1", "3-2", "3-3", "3-4"]);
        assert_eq!(
            suggestions[1].content,
            Content::Label {
                label: "あんな".to_string(),
                kind: LabelKind::Character,
                character: Some("3-1".to_string()),
            }
        );
    }

    #[test]
    fn a_new_character_is_an_entity_of_kind_character() {
        let suggestions = super::super::parse_answer(&prompt(), ANSWER).expect("the answer parses");

        assert!(matches!(
            &suggestions[0].content,
            Content::Entity { kind, .. } if kind == CHARACTER
        ));
    }

    #[test]
    fn an_unknown_reference_is_an_error() {
        let answer = r#"{ "lines": [{ "line": "L2", "characters": ["C9"] }] }"#;

        let error = super::super::parse_answer(&prompt(), answer).expect_err("C9 is unknown");

        assert!(matches!(error, AnswerError::UnknownRef(reference) if reference == "C9"));
    }

    #[test]
    fn a_new_character_needs_evidence() {
        let answer = r#"{ "new_characters": [{ "ref": "N1", "name": "x", "evidence": [] }] }"#;

        let error =
            super::super::parse_answer(&prompt(), answer).expect_err("the evidence is missing");

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
        let error =
            super::super::parse_answer(&prompt(), "I cannot tell.").expect_err("there is no JSON");

        assert!(matches!(error, AnswerError::NoJson));
    }

    #[test]
    fn make_writes_short_references_and_the_speaker_of_each_line() {
        let input = SpeakersInput {
            lines: vec![PromptLine {
                id: LineId("s:0".to_string()),
                episode: 1,
                start_ms: 61_000,
                color: Some(0x00FF_FF00),
                label: Some("あんな".to_string()),
                text: "わたし".to_string(),
                speaker: Speaker::Characters(
                    vec![EntityId("2-1".to_string())],
                    SpeakerMethod::Label,
                ),
            }],
            entities: vec![Entity {
                id: EntityId("2-1".to_string()),
                kind: CHARACTER.to_string(),
                name: "明智あんな".to_string(),
                reading: None,
            }],
            labels: vec![],
            colors: vec![],
        };

        let prompt = make(PromptId(4), &[1], &input);

        assert!(
            prompt
                .text
                .contains("L1 E1 01:01 #FFFF00 （あんな） わたし [C1]")
        );
        assert_eq!(prompt.refs["L1"], "s:0");
        assert_eq!(prompt.refs["C1"], "2-1");
    }

    #[test]
    fn evidence_is_kept_in_order() {
        let suggestions = super::super::parse_answer(&prompt(), ANSWER).expect("the answer parses");

        assert_eq!(
            suggestions[0].content,
            Content::Entity {
                kind: CHARACTER.to_string(),
                name: "明智あんな".to_string(),
                reading: None,
                evidence: Evidence::new(vec![LineId("s:0".to_string())]).expect("one line"),
            }
        );
    }
}
