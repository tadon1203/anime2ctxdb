mod index;

pub use index::{DataView, EntityRow, EpisodeProgress, FactRow, SceneRow};

use index::Index;

use crate::answers::answer;
use crate::answers::prompt::{self, Prompt, PromptKind};
use crate::files::write_atomic;
use crate::importance::{self, Importance};
use crate::model::Position;
use crate::package;
use crate::sources::{self, Problem, Scan, Source, SourceId};
use anyhow::{Context, Result, ensure};
use fs4::FileExt;
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

/// A series directory. It holds only its path, so it can be cloned and sent to a thread.
/// Each call opens its own index.
#[derive(Debug, Clone)]
pub struct Series {
    dir: PathBuf,
}

/// The files and directories that every series directory has.
const SERIES_ENTRIES: [&str; 3] = ["sources.jsonl", "sources", "answers"];

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

/// How many sources a call removed and how many it kept because an answer uses them.
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
        for name in ["sources", "answers"] {
            let path = dir.join(name);
            fs::create_dir_all(&path)
                .with_context(|| format!("cannot create {}", path.display()))?;
        }
        let path = dir.join("sources.jsonl");
        File::create(&path).with_context(|| format!("cannot create {}", path.display()))?;
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

    /// The sources in `ids` that an answer uses. `remove_sources` keeps them.
    pub fn blocked_removals(&self, ids: &[SourceId]) -> Result<Vec<SourceId>> {
        sources::find_used(&self.dir, ids)
    }

    /// Removes the sources in `ids` and the copies of their subtitles. A source that an answer
    /// uses stays. The original files stay too.
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

    pub fn progress(&self) -> Result<Vec<EpisodeProgress>> {
        let index = Index::open(&self.dir)?;
        index.progress()
    }

    /// The entities, the facts, and the scenes that the answers gave.
    pub fn data(&self) -> Result<DataView> {
        let index = Index::open(&self.dir)?;
        index.data()
    }

    /// Sets the importance of an entity by hand. A hand setting wins over every answer. `None`
    /// removes the hand setting. An entity that no answer introduces cannot get a setting.
    pub fn set_importance(&self, name: &str, importance: Option<Importance>) -> Result<()> {
        let _lock = write_lock(&self.dir)?;
        let mut overrides = importance::load_overrides(&self.dir)?;
        match importance {
            Some(value) => {
                ensure!(
                    Index::open(&self.dir)?.has_entity(name)?,
                    "unknown entity {name}"
                );
                overrides.insert(name.to_string(), value);
            }
            None => {
                overrides.remove(name);
            }
        }
        importance::save_overrides(&self.dir, &overrides)?;
        Index::rebuild(&self.dir)
    }

    /// Makes the context package at one position, as YAML text. Nothing is saved.
    pub fn build_package(&self, at: Position) -> Result<String> {
        let input = {
            let index = Index::open(&self.dir)?;
            index.package_input(at)?
        };
        Ok(package::build(&input))
    }

    /// Makes the prompt for one episode. Nothing is saved. The prompt is made as if the episode had
    /// no answer of this kind, so the answer to it replaces the old answer whole.
    pub fn prompt(&self, kind: PromptKind, episode: u32) -> Result<Prompt> {
        let index = Index::without(&self.dir, kind, episode)?;
        make_prompt(&index, kind, episode)
    }

    /// The episodes that have an answer of this kind.
    pub fn answered_episodes(&self, kind: PromptKind) -> Result<BTreeSet<u32>> {
        Ok(answer::read_answers(&self.dir)?
            .into_iter()
            .filter(|answer| answer.kind() == kind)
            .map(|answer| answer.episode())
            .collect())
    }

    /// Reads the answer to the prompt for one episode, writes it to the answer file of the episode
    /// and the kind, and rebuilds the index. A new answer replaces the old one whole. Returns the
    /// number of entries in the answer.
    pub fn submit_answer(&self, kind: PromptKind, episode: u32, text: &str) -> Result<usize> {
        let _lock = write_lock(&self.dir)?;
        let prompt = self.prompt(kind, episode)?;
        let answer = prompt::parse_answer(&prompt, text)?;
        let path = answer::answer_path(&self.dir, kind, episode);
        write_atomic(&path, &serde_json::to_string_pretty(&answer)?)?;
        Index::rebuild(&self.dir)?;
        Ok(answer.count())
    }
}

fn make_prompt(index: &Index, kind: PromptKind, episode: u32) -> Result<Prompt> {
    Ok(match kind {
        PromptKind::Speakers => prompt::speakers::make(episode, &index.speakers_input(episode)?),
        PromptKind::Story => prompt::story::make(episode, &index.story_input(episode)?),
    })
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
