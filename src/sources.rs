use crate::files::{append_jsonl, read_jsonl};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub(crate) mod ass;

/// SHA-256 of a source file, in lowercase hex.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourceId(pub String);

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Subtitle,
    Video,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Ja,
    Zh,
    En,
    Other,
}

/// One imported file. The app never changes the original file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub kind: SourceKind,
    /// The original path.
    pub path: PathBuf,
    pub size: u64,
    /// `None`: the app ignores this source.
    pub episode: Option<u32>,
    /// `None` for a video.
    pub language: Option<Language>,
}

/// Each pattern gives the episode number in its first group. The first match wins.
static EPISODE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"第(\d+)話",
        r"[Ee][Pp]?(\d+)",
        r" - (\d+)",
        r"\[(\d+)\]",
        r"#(\d+)",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid regex"))
    .collect()
});

impl Source {
    /// Reads a file and makes its record. The episode comes from the file name.
    pub(crate) fn from_file(path: &Path) -> Result<Source> {
        let kind = kind_of(path)?;
        let size = fs::metadata(path)
            .with_context(|| format!("cannot read {}", path.display()))?
            .len();
        let id = hash_file(path)?;
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let language = match kind {
            SourceKind::Subtitle => Some(language_of(path)?),
            SourceKind::Video => None,
        };
        Ok(Source {
            id,
            kind,
            path: path.to_path_buf(),
            size,
            episode: episode_from_name(&name),
            language,
        })
    }
}

/// Reads the current record of each source. The last record for an ID wins.
pub(crate) fn load(series_dir: &Path) -> Result<Vec<Source>> {
    let mut records: Vec<Source> = Vec::new();
    for record in read_jsonl::<Source>(&series_dir.join("sources.jsonl"))? {
        match records.iter_mut().find(|old| old.id == record.id) {
            Some(old) => *old = record,
            None => records.push(record),
        }
    }
    Ok(records)
}

/// Checks the records, copies each new subtitle into the series, and appends the records.
pub(crate) fn add(series_dir: &Path, records: &[Source]) -> Result<()> {
    let mut merged = load(series_dir)?;
    for record in records {
        match merged.iter_mut().find(|old| old.id == record.id) {
            Some(old) => *old = record.clone(),
            None => merged.push(record.clone()),
        }
    }
    validate(&merged, records)?;

    let sources_dir = series_dir.join("sources");
    for record in records
        .iter()
        .filter(|record| record.kind == SourceKind::Subtitle)
    {
        let target = sources_dir.join(format!("{}.ass", record.id));
        if !target.exists() {
            fs::copy(&record.path, &target)
                .with_context(|| format!("cannot copy {}", record.path.display()))?;
        }
    }
    append_jsonl(&series_dir.join("sources.jsonl"), records)
}

/// Reads a subtitle file as UTF-8 and removes the BOM.
pub(crate) fn read_text(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let text =
        String::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
    Ok(match text.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => text,
    })
}

fn kind_of(path: &Path) -> Result<SourceKind> {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "ass" => Ok(SourceKind::Subtitle),
        "mkv" | "mp4" | "webm" | "avi" => Ok(SourceKind::Video),
        _ => bail!("unsupported file type: {}", path.display()),
    }
}

fn hash_file(path: &Path) -> Result<SourceId> {
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut block = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut block)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&block[..read]);
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(SourceId(hex))
}

fn language_of(path: &Path) -> Result<Language> {
    let text = read_text(path)?;
    let lines = ass::parse(&text).with_context(|| format!("cannot parse {}", path.display()))?;
    let spoken: String = lines.iter().map(|line| line.text.as_str()).collect();
    Ok(language_of_text(&spoken))
}

/// Any kana gives Ja. Else any CJK ideograph gives Zh. Else any ASCII letter gives En.
fn language_of_text(text: &str) -> Language {
    if text.chars().any(|c| ('\u{3040}'..='\u{30FF}').contains(&c)) {
        Language::Ja
    } else if text.chars().any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)) {
        Language::Zh
    } else if text.chars().any(|c| c.is_ascii_alphabetic()) {
        Language::En
    } else {
        Language::Other
    }
}

fn episode_from_name(name: &str) -> Option<u32> {
    EPISODE_PATTERNS.iter().find_map(|pattern| {
        let captures = pattern.captures(name)?;
        captures[1].parse().ok()
    })
}

/// The rules that a set of sources must follow together.
fn validate(merged: &[Source], saved: &[Source]) -> Result<()> {
    for record in saved {
        if record.episode == Some(0) {
            bail!("an episode is 1 or more: {}", record.path.display());
        }
        if record.kind == SourceKind::Video && record.language.is_some() {
            bail!("a video has no language: {}", record.path.display());
        }
    }

    let mut episodes: BTreeSet<u32> = BTreeSet::new();
    let mut ja_subtitles: HashMap<u32, usize> = HashMap::new();
    for record in merged {
        let Some(episode) = record.episode else {
            continue;
        };
        episodes.insert(episode);
        if record.kind == SourceKind::Subtitle && record.language == Some(Language::Ja) {
            let count = ja_subtitles.entry(episode).or_default();
            *count += 1;
            if *count > 1 {
                bail!("episode {episode} has more than one Ja subtitle");
            }
        }
    }

    if let Some(&last) = episodes.last()
        && let Some(missing) = (1..=last).find(|number| !episodes.contains(number))
    {
        bail!("episode {missing} is missing");
    }
    Ok(())
}
