use crate::files::{append_jsonl, read_jsonl, write_atomic};
use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use std::fmt::{self, Write};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

pub(crate) mod ass;

/// The size of the first part and of the last part of a video that its ID covers.
const VIDEO_HASH_PART: u64 = 1024 * 1024;

/// SHA-256 of a source file, in lowercase hex. For a video, the hash covers the file size, the
/// first part, and the last part only.
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

/// Where the proposed episode of a detected file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeOrigin {
    /// The file is already in the series.
    Saved,
    FileName,
    FolderName,
    /// A file in the same folder with the same name, such as `a.mkv` and `a.ja.ass`.
    SiblingFile,
    Unknown,
}

/// A file that the scan read, with a proposed record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub source: Source,
    pub origin: EpisodeOrigin,
}

/// A file that the scan could not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub path: PathBuf,
    pub message: String,
}

/// The result of a scan. Nothing is saved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Scan {
    pub detected: Vec<Detected>,
    pub failed: Vec<Failure>,
    /// Files with an unsupported type, and files with the same content as an earlier file.
    pub skipped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The records cannot be saved.
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub severity: Severity,
    /// The index of the pending record. `None` for a problem of the whole series.
    pub row: Option<usize>,
    pub message: String,
}

/// Each pattern gives the episode number in its first group. The first match wins.
static EPISODE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    compile(&[
        r"第(\d+)話",
        r"[Ee][Pp]?(\d+)",
        r" - (\d+)",
        r"\[(\d+)\]",
        r"#(\d+)",
    ])
});

/// The patterns for a folder name. A folder name such as `Season 2` must not give an episode.
static FOLDER_PATTERNS: LazyLock<Vec<Regex>> =
    LazyLock::new(|| compile(&[r"第(\d+)話", r"[Ee][Pp]?(\d+)", r"#(\d+)"]));

fn compile(patterns: &[&str]) -> Vec<Regex> {
    patterns
        .iter()
        .map(|pattern| Regex::new(pattern).expect("valid regex"))
        .collect()
}

impl Source {
    /// Reads a file and makes its record. The episode comes from the file name.
    pub(crate) fn from_file(path: &Path) -> Result<Source> {
        let kind = kind_of(path)?;
        let size = fs::metadata(path)
            .with_context(|| format!("cannot read {}", path.display()))?
            .len();
        let id = hash_file(path, kind)?;
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

/// Reads the files and the folders in `paths`, and proposes a record for each supported file.
/// A folder is read with all its subfolders. A file that is already in the series keeps its
/// saved episode and language. `progress` gets the number of files done and the total.
pub(crate) fn scan(
    series_dir: &Path,
    paths: &[PathBuf],
    mut progress: impl FnMut(usize, usize),
) -> Result<Scan> {
    let saved = load(series_dir)?;
    let mut scan = Scan::default();
    let mut files = Vec::new();
    for path in paths {
        collect_files(path, &mut files, &mut scan);
    }

    let total = files.len();
    let mut ids: HashSet<SourceId> = HashSet::new();
    for (done, path) in files.iter().enumerate() {
        progress(done, total);
        match Source::from_file(path) {
            Ok(source) if ids.insert(source.id.clone()) => {
                let origin = match source.episode {
                    Some(_) => EpisodeOrigin::FileName,
                    None => EpisodeOrigin::Unknown,
                };
                scan.detected.push(Detected { source, origin });
            }
            Ok(_) => scan.skipped += 1,
            Err(error) => scan.failed.push(Failure {
                path: path.clone(),
                message: format!("{error:#}"),
            }),
        }
    }
    progress(total, total);

    for item in &mut scan.detected {
        if let Some(current) = saved.iter().find(|current| current.id == item.source.id) {
            item.source.episode = current.episode;
            item.source.language = current.language;
            item.origin = EpisodeOrigin::Saved;
        }
    }
    fill_from_folder(&mut scan.detected);
    fill_from_sibling(&mut scan.detected);
    Ok(scan)
}

/// Adds the supported files below `path` to `files`. A folder gives its files in name order.
fn collect_files(path: &Path, files: &mut Vec<PathBuf>, scan: &mut Scan) {
    if !path.is_dir() {
        if kind_of_extension(path).is_some() {
            files.push(path.to_path_buf());
        } else {
            scan.skipped += 1;
        }
        return;
    }
    let entries = fs::read_dir(path).and_then(|entries| {
        entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::io::Result<Vec<PathBuf>>>()
    });
    match entries {
        Ok(mut children) => {
            children.sort();
            for child in &children {
                collect_files(child, files, scan);
            }
        }
        Err(error) => scan.failed.push(Failure {
            path: path.to_path_buf(),
            message: format!("cannot read {}: {error}", path.display()),
        }),
    }
}

/// Gives an episode from the name of the parent folder to each file that has none.
fn fill_from_folder(items: &mut [Detected]) {
    for item in items
        .iter_mut()
        .filter(|item| item.origin == EpisodeOrigin::Unknown)
    {
        let folder = item
            .source
            .path
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(episode) = first_number(&FOLDER_PATTERNS, &folder) {
            item.source.episode = Some(episode);
            item.origin = EpisodeOrigin::FolderName;
        }
    }
}

/// Gives an episode from a sibling file to each file that has none.
fn fill_from_sibling(items: &mut [Detected]) {
    let known: Vec<(PathBuf, u32)> = items
        .iter()
        .filter_map(|item| Some((item.source.path.clone(), item.source.episode?)))
        .collect();
    for item in items
        .iter_mut()
        .filter(|item| item.origin == EpisodeOrigin::Unknown)
    {
        let sibling = known
            .iter()
            .find(|(path, _)| is_sibling(&item.source.path, path));
        if let Some(&(_, episode)) = sibling {
            item.source.episode = Some(episode);
            item.origin = EpisodeOrigin::SiblingFile;
        }
    }
}

/// True for two files in one folder whose names differ only after the stem, such as
/// `a.mkv` and `a.ja.ass`.
fn is_sibling(first: &Path, second: &Path) -> bool {
    if first == second || first.parent() != second.parent() {
        return false;
    }
    let (first, second) = (stem_of(first), stem_of(second));
    first == second
        || first.starts_with(&format!("{second}."))
        || second.starts_with(&format!("{first}."))
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Finds the problems of saving `pending` records next to the `saved` records.
pub(crate) fn check(saved: &[Source], pending: &[Source]) -> Vec<Problem> {
    let mut merged = saved.to_vec();
    for record in pending {
        match merged.iter_mut().find(|old| old.id == record.id) {
            Some(old) => *old = record.clone(),
            None => merged.push(record.clone()),
        }
    }

    let mut problems = Vec::new();
    for (row, record) in pending.iter().enumerate() {
        let mut add = |severity, message: String| {
            problems.push(Problem {
                severity,
                row: Some(row),
                message,
            });
        };
        if record.kind == SourceKind::Video && record.language.is_some() {
            add(Severity::Error, "a video has no language".to_string());
        }
        let Some(episode) = record.episode else {
            add(
                Severity::Warning,
                "no episode, so the app ignores this file".to_string(),
            );
            continue;
        };
        if episode == 0 {
            add(Severity::Error, "an episode is 1 or more".to_string());
            continue;
        }
        let is_ja_subtitle =
            record.kind == SourceKind::Subtitle && record.language == Some(Language::Ja);
        let ja_count = merged
            .iter()
            .filter(|other| {
                other.episode == Some(episode)
                    && other.kind == SourceKind::Subtitle
                    && other.language == Some(Language::Ja)
            })
            .count();
        if is_ja_subtitle && ja_count > 1 {
            add(
                Severity::Error,
                format!("episode {episode} has more than one Ja subtitle"),
            );
        } else if saved.iter().any(|old| {
            old.id != record.id
                && old.episode == record.episode
                && old.kind == record.kind
                && old.language == record.language
        }) {
            add(
                Severity::Warning,
                format!("episode {episode} already has a saved source of this kind"),
            );
        }
    }

    let with_subtitle: BTreeSet<u32> = merged
        .iter()
        .filter(|record| record.kind == SourceKind::Subtitle)
        .filter_map(|record| record.episode)
        .collect();
    let last = merged.iter().filter_map(|record| record.episode).max();
    let missing: Vec<String> = (1..=last.unwrap_or(0))
        .filter(|number| !with_subtitle.contains(number))
        .map(|number| number.to_string())
        .collect();
    if !missing.is_empty() {
        problems.push(Problem {
            severity: Severity::Warning,
            row: None,
            message: format!("no subtitle for episode {}", missing.join(", ")),
        });
    }
    problems
}

/// Checks the records, copies each new subtitle into the series, and appends the records.
pub(crate) fn add(series_dir: &Path, records: &[Source]) -> Result<()> {
    let saved = load(series_dir)?;
    if let Some(problem) = check(&saved, records)
        .into_iter()
        .find(|problem| problem.severity == Severity::Error)
    {
        match problem.row {
            Some(row) => bail!("{}: {}", problem.message, records[row].path.display()),
            None => bail!("{}", problem.message),
        }
    }

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

/// The sources in `ids` that a prompt or an answer uses. A prompt or an answer has the ID of
/// each source whose lines it uses, so a search for the ID in their text finds them.
pub(crate) fn find_used(series_dir: &Path, ids: &[SourceId]) -> Result<Vec<SourceId>> {
    let mut texts = Vec::new();
    for name in ["prompts", "answers"] {
        let directory = series_dir.join(name);
        let entries = fs::read_dir(&directory)
            .with_context(|| format!("cannot read {}", directory.display()))?;
        for entry in entries {
            let path = entry
                .with_context(|| format!("cannot read {}", directory.display()))?
                .path();
            texts.push(
                fs::read_to_string(&path)
                    .with_context(|| format!("cannot read {}", path.display()))?,
            );
        }
    }
    Ok(ids
        .iter()
        .filter(|id| texts.iter().any(|text| text.contains(&id.0)))
        .cloned()
        .collect())
}

/// Removes the records in `ids` and the copies of their subtitles. Returns how many records it
/// removed. The original files stay.
pub(crate) fn remove(series_dir: &Path, ids: &[SourceId]) -> Result<usize> {
    let saved = load(series_dir)?;
    let (removed, kept): (Vec<Source>, Vec<Source>) = saved
        .into_iter()
        .partition(|record| ids.contains(&record.id));
    let mut text = String::new();
    for record in &kept {
        text.push_str(&serde_json::to_string(record)?);
        text.push('\n');
    }
    write_atomic(&series_dir.join("sources.jsonl"), &text)?;

    // A copy that stays by mistake does no harm, because no record points to it.
    for record in removed
        .iter()
        .filter(|record| record.kind == SourceKind::Subtitle)
    {
        let copy = series_dir
            .join("sources")
            .join(format!("{}.ass", record.id));
        if copy.exists() {
            fs::remove_file(&copy).with_context(|| format!("cannot remove {}", copy.display()))?;
        }
    }
    Ok(removed.len())
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
    match kind_of_extension(path) {
        Some(kind) => Ok(kind),
        None => bail!("unsupported file type: {}", path.display()),
    }
}

fn kind_of_extension(path: &Path) -> Option<SourceKind> {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "ass" => Some(SourceKind::Subtitle),
        "mkv" | "mp4" | "webm" | "avi" => Some(SourceKind::Video),
        _ => None,
    }
}

/// The ID of a file. A subtitle is small, so its ID covers the whole file. A video is large,
/// so its ID covers the size, the first part, and the last part.
fn hash_file(path: &Path, kind: SourceKind) -> Result<SourceId> {
    let mut file = File::open(path).with_context(|| format!("cannot open {}", path.display()))?;
    let mut hasher = Sha256::new();
    match kind {
        SourceKind::Subtitle => {
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
        }
        SourceKind::Video => {
            let size = file
                .metadata()
                .with_context(|| format!("cannot read {}", path.display()))?
                .len();
            hasher.update(size.to_le_bytes());
            let mut part = Vec::new();
            let head_end = size.min(VIDEO_HASH_PART);
            (&mut file)
                .take(head_end)
                .read_to_end(&mut part)
                .with_context(|| format!("cannot read {}", path.display()))?;
            hasher.update(&part);
            // The tail starts after the head, so no byte counts twice.
            let tail_start = head_end.max(size - size.min(VIDEO_HASH_PART));
            part.clear();
            file.seek(SeekFrom::Start(tail_start))
                .with_context(|| format!("cannot read {}", path.display()))?;
            file.read_to_end(&mut part)
                .with_context(|| format!("cannot read {}", path.display()))?;
            hasher.update(&part);
        }
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in &digest {
        write!(hex, "{byte:02x}").expect("writing to a String does not fail");
    }
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

/// The episode from a file name. A name that is only a number, such as `01.ass`, is an episode.
fn episode_from_name(name: &str) -> Option<u32> {
    first_number(&EPISODE_PATTERNS, name).or_else(|| {
        let stem = Path::new(name).file_stem()?.to_string_lossy();
        if stem.chars().all(|c| c.is_ascii_digit()) {
            stem.parse().ok()
        } else {
            None
        }
    })
}

/// The number in the first group of the first pattern that matches.
fn first_number(patterns: &[Regex], text: &str) -> Option<u32> {
    patterns.iter().find_map(|pattern| {
        let captures = pattern.captures(text)?;
        captures[1].parse().ok()
    })
}
