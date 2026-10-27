mod index;

pub use index::EpisodeProgress;

use index::Index;

use crate::files::{enum_text, read_json, write_atomic};
use crate::sources::{self, Source};
use crate::speakers::prompt::{self, AnswerError, Prompt, PromptId, PromptKind};
use crate::speakers::suggestion::{
    self, Action, Content, Decision, Status, Suggestion, SuggestionId, SuggestionKind,
};
use anyhow::{Context, Result, bail, ensure};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::path::{Path, PathBuf};

/// A series directory. It holds only its path, so it can be cloned and sent to a thread.
/// Each call opens its own index.
#[derive(Clone)]
pub struct Series {
    dir: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuggestionFilter {
    pub status: Option<Status>,
    pub kind: Option<SuggestionKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestionRow {
    pub id: SuggestionId,
    pub kind: SuggestionKind,
    /// For example `label "あんな" -> 明智あんな`.
    pub content: String,
    /// For example `E1 00:10 わたし 持つ！`.
    pub evidence: Vec<String>,
    pub status: Status,
    pub blocked: bool,
}

/// How many suggestions a call changed and how many it skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decided {
    pub applied: usize,
    pub skipped: usize,
}

impl Series {
    /// Makes a new series. The directory must be missing or empty.
    pub fn create(dir: &Path) -> Result<Series> {
        if dir.exists() {
            ensure!(dir.is_dir(), "{} is not a directory", dir.display());
            let mut entries =
                fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
            ensure!(entries.next().is_none(), "{} is not empty", dir.display());
        } else {
            fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
        }
        for name in ["sources", "prompts", "answers"] {
            let path = dir.join(name);
            fs::create_dir_all(&path)
                .with_context(|| format!("cannot create {}", path.display()))?;
        }
        for name in ["sources.jsonl", "decisions.jsonl"] {
            let path = dir.join(name);
            File::create(&path).with_context(|| format!("cannot create {}", path.display()))?;
        }
        Index::rebuild(dir)?;
        Ok(Series {
            dir: dir.to_path_buf(),
        })
    }

    /// Opens an existing series. The index is built again when it is missing.
    pub fn open(dir: &Path) -> Result<Series> {
        for name in [
            "sources.jsonl",
            "decisions.jsonl",
            "sources",
            "prompts",
            "answers",
        ] {
            ensure!(
                dir.join(name).exists(),
                "{} is not a series: {name} is missing",
                dir.display()
            );
        }
        Index::open(dir)?;
        Ok(Series {
            dir: dir.to_path_buf(),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Reads the files and proposes their kind, episode, and language. Nothing is saved.
    /// A file that is already in the series keeps its saved episode and language.
    pub fn read_files(&self, paths: &[PathBuf]) -> Result<Vec<Source>> {
        let saved = sources::load(&self.dir)?;
        paths
            .iter()
            .map(|path| {
                let mut record = Source::from_file(path)?;
                if let Some(current) = saved.iter().find(|current| current.id == record.id) {
                    record.episode = current.episode;
                    record.language = current.language;
                }
                Ok(record)
            })
            .collect()
    }

    pub fn add_sources(&self, records: &[Source]) -> Result<()> {
        sources::add(&self.dir, records)?;
        Index::rebuild(&self.dir)
    }

    pub fn sources(&self) -> Result<Vec<Source>> {
        sources::load(&self.dir)
    }

    pub fn suggestions(&self, filter: &SuggestionFilter) -> Result<Vec<SuggestionRow>> {
        let index = Index::open(&self.dir)?;
        let mut all = index.suggestions()?;
        all.sort_by_key(|(suggestion, _)| suggestion.id.sort_key());

        let names: HashMap<String, String> = all
            .iter()
            .filter_map(|(suggestion, _)| match &suggestion.content {
                Content::Character { name, .. } => Some((suggestion.id.0.clone(), name.clone())),
                _ => None,
            })
            .collect();

        let mut rows = Vec::new();
        for (suggestion, review) in all {
            let kind = suggestion.content.kind();
            if filter.status.is_some_and(|status| status != review.status) {
                continue;
            }
            if filter.kind.is_some_and(|wanted| wanted != kind) {
                continue;
            }
            let evidence = evidence_of(&index, &suggestion.content)?;
            rows.push(SuggestionRow {
                content: content_of(&suggestion.content, &names),
                id: suggestion.id,
                kind,
                evidence,
                status: review.status,
                blocked: review.blocked,
            });
        }
        Ok(rows)
    }

    /// Applies the decisions in the given order. A decision on a suggestion that cannot take it is skipped.
    pub fn decide(&self, ids: &[SuggestionId], action: Action) -> Result<Decided> {
        let all = {
            let index = Index::open(&self.dir)?;
            index.suggestions()?
        };
        let suggestions: Vec<Suggestion> = all
            .iter()
            .map(|(suggestion, _)| suggestion.clone())
            .collect();
        let mut working = suggestion::read_decisions(&self.dir)?;
        let mut appended: Vec<Decision> = Vec::new();
        let mut done = 0;
        let mut skipped = 0;

        for id in ids {
            let reviews = suggestion::reviews(&suggestions, &working);
            let Some(review) = reviews.get(id) else {
                bail!("unknown suggestion {id}");
            };
            let taken: HashSet<String> = suggestions
                .iter()
                .filter(|other| {
                    reviews
                        .get(&other.id)
                        .is_some_and(|s| s.status == Status::Approved && !s.blocked)
                })
                .filter_map(|other| data_key(&other.content))
                .collect();
            let key_taken = suggestions
                .iter()
                .find(|other| &other.id == id)
                .and_then(|other| data_key(&other.content))
                .is_some_and(|key| taken.contains(&key));
            let allowed = match action {
                Action::Approve => {
                    review.status == Status::Pending && !review.blocked && !key_taken
                }
                Action::Reject => review.status == Status::Pending,
                Action::Revoke => matches!(review.status, Status::Approved | Status::Rejected),
            };
            if allowed {
                let decision = Decision {
                    suggestion: id.clone(),
                    action,
                };
                working.push(decision.clone());
                appended.push(decision);
                done += 1;
            } else {
                skipped += 1;
            }
        }

        if !appended.is_empty() {
            suggestion::append_decisions(&self.dir, &appended)?;
            Index::rebuild(&self.dir)?;
        }
        Ok(Decided {
            applied: done,
            skipped,
        })
    }

    pub fn progress(&self) -> Result<Vec<EpisodeProgress>> {
        let index = Index::open(&self.dir)?;
        index.progress()
    }

    /// Writes a new prompt file. It does not rebuild the index.
    pub fn create_prompt(&self, kind: PromptKind, episode: u32) -> Result<Prompt> {
        let input = {
            let index = Index::open(&self.dir)?;
            index.prompt_input(episode)?
        };
        let id = self.next_prompt_id()?;
        let prompt = prompt::make(id, kind, episode, &input);
        let path = self.prompt_file(id);
        write_atomic(&path, &serde_json::to_string_pretty(&prompt)?)?;
        Ok(prompt)
    }

    /// Reads the answer, writes its JSON part, and rebuilds the index.
    pub fn submit_answer(&self, prompt_id: PromptId, text: &str) -> Result<usize> {
        let answer_path = self.answer_file(prompt_id);
        ensure!(
            !answer_path.exists(),
            "prompt {prompt_id} already has an answer"
        );
        let prompt_path = self.prompt_file(prompt_id);
        let prompt: Prompt = read_json(&prompt_path)?;
        let json = prompt::json_part(text).ok_or(AnswerError::NoJson)?;
        let suggestions = prompt::parse_answer(&prompt, text)?;
        write_atomic(&answer_path, json)?;
        Index::rebuild(&self.dir)?;
        Ok(suggestions.len())
    }

    fn prompt_file(&self, id: PromptId) -> PathBuf {
        self.dir.join("prompts").join(format!("{id}.json"))
    }

    fn answer_file(&self, id: PromptId) -> PathBuf {
        self.dir.join("answers").join(format!("{id}.json"))
    }

    /// The largest prompt ID in `prompts/` plus 1.
    fn next_prompt_id(&self) -> Result<PromptId> {
        let directory = self.dir.join("prompts");
        let mut largest = 0;
        for entry in fs::read_dir(&directory)
            .with_context(|| format!("cannot read {}", directory.display()))?
        {
            let path = entry
                .with_context(|| format!("cannot read {}", directory.display()))?
                .path();
            if let Some(number) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| stem.parse::<u32>().ok())
            {
                largest = largest.max(number);
            }
        }
        Ok(PromptId(largest + 1))
    }
}

fn evidence_of(index: &Index, content: &Content) -> Result<Vec<String>> {
    match content {
        Content::Character { evidence, .. } => evidence
            .iter()
            .map(|line| index.evidence_text(line))
            .collect(),
        Content::LineSpeaker { line, .. } => Ok(vec![index.evidence_text(line)?]),
        Content::Label { .. } | Content::Color { .. } => Ok(Vec::new()),
    }
}

fn content_of(content: &Content, names: &HashMap<String, String>) -> String {
    let name_of = |id: &String| names.get(id).cloned().unwrap_or_else(|| id.clone());
    match content {
        Content::Character {
            name,
            reading: Some(reading),
            ..
        } => format!("character \"{name}\" ({reading})"),
        Content::Character { name, .. } => format!("character \"{name}\""),
        Content::Label {
            label,
            kind,
            character,
        } => {
            let target = match character {
                Some(id) => name_of(id),
                None => enum_text(kind),
            };
            format!("label \"{label}\" -> {target}")
        }
        Content::Color { color, character } => {
            format!("color #{color:06X} -> {}", name_of(character))
        }
        Content::LineSpeaker { characters, .. } if characters.is_empty() => "nobody".to_string(),
        Content::LineSpeaker { characters, .. } => {
            let names: Vec<String> = characters.iter().map(name_of).collect();
            format!("speaker {}", names.join(", "))
        }
    }
}

/// The text that only one approved suggestion can hold: a label or a color.
fn data_key(content: &Content) -> Option<String> {
    match content {
        Content::Label { label, .. } => Some(format!("label {label}")),
        Content::Color { color, .. } => Some(format!("color {color}")),
        Content::Character { .. } | Content::LineSpeaker { .. } => None,
    }
}
