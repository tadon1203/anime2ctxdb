pub mod speakers;
pub mod story;

use crate::model::{CHARACTER, Entity, EntityId, format_clock};
use crate::sources::ass::LineId;
use crate::speakers::Speaker;
use crate::suggestions::suggestion::{Evidence, Suggestion, SuggestionId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use thiserror::Error;

pub use speakers::SpeakersInput;
pub use story::{PromptFact, StoryInput};

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
    Story,
}

/// A prompt file. The app writes it, and the user copies `text` to an AI chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub id: PromptId,
    pub kind: PromptKind,
    pub episode: u32,
    pub text: String,
    /// Short reference (`L1`, `C1`, `T1`) to the real ID.
    pub refs: BTreeMap<String, String>,
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
    #[error("entry {position} of {section} has no evidence")]
    MissingEvidence {
        section: &'static str,
        position: usize,
    },
    #[error("scene {0} ends before it starts")]
    SceneOrder(usize),
}

/// Reads the answer and makes its suggestions. The kind of the prompt picks the parser.
pub fn parse_answer(prompt: &Prompt, text: &str) -> Result<Vec<Suggestion>, AnswerError> {
    let json = json_part(text).ok_or(AnswerError::NoJson)?;
    match prompt.kind {
        PromptKind::Speakers => speakers::parse(prompt, json),
        PromptKind::Story => story::parse(prompt, json),
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
    prompt: PromptId,
    position: usize,
}

impl Numbering {
    fn new(prompt: PromptId) -> Numbering {
        Numbering {
            prompt,
            position: 0,
        }
    }

    fn next_id(&mut self) -> SuggestionId {
        self.position += 1;
        SuggestionId(format!("{}-{}", self.prompt, self.position))
    }
}

/// Writes the entity list of a prompt. A character gets a `C` reference and any other entity gets a `T` reference.
///
/// Returns the list lines, and the short reference of each entity by its ID.
fn entity_list(
    entities: &[Entity],
    refs: &mut BTreeMap<String, String>,
) -> (Vec<String>, HashMap<String, String>) {
    let mut lines = Vec::new();
    let mut entity_refs = HashMap::new();
    let mut characters = 0;
    let mut others = 0;
    for entity in entities {
        let (reference, detail) = if entity.kind == CHARACTER {
            characters += 1;
            (format!("C{characters}"), entity.reading.clone())
        } else {
            others += 1;
            (format!("T{others}"), Some(entity.kind.clone()))
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
        let mut parts = vec![reference, format_clock(line.start_ms)];
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

/// The lines of a list, or `none` for an empty list.
fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join("\n")
    }
}
