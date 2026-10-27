use crate::files::{enum_from_text, enum_text, read_json};
use crate::sources::ass::{self, LineId};
use crate::sources::{self, Language, Source, SourceId, SourceKind};
use crate::speakers::prompt::{self, Prompt, PromptInput, PromptLine};
use crate::speakers::suggestion::{self, Content, Review, Status, Suggestion, SuggestionId};
use crate::speakers::{self, Character, CharacterId, Label, Speaker, SpeakerMethod};
use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags, Transaction, params};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// The version of the tables in `schema.sql`. A different version means a full rebuild.
pub const SCHEMA_VERSION: i32 = 1;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeProgress {
    pub episode: u32,
    /// The episode has a `Ja` subtitle.
    pub has_subtitle: bool,
    /// `None` when there is no `Ja` subtitle.
    pub unknown_lines: Option<u32>,
}

/// One line of an episode, from a `Ja` subtitle.
struct EpisodeLine {
    episode: u32,
    id: LineId,
    source_id: SourceId,
    line: ass::Line,
}

/// Everything the rebuild needs, read from the files of the series.
struct Data {
    records: Vec<Source>,
    lines: Vec<EpisodeLine>,
    suggestions: Vec<Suggestion>,
    reviews: HashMap<SuggestionId, Review>,
}

/// The SQLite index of one series. It holds no truth. The files hold it.
pub struct Index {
    connection: Connection,
}

pub fn index_path(series_dir: &Path) -> PathBuf {
    series_dir.join("index.sqlite")
}

fn is_current(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("cannot open {}", path.display()))?;
    let version: i32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    Ok(version == SCHEMA_VERSION)
}

/// Builds the index again from the files. The new file replaces the old one only when it is complete.
fn rebuild(series_dir: &Path) -> Result<()> {
    let data = load(series_dir)?;
    let temporary = series_dir.join("index.sqlite.tmp");
    if temporary.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("cannot remove {}", temporary.display()))?;
    }
    {
        let mut connection = Connection::open(&temporary)
            .with_context(|| format!("cannot create {}", temporary.display()))?;
        connection.execute_batch(SCHEMA)?;
        connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        let transaction = connection.transaction()?;
        write_all(&transaction, &data)?;
        transaction.commit()?;
    }
    let index = index_path(series_dir);
    fs::rename(&temporary, &index)
        .with_context(|| format!("cannot replace {}", index.display()))?;
    Ok(())
}

fn load(series_dir: &Path) -> Result<Data> {
    let records = sources::load(series_dir)?;
    let decisions = suggestion::read_decisions(series_dir)?;

    let mut suggestions = Vec::new();
    for prompt in read_prompts(series_dir)? {
        let answer_path = series_dir
            .join("answers")
            .join(format!("{}.json", prompt.id));
        if !answer_path.exists() {
            continue;
        }
        let text = fs::read_to_string(&answer_path)
            .with_context(|| format!("cannot read {}", answer_path.display()))?;
        let parsed = prompt::parse_answer(&prompt, &text)
            .with_context(|| format!("invalid answer {}", prompt.id))?;
        suggestions.extend(parsed);
    }
    let reviews = suggestion::reviews(&suggestions, &decisions);

    let mut lines = Vec::new();
    for record in &records {
        let (Some(episode), SourceKind::Subtitle, Some(Language::Ja)) =
            (record.episode, record.kind, record.language)
        else {
            continue;
        };
        let path = series_dir
            .join("sources")
            .join(format!("{}.ass", record.id));
        let text = sources::read_text(&path)?;
        let parsed =
            ass::parse(&text).with_context(|| format!("cannot parse {}", path.display()))?;
        for line in parsed {
            lines.push(EpisodeLine {
                episode,
                id: LineId(format!("{}:{}", record.id, line.index)),
                source_id: record.id.clone(),
                line,
            });
        }
    }

    Ok(Data {
        records,
        lines,
        suggestions,
        reviews,
    })
}

/// The prompt files, in ID order.
fn read_prompts(series_dir: &Path) -> Result<Vec<Prompt>> {
    let directory = series_dir.join("prompts");
    let mut prompts = Vec::new();
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
            prompts.push(read_json::<Prompt>(&path)?);
        }
    }
    prompts.sort_by_key(|prompt| prompt.id);
    Ok(prompts)
}

fn write_all(transaction: &Transaction, data: &Data) -> Result<()> {
    for record in &data.records {
        transaction.execute(
            "INSERT INTO sources VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id.0,
                enum_text(&record.kind),
                record.path.to_string_lossy().into_owned(),
                record.size as i64,
                record.episode,
                record.language.as_ref().map(enum_text),
            ],
        )?;
    }

    for episode_line in &data.lines {
        let line = &episode_line.line;
        transaction.execute(
            "INSERT INTO lines VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                episode_line.id.0,
                episode_line.source_id.0,
                episode_line.episode,
                line.start_ms,
                line.end_ms,
                line.text,
                line.color,
                line.offscreen,
                line.speaker_label,
            ],
        )?;
    }

    let approved: Vec<&Suggestion> = data
        .suggestions
        .iter()
        .filter(|suggestion| is_data(&data.reviews[&suggestion.id]))
        .collect();
    let line_index: HashMap<&LineId, (u32, u32)> = data
        .lines
        .iter()
        .map(|episode_line| {
            (
                &episode_line.id,
                (episode_line.episode, episode_line.line.end_ms),
            )
        })
        .collect();

    let mut labels: HashMap<String, Label> = HashMap::new();
    let mut colors: HashMap<u32, CharacterId> = HashMap::new();
    let mut line_speakers: HashMap<LineId, Vec<CharacterId>> = HashMap::new();

    for suggestion in &approved {
        if let Content::Character {
            name,
            reading,
            evidence,
        } = &suggestion.content
        {
            let mut since: Option<(u32, u32)> = None;
            for line in evidence {
                let found = line_index
                    .get(line)
                    .copied()
                    .ok_or_else(|| missing_line(&suggestion.id, line))?;
                since = Some(since.map_or(found, |current| current.max(found)));
            }
            let (since_episode, since_ms) =
                since.expect("a character has at least one evidence line");
            transaction.execute(
                "INSERT INTO characters VALUES (?1, ?2, ?3, ?4, ?5)",
                params![suggestion.id.0, name, reading, since_episode, since_ms],
            )?;
        }
    }

    for suggestion in &approved {
        match &suggestion.content {
            Content::Label {
                label,
                kind,
                character,
            } => {
                transaction.execute(
                    "INSERT INTO speaker_labels VALUES (?1, ?2, ?3)",
                    params![label, enum_text(kind), character],
                )?;
                labels.insert(
                    label.clone(),
                    Label {
                        kind: *kind,
                        character: character.clone().map(CharacterId),
                    },
                );
            }
            Content::Color { color, character } => {
                transaction.execute(
                    "INSERT INTO speaker_colors VALUES (?1, ?2)",
                    params![color, character],
                )?;
                colors.insert(*color, CharacterId(character.clone()));
            }
            Content::LineSpeaker { line, characters } => {
                if !line_index.contains_key(line) {
                    return Err(missing_line(&suggestion.id, line));
                }
                line_speakers.insert(
                    line.clone(),
                    characters.iter().cloned().map(CharacterId).collect(),
                );
            }
            Content::Character { .. } => {}
        }
    }

    let mut episodes: BTreeMap<u32, Vec<(LineId, &ass::Line)>> = BTreeMap::new();
    for episode_line in &data.lines {
        episodes
            .entry(episode_line.episode)
            .or_default()
            .push((episode_line.id.clone(), &episode_line.line));
    }
    for lines in episodes.values() {
        let speakers = speakers::resolve(lines, &labels, &colors, &line_speakers);
        for ((line_id, _), speaker) in lines.iter().zip(speakers) {
            insert_speaker(transaction, line_id, &speaker)?;
        }
    }

    for suggestion in &data.suggestions {
        let review = data.reviews[&suggestion.id];
        let prompt_id = suggestion.id.sort_key().0;
        transaction.execute(
            "INSERT INTO suggestions VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                suggestion.id.0,
                prompt_id,
                enum_text(&suggestion.content.kind()),
                enum_text(&review.status),
                review.blocked,
                serde_json::to_string(&suggestion.content)?,
            ],
        )?;
    }
    Ok(())
}

/// Approved and not blocked. Only these suggestions become data.
fn is_data(review: &Review) -> bool {
    review.status == Status::Approved && !review.blocked
}

fn missing_line(suggestion: &SuggestionId, line: &LineId) -> anyhow::Error {
    anyhow!("approved suggestion {suggestion} uses line {line}, which is not in lines")
}

fn insert_speaker(transaction: &Transaction, line: &LineId, speaker: &Speaker) -> Result<()> {
    match speaker {
        Speaker::Unknown => {}
        Speaker::Nobody => {
            transaction.execute(
                "INSERT INTO line_speakers VALUES (?1, NULL, NULL)",
                params![line.0],
            )?;
        }
        Speaker::Characters(characters, method) => {
            for character in characters {
                transaction.execute(
                    "INSERT INTO line_speakers VALUES (?1, ?2, ?3)",
                    params![line.0, character.0, enum_text(method)],
                )?;
            }
        }
    }
    Ok(())
}

impl Index {
    /// Opens the index. It is built again when it is missing or has another schema version.
    pub fn open(series_dir: &Path) -> Result<Index> {
        let path = index_path(series_dir);
        if !is_current(&path)? {
            Index::rebuild(series_dir)?;
        }
        let connection =
            Connection::open(&path).with_context(|| format!("cannot open {}", path.display()))?;
        Ok(Index { connection })
    }

    /// Builds the index again from the files. The new file replaces the old one only when it is complete.
    pub fn rebuild(series_dir: &Path) -> Result<()> {
        rebuild(series_dir)
    }

    /// Each episode with its progress for the speakers.
    pub fn progress(&self) -> Result<Vec<EpisodeProgress>> {
        let mut statement = self.connection.prepare(
            "SELECT DISTINCT episode FROM sources WHERE episode IS NOT NULL ORDER BY episode",
        )?;
        let episodes: Vec<u32> = statement
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;

        let mut progress = Vec::new();
        for episode in episodes {
            let has_subtitle: bool = self.connection.query_row(
                "SELECT COUNT(*) > 0 FROM sources WHERE episode = ?1 AND kind = 'subtitle' AND language = 'ja'",
                params![episode],
                |row| row.get(0),
            )?;
            let unknown_lines = if has_subtitle {
                let count: u32 = self.connection.query_row(
                    "SELECT COUNT(*) FROM lines l WHERE l.episode = ?1
                     AND NOT EXISTS (SELECT 1 FROM line_speakers s WHERE s.line_id = l.id)",
                    params![episode],
                    |row| row.get(0),
                )?;
                Some(count)
            } else {
                None
            };
            progress.push(EpisodeProgress {
                episode,
                has_subtitle,
                unknown_lines,
            });
        }
        Ok(progress)
    }

    /// Every suggestion with its review, as it was at the last rebuild.
    pub fn suggestions(&self) -> Result<Vec<(Suggestion, Review)>> {
        let mut statement = self
            .connection
            .prepare("SELECT id, status, blocked, content FROM suggestions")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut suggestions = Vec::new();
        for row in rows {
            let (id, status, blocked, content) = row?;
            suggestions.push((
                Suggestion {
                    id: SuggestionId(id),
                    content: serde_json::from_str(&content)?,
                },
                Review {
                    status: enum_from_text(&status)?,
                    blocked,
                },
            ));
        }
        Ok(suggestions)
    }

    /// One line of evidence, for example `E1 00:10 わたし 持つ！`.
    pub fn evidence_text(&self, line: &LineId) -> Result<String> {
        let row = self.connection.query_row(
            "SELECT episode, start_ms, text FROM lines WHERE id = ?1",
            params![line.0],
            |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        );
        match row {
            Ok((episode, start_ms, text)) => {
                let seconds = start_ms / 1000;
                Ok(format!(
                    "E{episode} {:02}:{:02} {text}",
                    seconds / 60,
                    seconds % 60
                ))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(format!("missing line {line}")),
            Err(error) => Err(error.into()),
        }
    }

    /// The input of a Speakers prompt. It fails when the episode has no `Ja` subtitle.
    pub fn prompt_input(&self, episode: u32) -> Result<PromptInput> {
        let has_subtitle: bool = self.connection.query_row(
            "SELECT COUNT(*) > 0 FROM sources WHERE episode = ?1 AND kind = 'subtitle' AND language = 'ja'",
            params![episode],
            |row| row.get(0),
        )?;
        if !has_subtitle {
            bail!("episode {episode} has no Ja subtitle");
        }

        let speakers = speakers_of_episode(&self.connection, episode)?;
        let mut statement = self.connection.prepare(
            "SELECT id, start_ms, color, speaker_label, text FROM lines WHERE episode = ?1 ORDER BY rowid",
        )?;
        let lines = statement
            .query_map(params![episode], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, Option<u32>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .map(|row| {
                let (id, start_ms, color, label, text) = row?;
                let speaker = speakers.get(&id).cloned().unwrap_or(Speaker::Unknown);
                Ok(PromptLine {
                    id: LineId(id),
                    start_ms,
                    color,
                    label,
                    text,
                    speaker,
                })
            })
            .collect::<Result<Vec<PromptLine>>>()?;

        let characters = characters(&self.connection)?;
        let labels = unapproved_labels(&self.connection, episode)?;
        let colors = unapproved_colors(&self.connection, episode)?;
        Ok(PromptInput {
            lines,
            characters,
            labels,
            colors,
        })
    }
}

/// The current speaker of each line of one episode. A line with no row is unknown.
fn speakers_of_episode(connection: &Connection, episode: u32) -> Result<HashMap<String, Speaker>> {
    let mut statement = connection.prepare(
        "SELECT s.line_id, s.character_id, s.method FROM line_speakers s
         JOIN lines l ON l.id = s.line_id WHERE l.episode = ?1 ORDER BY s.rowid",
    )?;
    let rows = statement.query_map(params![episode], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;

    let mut grouped: BTreeMap<String, (Vec<CharacterId>, Option<SpeakerMethod>, bool)> =
        BTreeMap::new();
    for row in rows {
        let (line, character, method) = row?;
        let entry = grouped.entry(line).or_insert((Vec::new(), None, false));
        match character {
            Some(character) => {
                entry.0.push(CharacterId(character));
                entry.1 = method.map(|text| enum_from_text(&text)).transpose()?;
            }
            None => entry.2 = true,
        }
    }

    let mut speakers = HashMap::new();
    for (line, (characters, method, nobody)) in grouped {
        let speaker = if nobody {
            Speaker::Nobody
        } else {
            Speaker::Characters(characters, method.expect("a speaker row has a method"))
        };
        speakers.insert(line, speaker);
    }
    Ok(speakers)
}

fn characters(connection: &Connection) -> Result<Vec<Character>> {
    let mut statement = connection
        .prepare("SELECT id, name, reading FROM characters ORDER BY since_episode, since_ms, id")?;
    let characters = statement
        .query_map([], |row| {
            Ok(Character {
                id: CharacterId(row.get(0)?),
                name: row.get(1)?,
                reading: row.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(characters)
}

/// The parts of the labels of one episode that have no approved label.
fn unapproved_labels(connection: &Connection, episode: u32) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT speaker_label FROM lines WHERE episode = ?1 AND speaker_label IS NOT NULL",
    )?;
    let labels: Vec<String> = statement
        .query_map(params![episode], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    let mut parts: BTreeSet<String> = BTreeSet::new();
    for label in labels {
        for part in label.split('・') {
            let approved: bool = connection.query_row(
                "SELECT COUNT(*) > 0 FROM speaker_labels WHERE label = ?1",
                params![part],
                |row| row.get(0),
            )?;
            if !approved {
                parts.insert(part.to_string());
            }
        }
    }
    Ok(parts.into_iter().collect())
}

fn unapproved_colors(connection: &Connection, episode: u32) -> Result<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT color FROM lines WHERE episode = ?1 AND color IS NOT NULL
         AND color NOT IN (SELECT color FROM speaker_colors) ORDER BY color",
    )?;
    let colors = statement
        .query_map(params![episode], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(colors)
}
