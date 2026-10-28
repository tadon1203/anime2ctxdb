pub mod speakers;
pub mod story;

use crate::files::read_json;
use crate::model::{CHARACTER, Entity, EntityId, format_clock};
use crate::sources::ass::LineId;
use crate::speakers::Speaker;
use crate::suggestions::suggestion::{Evidence, Suggestion, SuggestionId};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use thiserror::Error;

pub use speakers::SpeakersInput;
pub use story::{PromptFact, StoryInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    Speakers,
    Story,
}

/// A prompt for one or more episodes. The app makes it when it is needed and does not save it.
/// The user copies `text` to an AI chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    /// The episodes of the transcript, in order.
    pub episodes: Vec<u32>,
    pub text: String,
    /// Short reference (`L1`, `C2-1`, `T3-1`) to the real ID. A line reference is the number of
    /// the line in the transcript. An entity reference has the ID of the entity, so it does not
    /// change when the user approves another entity.
    pub refs: BTreeMap<String, String>,
}

/// An answer file, `answers/<n>.json`. The app writes it when the user submits an answer. It holds
/// the suggestions of the answer, with real IDs, so the app does not need the prompt again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerFile {
    pub kind: PromptKind,
    pub episodes: Vec<u32>,
    pub suggestions: Vec<Suggestion>,
}

/// Reads the answer files in `answers/` with their numbers, in number order.
pub fn read_answers(series_dir: &Path) -> Result<Vec<(u32, AnswerFile)>> {
    let directory = series_dir.join("answers");
    let mut answers = Vec::new();
    for entry in
        fs::read_dir(&directory).with_context(|| format!("cannot read {}", directory.display()))?
    {
        let path = entry
            .with_context(|| format!("cannot read {}", directory.display()))?
            .path();
        let number = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| stem.parse::<u32>().ok());
        if let Some(number) = number.filter(|_| path.extension().is_some_and(|ext| ext == "json")) {
            answers.push((number, read_json::<AnswerFile>(&path)?));
        }
    }
    answers.sort_by_key(|(number, _)| *number);
    Ok(answers)
}

pub struct PromptLine {
    pub id: LineId,
    pub episode: u32,
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
    #[error("entry {position} of {section} has no evidence")]
    MissingEvidence {
        section: &'static str,
        position: usize,
    },
    #[error("scene {0} ends before it starts")]
    SceneOrder(usize),
}

/// Reads the answer and makes its suggestions. The kind of the prompt picks the parser.
/// `number` is the number of the answer. It starts the ID of each suggestion.
pub fn parse_answer(
    prompt: &Prompt,
    number: u32,
    text: &str,
) -> Result<Vec<Suggestion>, AnswerError> {
    let json = json_part(text).ok_or(AnswerError::NoJson)?;
    match prompt.kind {
        PromptKind::Speakers => speakers::parse(prompt, number, json),
        PromptKind::Story => story::parse(prompt, number, json),
    }
}

/// The JSON part of an answer: the first JSON code block, or else the whole text.
pub fn json_part(text: &str) -> Option<&str> {
    const FENCE: &str = "```json";
    if let Some(start) = text.find(FENCE) {
        let body = &text[start + FENCE.len()..];
        let end = body.find("```")?;
        return Some(body[..end].trim());
    }
    text.contains('{').then(|| text.trim())
}

/// Numbers the suggestions of one answer, from 1, in the order that they are made.
struct Numbering {
    answer: u32,
    position: usize,
}

impl Numbering {
    fn new(answer: u32) -> Numbering {
        Numbering {
            answer,
            position: 0,
        }
    }

    fn next_id(&mut self) -> SuggestionId {
        self.position += 1;
        SuggestionId(format!("{}-{}", self.answer, self.position))
    }
}

/// Writes the entity list of a prompt. A character gets a `C` reference and any other entity gets
/// a `T` reference. The reference has the ID of the entity, such as `C2-1`.
///
/// Returns the list lines, and the short reference of each entity by its ID.
fn entity_list(
    entities: &[Entity],
    refs: &mut BTreeMap<String, String>,
) -> (Vec<String>, HashMap<String, String>) {
    let mut lines = Vec::new();
    let mut entity_refs = HashMap::new();
    for entity in entities {
        let (reference, detail) = if entity.kind == CHARACTER {
            (format!("C{}", entity.id.0), entity.reading.clone())
        } else {
            (format!("T{}", entity.id.0), Some(entity.kind.clone()))
        };
        let detail = detail
            .map(|detail| format!(" ({detail})"))
            .unwrap_or_default();
        lines.push(format!("{reference} {}{detail}", entity.name));
        refs.insert(reference.clone(), entity.id.0.clone());
        entity_refs.insert(entity.id.0.clone(), reference);
    }
    (lines, entity_refs)
}

/// Writes the transcript lines with `L` references. A speaker is written with its entity reference.
fn transcript(
    lines: &[PromptLine],
    entity_refs: &HashMap<String, String>,
    refs: &mut BTreeMap<String, String>,
) -> Vec<String> {
    let mut transcript = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let reference = format!("L{}", index + 1);
        refs.insert(reference.clone(), line.id.0.clone());
        let mut parts = vec![
            reference,
            format!("E{}", line.episode),
            format_clock(line.start_ms),
        ];
        if let Some(color) = line.color {
            parts.push(format!("#{color:06X}"));
        }
        if let Some(label) = &line.label {
            parts.push(format!("（{label}）"));
        }
        parts.push(line.text.clone());
        parts.push(speaker_text(&line.speaker, entity_refs));
        transcript.push(parts.join(" "));
    }
    transcript
}

/// Reads the line references of one entry of an answer. An entry needs at least one line.
fn evidence_of(
    prompt: &Prompt,
    section: &'static str,
    index: usize,
    references: &[String],
) -> Result<Evidence, AnswerError> {
    let lines = references
        .iter()
        .map(|reference| line_ref(prompt, reference))
        .collect::<Result<Vec<LineId>, _>>()?;
    Evidence::new(lines).ok_or(AnswerError::MissingEvidence {
        section,
        position: index + 1,
    })
}

/// A line reference is `L1` from the prompt.
fn line_ref(prompt: &Prompt, reference: &str) -> Result<LineId, AnswerError> {
    prompt
        .refs
        .get(reference)
        .filter(|_| reference.starts_with('L'))
        .map(|id| LineId(id.clone()))
        .ok_or_else(|| AnswerError::UnknownRef(reference.to_string()))
}

/// An entity reference is `C1` or `T1` from the prompt, or `N1` for a new entity of this answer.
fn entity_ref(
    prompt: &Prompt,
    new_refs: &HashMap<String, EntityId>,
    reference: &str,
) -> Result<EntityId, AnswerError> {
    if let Some(id) = new_refs.get(reference) {
        return Ok(id.clone());
    }
    prompt
        .refs
        .get(reference)
        .filter(|_| reference.starts_with(['C', 'T']))
        .map(|id| EntityId(id.clone()))
        .ok_or_else(|| AnswerError::UnknownRef(reference.to_string()))
}

fn speaker_text(speaker: &Speaker, entity_refs: &HashMap<String, String>) -> String {
    match speaker {
        Speaker::Unknown => "[?]".to_string(),
        Speaker::Nobody => "[-]".to_string(),
        Speaker::Characters(ids, _) => {
            let refs: Vec<&str> = ids
                .iter()
                .map(|id| {
                    entity_refs
                        .get(&id.0)
                        .expect("every speaker is a character of the prompt")
                        .as_str()
                })
                .collect();
            format!("[{}]", refs.join(","))
        }
    }
}

/// The episodes in words, for example `episode 1` or `episodes 1, 2, 3`.
fn episodes_text(episodes: &[u32]) -> String {
    let numbers: Vec<String> = episodes.iter().map(u32::to_string).collect();
    let noun = if episodes.len() == 1 {
        "episode"
    } else {
        "episodes"
    };
    format!("{noun} {}", numbers.join(", "))
}

/// The lines of a list, or `none` for an empty list.
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

    #[test]
    fn episodes_are_named_in_words() {
        assert_eq!(episodes_text(&[4]), "episode 4");
        assert_eq!(episodes_text(&[1, 2, 5]), "episodes 1, 2, 5");
    }
}
