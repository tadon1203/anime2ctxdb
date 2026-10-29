pub mod speakers;
pub mod story;

use crate::answers::answer::{Answer, Evidence};
use crate::model::{CHARACTER, Entity};
use crate::sources::ass::LineId;
use crate::speakers::Speaker;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub use speakers::SpeakersInput;
pub use story::{PromptFact, StoryInput};

/// The order is the order of the answer files of one episode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptKind {
    Speakers,
    Story,
}

/// The prompt for one episode. The app makes it when it is needed and does not save it. The user
/// copies `text` to an AI chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub kind: PromptKind,
    pub episode: u32,
    pub text: String,
    /// Short reference (`L1`, `L2`) to the real ID of each line of the transcript.
    pub refs: BTreeMap<String, String>,
    /// The names of the entities that the prompt lists. An answer can use them.
    pub known: BTreeSet<String>,
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
    #[error("unknown entity {0}. Add it to the new entities of the answer.")]
    UnknownEntity(String),
    #[error("entry {position} of {section} has no evidence")]
    MissingEvidence {
        section: &'static str,
        position: usize,
    },
    #[error("scene {0} does not start after the previous scene")]
    SceneOrder(usize),
    #[error("range {0} of lines ends before it starts")]
    LineRange(usize),
}

/// Reads the answer to the prompt. The kind of the prompt picks the parser.
pub fn parse_answer(prompt: &Prompt, text: &str) -> Result<Answer, AnswerError> {
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

/// Writes the entity list of a prompt, one entity on a line: the name, then the reading of a
/// character or the kind of another entity.
fn entity_list(entities: &[Entity]) -> Vec<String> {
    entities
        .iter()
        .map(|entity| {
            let detail = if entity.kind == CHARACTER {
                entity.reading.clone()
            } else {
                Some(entity.kind.clone())
            };
            match detail {
                Some(detail) => format!("{} ({detail})", entity.name),
                None => entity.name.clone(),
            }
        })
        .collect()
}

/// Gives the next line its short reference, `L1` for the first line, and remembers its real ID.
fn reference(line: &PromptLine, refs: &mut BTreeMap<String, String>) -> String {
    let reference = format!("L{}", refs.len() + 1);
    refs.insert(reference.clone(), line.id.0.clone());
    reference
}

/// The number of an `L` reference. Only a reference that `line_ref` accepted comes here.
fn line_number(reference: &str) -> u32 {
    reference[1..]
        .parse()
        .expect("a line reference has a number")
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
        .map(|id| LineId(id.clone()))
        .ok_or_else(|| AnswerError::UnknownRef(reference.to_string()))
}

/// An entity is named in the prompt, or the answer introduces it.
fn entity_name(
    prompt: &Prompt,
    declared: &BTreeSet<String>,
    name: &str,
) -> Result<String, AnswerError> {
    if declared.contains(name) || prompt.known.contains(name) {
        Ok(name.to_string())
    } else {
        Err(AnswerError::UnknownEntity(name.to_string()))
    }
}

fn speaker_text(speaker: &Speaker) -> String {
    match speaker {
        Speaker::Unknown => "[?]".to_string(),
        Speaker::Nobody => "[-]".to_string(),
        Speaker::Characters(ids, _) => {
            let names: Vec<&str> = ids.iter().map(|id| id.0.as_str()).collect();
            format!("[{}]", names.join(","))
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
