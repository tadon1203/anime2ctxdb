use crate::sources::ass::LineId;
use crate::speakers::suggestion::{Content, Suggestion, SuggestionId};
use crate::speakers::{Character, LabelKind, Speaker};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use thiserror::Error;

const TEMPLATE: &str = include_str!("../../prompts/speakers.md");

const FORMAT: &str = r##"{
  "new_characters": [{ "ref": "N1", "name": "...", "reading": "...", "evidence": ["L4"] }],
  "labels": [{ "label": "...", "kind": "character", "character": "N1" },
             { "label": "...", "kind": "sound" }],
  "colors": [{ "color": "#FFFF00", "character": "C1" }],
  "lines": [{ "line": "L88", "characters": ["C2"] },
            { "line": "L90", "characters": [] }]
}"##;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PromptId(pub u32);

impl fmt::Display for PromptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    Speakers,
}

/// A prompt file. The app writes it, and the user copies `text` to an AI chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub id: PromptId,
    pub kind: PromptKind,
    pub episode: u32,
    pub text: String,
    /// Short reference (`L1`, `C1`) to the real ID.
    pub refs: BTreeMap<String, String>,
}

/// What a prompt needs from the series.
pub struct PromptInput {
    pub lines: Vec<PromptLine>,
    pub characters: Vec<Character>,
    /// Labels with no approved decision.
    pub labels: Vec<String>,
    /// Colors with no approved decision.
    pub colors: Vec<u32>,
}

pub struct PromptLine {
    pub id: LineId,
    pub start_ms: u32,
    pub color: Option<u32>,
    pub label: Option<String>,
    pub text: String,
    pub speaker: Speaker,
}

#[derive(Debug, Error)]
pub enum AnswerError {
    #[error("no JSON object in the answer")]
    NoJson,
    #[error("invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unknown reference {0}")]
    UnknownRef(String),
    #[error("entry {0} has no evidence")]
    MissingEvidence(usize),
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

/// Builds a prompt. The short references are `L1` for lines and `C1` for characters, in order.
pub fn make(id: PromptId, kind: PromptKind, episode: u32, input: &PromptInput) -> Prompt {
    let mut refs = BTreeMap::new();
    let mut character_refs: HashMap<String, String> = HashMap::new();
    let mut character_lines = Vec::new();
    for (index, character) in input.characters.iter().enumerate() {
        let reference = format!("C{}", index + 1);
        refs.insert(reference.clone(), character.id.0.clone());
        let reading = character
            .reading
            .as_ref()
            .map(|reading| format!(" ({reading})"))
            .unwrap_or_default();
        character_lines.push(format!("{reference} {}{reading}", character.name));
        character_refs.insert(character.id.0.clone(), reference);
    }

    let mut transcript = Vec::new();
    for (index, line) in input.lines.iter().enumerate() {
        let reference = format!("L{}", index + 1);
        refs.insert(reference.clone(), line.id.0.clone());
        let mut parts = vec![reference, time_text(line.start_ms)];
        if let Some(color) = line.color {
            parts.push(format!("#{color:06X}"));
        }
        if let Some(label) = &line.label {
            parts.push(format!("（{label}）"));
        }
        parts.push(line.text.clone());
        parts.push(speaker_text(&line.speaker, &character_refs));
        transcript.push(parts.join(" "));
    }

    let colors: Vec<String> = input
        .colors
        .iter()
        .map(|color| format!("#{color:06X}"))
        .collect();
    let text = TEMPLATE
        .replace("{{characters}}", &list_or_none(&character_lines))
        .replace("{{transcript}}", &list_or_none(&transcript))
        .replace("{{labels}}", &list_or_none(&input.labels))
        .replace("{{colors}}", &list_or_none(&colors))
        .replace("{{format}}", FORMAT);

    Prompt {
        id,
        kind,
        episode,
        text,
        refs,
    }
}

/// Reads the answer and makes its suggestions. A new character `N1` becomes the suggestion of its own.
pub fn parse_answer(prompt: &Prompt, text: &str) -> Result<Vec<Suggestion>, AnswerError> {
    let json = json_part(text).ok_or(AnswerError::NoJson)?;
    let answer: Answer = serde_json::from_str(json)?;

    let mut position = 0;
    let mut next_id = || {
        position += 1;
        SuggestionId(format!("{}-{position}", prompt.id))
    };

    let mut suggestions = Vec::new();
    let mut new_refs: HashMap<String, String> = HashMap::new();
    for (index, character) in answer.new_characters.into_iter().enumerate() {
        if character.evidence.is_empty() {
            return Err(AnswerError::MissingEvidence(index + 1));
        }
        let evidence = character
            .evidence
            .iter()
            .map(|reference| line_ref(prompt, reference))
            .collect::<Result<Vec<LineId>, _>>()?;
        let id = next_id();
        new_refs.insert(character.reference, id.0.clone());
        suggestions.push(Suggestion {
            id,
            content: Content::Character {
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
            .map(|reference| character_ref(prompt, &new_refs, reference))
            .transpose()?;
        suggestions.push(Suggestion {
            id: next_id(),
            content: Content::Label {
                label: label.label,
                kind: label.kind,
                character,
            },
        });
    }

    for color in answer.colors {
        let character = character_ref(prompt, &new_refs, &color.character)?;
        suggestions.push(Suggestion {
            id: next_id(),
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
            .map(|reference| character_ref(prompt, &new_refs, reference))
            .collect::<Result<Vec<String>, _>>()?;
        suggestions.push(Suggestion {
            id: next_id(),
            content: Content::LineSpeaker {
                line: line_id,
                characters,
            },
        });
    }

    Ok(suggestions)
}

/// The JSON part of an answer: the first ```json block, or else the whole text.
pub fn json_part(text: &str) -> Option<&str> {
    const FENCE: &str = "```json";
    if let Some(start) = text.find(FENCE) {
        let body = &text[start + FENCE.len()..];
        let end = body.find("```")?;
        return Some(body[..end].trim());
    }
    text.contains('{').then(|| text.trim())
}

fn line_ref(prompt: &Prompt, reference: &str) -> Result<LineId, AnswerError> {
    prompt
        .refs
        .get(reference)
        .filter(|_| reference.starts_with('L'))
        .map(|id| LineId(id.clone()))
        .ok_or_else(|| AnswerError::UnknownRef(reference.to_string()))
}

/// A character reference is `C1` from the prompt, or `N1` for a new character of this answer.
fn character_ref(
    prompt: &Prompt,
    new_refs: &HashMap<String, String>,
    reference: &str,
) -> Result<String, AnswerError> {
    new_refs
        .get(reference)
        .cloned()
        .or_else(|| {
            prompt
                .refs
                .get(reference)
                .filter(|_| reference.starts_with('C'))
                .cloned()
        })
        .ok_or_else(|| AnswerError::UnknownRef(reference.to_string()))
}

fn time_text(ms: u32) -> String {
    let seconds = ms / 1000;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn speaker_text(speaker: &Speaker, character_refs: &HashMap<String, String>) -> String {
    match speaker {
        Speaker::Unknown => "[?]".to_string(),
        Speaker::Nobody => "[-]".to_string(),
        Speaker::Characters(ids, _) => {
            let refs: Vec<&str> = ids
                .iter()
                .map(|id| {
                    character_refs
                        .get(&id.0)
                        .expect("every speaker is a character of the prompt")
                        .as_str()
                })
                .collect();
            format!("[{}]", refs.join(","))
        }
    }
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speakers::CharacterId;

    fn prompt() -> Prompt {
        Prompt {
            id: PromptId(3),
            kind: PromptKind::Speakers,
            episode: 1,
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
        let plain = parse_answer(&prompt(), ANSWER).unwrap();
        let fenced =
            parse_answer(&prompt(), &format!("Here it is:\n```json\n{ANSWER}\n```")).unwrap();

        assert_eq!(plain, fenced);
        assert_eq!(plain.len(), 4);
    }

    #[test]
    fn new_characters_are_numbered_first_then_the_other_entries_in_order() {
        let suggestions = parse_answer(&prompt(), ANSWER).unwrap();

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
    fn an_unknown_reference_is_an_error() {
        let answer = r#"{ "lines": [{ "line": "L2", "characters": ["C9"] }] }"#;

        let error = parse_answer(&prompt(), answer).unwrap_err();

        assert!(matches!(error, AnswerError::UnknownRef(reference) if reference == "C9"));
    }

    #[test]
    fn a_new_character_needs_evidence() {
        let answer = r#"{ "new_characters": [{ "ref": "N1", "name": "x", "evidence": [] }] }"#;

        let error = parse_answer(&prompt(), answer).unwrap_err();

        assert!(matches!(error, AnswerError::MissingEvidence(1)));
    }

    #[test]
    fn text_without_json_is_an_error() {
        let error = parse_answer(&prompt(), "I cannot tell.").unwrap_err();

        assert!(matches!(error, AnswerError::NoJson));
    }

    #[test]
    fn make_writes_short_references_and_the_speaker_of_each_line() {
        let input = PromptInput {
            lines: vec![PromptLine {
                id: LineId("s:0".to_string()),
                start_ms: 61_000,
                color: Some(0xFFFF00),
                label: Some("あんな".to_string()),
                text: "わたし".to_string(),
                speaker: Speaker::Characters(
                    vec![CharacterId("2-1".to_string())],
                    crate::speakers::SpeakerMethod::Label,
                ),
            }],
            characters: vec![Character {
                id: CharacterId("2-1".to_string()),
                name: "明智あんな".to_string(),
                reading: None,
            }],
            labels: vec![],
            colors: vec![],
        };

        let prompt = make(PromptId(4), PromptKind::Speakers, 1, &input);

        assert!(
            prompt
                .text
                .contains("L1 01:01 #FFFF00 （あんな） わたし [C1]")
        );
        assert_eq!(prompt.refs["L1"], "s:0");
        assert_eq!(prompt.refs["C1"], "2-1");
    }
}
