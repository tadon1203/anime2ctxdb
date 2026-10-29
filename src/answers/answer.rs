use crate::answers::prompt::PromptKind;
use crate::files::{enum_text, read_json};
use crate::importance::Importance;
use crate::sources::ass::LineId;
use crate::speakers::LabelKind;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// One or more lines. A stored entity or fact with no evidence cannot be read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<LineId>", into = "Vec<LineId>")]
pub struct Evidence(Vec<LineId>);

impl Evidence {
    /// `None` for an empty list.
    pub fn new(lines: Vec<LineId>) -> Option<Evidence> {
        (!lines.is_empty()).then_some(Evidence(lines))
    }

    pub fn lines(&self) -> &[LineId] {
        &self.0
    }
}

impl TryFrom<Vec<LineId>> for Evidence {
    type Error = String;

    fn try_from(lines: Vec<LineId>) -> Result<Evidence, String> {
        Evidence::new(lines).ok_or_else(|| "evidence has no line".to_string())
    }
}

impl From<Evidence> for Vec<LineId> {
    fn from(evidence: Evidence) -> Vec<LineId> {
        evidence.0
    }
}

/// An entity that an answer introduces. An entity is its name: two answers with the same name
/// give one entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewEntity {
    pub kind: String,
    pub name: String,
    pub reading: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<Importance>,
    pub evidence: Evidence,
}

/// A new importance for an entity that earlier answers introduced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportanceChange {
    pub name: String,
    pub importance: Importance,
}

/// What a speaker label means. `character` is the name of an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelEntry {
    pub label: String,
    pub kind: LabelKind,
    pub character: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorEntry {
    pub color: u32,
    pub character: String,
}

/// The speakers of one line. An empty list means that nobody speaks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineEntry {
    pub line: LineId,
    pub characters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fact {
    pub entities: Vec<String>,
    pub text: String,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scene {
    /// The scene runs to the start of the next scene, or to the end of the episode.
    pub start: LineId,
    pub place: Option<String>,
    pub summary: String,
}

/// An answer file, `answers/<kind>-<episode>.json`. It holds what the answer says, checked and
/// with real line IDs, so the app does not need the prompt again. An episode has one answer of
/// each kind. A new answer replaces the old one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Answer {
    Speakers {
        episode: u32,
        characters: Vec<NewEntity>,
        labels: Vec<LabelEntry>,
        colors: Vec<ColorEntry>,
        lines: Vec<LineEntry>,
    },
    Story {
        episode: u32,
        entities: Vec<NewEntity>,
        facts: Vec<Fact>,
        scenes: Vec<Scene>,
        #[serde(default)]
        importance: Vec<ImportanceChange>,
    },
}

impl Answer {
    pub fn kind(&self) -> PromptKind {
        match self {
            Answer::Speakers { .. } => PromptKind::Speakers,
            Answer::Story { .. } => PromptKind::Story,
        }
    }

    pub fn episode(&self) -> u32 {
        match self {
            Answer::Speakers { episode, .. } | Answer::Story { episode, .. } => *episode,
        }
    }

    /// The number of entries in the answer.
    pub fn count(&self) -> usize {
        match self {
            Answer::Speakers {
                characters,
                labels,
                colors,
                lines,
                ..
            } => characters.len() + labels.len() + colors.len() + lines.len(),
            Answer::Story {
                entities,
                facts,
                scenes,
                importance,
                ..
            } => entities.len() + facts.len() + scenes.len() + importance.len(),
        }
    }
}

/// The file of the answer of one kind to one episode.
pub fn answer_path(series_dir: &Path, kind: PromptKind, episode: u32) -> PathBuf {
    series_dir
        .join("answers")
        .join(format!("{}-{episode}.json", enum_text(&kind)))
}

/// Reads the answer files. The order is by episode, and then speakers before story.
pub fn read_answers(series_dir: &Path) -> Result<Vec<Answer>> {
    let directory = series_dir.join("answers");
    let mut answers = Vec::new();
    for entry in
        fs::read_dir(&directory).with_context(|| format!("cannot read {}", directory.display()))?
    {
        let path = entry
            .with_context(|| format!("cannot read {}", directory.display()))?
            .path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            answers.push(read_json::<Answer>(&path)?);
        }
    }
    answers.sort_by_key(|answer| (answer.episode(), answer.kind()));
    Ok(answers)
}
