mod index;

pub use index::EpisodeProgress;

use index::Index;

use crate::files::{read_json, write_atomic};
use crate::model::Position;
use crate::package;
use crate::sources::{self, Problem, Scan, Source, SourceId};
use crate::suggestions::prompt::{self, AnswerError, Prompt, PromptId, PromptKind};
use crate::suggestions::suggestion::{
    self, Action, Content, Decision, Status, Suggestion, SuggestionId, SuggestionKind,
};
use anyhow::{Context, Result, bail, ensure};
use fs4::FileExt;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

/// A series directory. It holds only its path, so it can be cloned and sent to a thread.
/// Each call opens its own index.
#[derive(Debug, Clone)]
pub struct Series {
    dir: PathBuf,
}

/// The files and directories that every series directory has.
const SERIES_ENTRIES: [&str; 5] = [
    "sources.jsonl",
    "decisions.jsonl",
    "sources",
    "prompts",
    "answers",
];

/// Checks that a new series can use the directory: it is missing or empty.
fn check_new_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        ensure!(dir.is_dir(), "{} is not a directory", dir.display());
        let mut entries =
            fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?;
        ensure!(entries.next().is_none(), "{} is not empty", dir.display());
    }
    Ok(())
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

/// A prompt file and whether it has an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptRow {
    pub prompt: Prompt,
    pub answered: bool,
}

/// How many suggestions a call changed and how many it skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decided {
    pub applied: usize,
    pub skipped: usize,
}

/// How many sources a call removed and how many it kept because a prompt or an answer uses them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removed {
    pub removed: usize,
    pub blocked: usize,
}

impl Series {
    /// The directory for a new series called `name` inside `parent`. The directory must be
    /// missing or empty. This checks the name and the directory. It makes nothing.
    pub fn new_dir(parent: &Path, name: &str) -> Result<PathBuf> {
        let name = name.trim();
        ensure!(!name.is_empty(), "the name is empty");
        ensure!(
            !name.contains(|c: char| c.is_control() || r#"\/:*?"<>|"#.contains(c)),
            r#"the name must not have any of \ / : * ? " < > |"#
        );
        ensure!(!name.ends_with('.'), "the name must not end with a dot");
        let dir = parent.join(name);
        check_new_dir(&dir)?;
        Ok(dir)
    }

    /// Makes a new series. The directory must be missing or empty.
    pub fn create(dir: &Path) -> Result<Series> {
        check_new_dir(dir)?;
        fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
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

    /// The series directories inside `root`, sorted by name. A missing `root` has no series.
    /// A directory that is not a series is left out.
    pub fn list(root: &Path) -> Result<Vec<PathBuf>> {
        if !root.exists() {
            return Ok(Vec::new());
        }
        let entries =
            fs::read_dir(root).with_context(|| format!("cannot read {}", root.display()))?;
        let mut dirs = Vec::new();
        for entry in entries {
            let path = entry
                .with_context(|| format!("cannot read {}", root.display()))?
                .path();
            if SERIES_ENTRIES.iter().all(|name| path.join(name).exists()) {
                dirs.push(path);
            }
        }
        dirs.sort();
        Ok(dirs)
    }

    /// Opens an existing series. The index is built again when it is missing.
    pub fn open(dir: &Path) -> Result<Series> {
        for name in SERIES_ENTRIES {
            ensure!(
                dir.join(name).exists(),
                "{} is not a series folder: {name} is missing.",
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

    /// Reads files and folders and proposes a record for each supported file. Nothing is saved.
    /// `progress` gets the number of files done and the total.
    pub fn scan(&self, paths: &[PathBuf], progress: impl FnMut(usize, usize)) -> Result<Scan> {
        sources::scan(&self.dir, paths, progress)
    }

    /// Finds the problems of saving `pending` records next to the `saved` records.
    /// An error blocks the save. A warning does not.
    pub fn check_sources(saved: &[Source], pending: &[Source]) -> Vec<Problem> {
        sources::check(saved, pending)
    }

    pub fn add_sources(&self, records: &[Source]) -> Result<()> {
        let _lock = write_lock(&self.dir)?;
        sources::add(&self.dir, records)?;
        Index::rebuild(&self.dir)
    }

    pub fn sources(&self) -> Result<Vec<Source>> {
        sources::load(&self.dir)
    }

    /// The sources in `ids` that a prompt or an answer uses. `remove_sources` keeps them.
    pub fn blocked_removals(&self, ids: &[SourceId]) -> Result<Vec<SourceId>> {
        sources::find_used(&self.dir, ids)
    }

    /// Removes the sources in `ids` and the copies of their subtitles. A source that a prompt
    /// or an answer uses stays. The original files stay too.
    pub fn remove_sources(&self, ids: &[SourceId]) -> Result<Removed> {
        let _lock = write_lock(&self.dir)?;
        let blocked = sources::find_used(&self.dir, ids)?;
        let allowed: Vec<SourceId> = ids
            .iter()
            .filter(|id| !blocked.contains(id))
            .cloned()
            .collect();
        let removed = sources::remove(&self.dir, &allowed)?;
        if removed > 0 {
            Index::rebuild(&self.dir)?;
        }
        Ok(Removed {
            removed,
            blocked: blocked.len(),
        })
    }

    pub fn suggestions(&self, filter: &SuggestionFilter) -> Result<Vec<SuggestionRow>> {
        let index = Index::open(&self.dir)?;
        let mut all = index.suggestions()?;
        all.sort_by_key(|(suggestion, _)| suggestion.id.sort_key());

        let names: HashMap<String, String> = all
            .iter()
            .filter_map(|(suggestion, _)| match &suggestion.content {
                Content::Entity { name, .. } => Some((suggestion.id.0.clone(), name.clone())),
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
                content: suggestion.content.describe(&names),
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
        let _lock = write_lock(&self.dir)?;
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
                .filter_map(|other| other.content.unique_key())
                .collect();
            let key_taken = suggestions
                .iter()
                .find(|other| &other.id == id)
                .and_then(|other| other.content.unique_key())
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

    /// Makes the context package at one position, as YAML text. Nothing is saved.
    pub fn build_package(&self, at: Position) -> Result<String> {
        let input = {
            let index = Index::open(&self.dir)?;
            index.package_input(at)?
        };
        Ok(package::build(&input))
    }

    /// Lists the prompt files by ID, and says which ones have an answer.
    pub fn prompts(&self) -> Result<Vec<PromptRow>> {
        let directory = self.dir.join("prompts");
        let entries = fs::read_dir(&directory)
            .with_context(|| format!("cannot read {}", directory.display()))?;
        let mut rows = Vec::new();
        for entry in entries {
            let path = entry
                .with_context(|| format!("cannot read {}", directory.display()))?
                .path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                let prompt: Prompt = read_json(&path)?;
                let answered = self.answer_file(prompt.id).exists();
                rows.push(PromptRow { prompt, answered });
            }
        }
        rows.sort_by_key(|row| row.prompt.id);
        Ok(rows)
    }

    /// Reads one prompt file.
    pub fn prompt(&self, id: PromptId) -> Result<Prompt> {
        read_json(&self.prompt_file(id))
    }

    /// Writes a new prompt file for one or more episodes. It does not rebuild the index.
    pub fn create_prompt(&self, kind: PromptKind, episodes: &[u32]) -> Result<Prompt> {
        let mut episodes = episodes.to_vec();
        episodes.sort_unstable();
        episodes.dedup();
        ensure!(!episodes.is_empty(), "a prompt needs at least one episode");
        let _lock = write_lock(&self.dir)?;
        let index = Index::open(&self.dir)?;
        let id = self.next_prompt_id()?;
        let prompt = match kind {
            PromptKind::Speakers => {
                prompt::speakers::make(id, &episodes, &index.speakers_input(&episodes)?)
            }
            PromptKind::Story => prompt::story::make(id, &episodes, &index.story_input(&episodes)?),
        };
        let path = self.prompt_file(id);
        write_atomic(&path, &serde_json::to_string_pretty(&prompt)?)?;
        Ok(prompt)
    }

    /// Reads the answer, writes its JSON part, and rebuilds the index.
    pub fn submit_answer(&self, prompt_id: PromptId, text: &str) -> Result<usize> {
        let _lock = write_lock(&self.dir)?;
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

/// Takes the write lock of the series in `dir`. The lock lasts until the returned file is dropped.
/// The lock is a file in the series folder. The OS frees it when the process ends, even after a crash.
fn write_lock(dir: &Path) -> Result<File> {
    let path = dir.join("series.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .with_context(|| format!("cannot open {}", path.display()))?;
    FileExt::lock(&file).with_context(|| format!("cannot lock {}", path.display()))?;
    Ok(file)
}

fn evidence_of(index: &Index, content: &Content) -> Result<Vec<String>> {
    content
        .evidence_lines()
        .iter()
        .map(|line| index.evidence_text(line))
        .collect()
}
