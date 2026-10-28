use crate::files::{enum_from_text, enum_text};
use crate::model::{CHARACTER, Entity, EntityId, Position, format_clock};
use crate::package::{PackageFact, PackageInput, PackageLine, PackageScene};
use crate::sources::ass::{self, LineId};
use crate::sources::{self, Language, Source, SourceId, SourceKind};
use crate::speakers::{self, Label, Speaker, SpeakerMethod};
use crate::suggestions::prompt::{self, PromptFact, PromptLine, SpeakersInput, StoryInput};
use crate::suggestions::suggestion::{self, Content, Review, Status, Suggestion, SuggestionId};
use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags, Row, Transaction, params};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

/// The version of the tables in `schema.sql`. A different version means a full rebuild.
pub const SCHEMA_VERSION: i32 = 4;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeProgress {
    pub episode: u32,
    /// The episode has a `Ja` subtitle.
    pub has_subtitle: bool,
    /// `None` when there is no `Ja` subtitle.
    pub unknown_lines: Option<u32>,
    /// The approved facts that start in this episode.
    pub facts: u32,
}

/// Where a line is in its episode. A line is in the story at `end_ms`.
#[derive(Debug, Clone, Copy)]
struct LineTime {
    episode: u32,
    start_ms: u32,
    end_ms: u32,
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

/// The labels, colors, and line speakers that the approved suggestions set.
struct ApprovedContent {
    labels: HashMap<String, Label>,
    colors: HashMap<u32, EntityId>,
    line_speakers: HashMap<LineId, Vec<EntityId>>,
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
    for (_, answer) in prompt::read_answers(series_dir)? {
        suggestions.extend(answer.suggestions);
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

fn write_all(transaction: &Transaction<'_>, data: &Data) -> Result<()> {
    insert_sources(transaction, &data.records)?;
    insert_lines(transaction, &data.lines)?;

    let approved: Vec<&Suggestion> = data
        .suggestions
        .iter()
        .filter(|suggestion| is_data(data.reviews[&suggestion.id]))
        .collect();
    let line_index: HashMap<&LineId, LineTime> = data
        .lines
        .iter()
        .map(|episode_line| {
            (
                &episode_line.id,
                LineTime {
                    episode: episode_line.episode,
                    start_ms: episode_line.line.start_ms,
                    end_ms: episode_line.line.end_ms,
                },
            )
        })
        .collect();

    insert_entities(transaction, &approved, &line_index)?;
    insert_facts(transaction, &approved, &line_index)?;
    insert_scenes(transaction, &approved, &line_index)?;
    let content = insert_approved_content(transaction, &approved, &line_index)?;
    insert_episode_speakers(transaction, &data.lines, &content)?;
    insert_suggestions(transaction, data)
}

fn insert_sources(transaction: &Transaction<'_>, records: &[Source]) -> Result<()> {
    for record in records {
        transaction.execute(
            "INSERT INTO sources VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.id.0,
                enum_text(&record.kind),
                record.path.to_string_lossy().into_owned(),
                record.size.cast_signed(),
                record.episode,
                record.language.as_ref().map(enum_text),
            ],
        )?;
    }
    Ok(())
}

fn insert_lines(transaction: &Transaction<'_>, lines: &[EpisodeLine]) -> Result<()> {
    for episode_line in lines {
        let line = &episode_line.line;
        transaction.execute(
            "INSERT INTO lines VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
                line.prompt_text(),
            ],
        )?;
    }
    Ok(())
}

fn insert_entities(
    transaction: &Transaction<'_>,
    approved: &[&Suggestion],
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<()> {
    for suggestion in approved {
        if let Content::Entity {
            kind,
            name,
            reading,
            ..
        } = &suggestion.content
        {
            let since = since_of(suggestion, line_index)?;
            transaction.execute(
                "INSERT INTO entities VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    suggestion.id.0,
                    kind,
                    name,
                    reading,
                    since.episode,
                    since.time_ms
                ],
            )?;
        }
    }
    Ok(())
}

fn insert_facts(
    transaction: &Transaction<'_>,
    approved: &[&Suggestion],
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<()> {
    for suggestion in approved {
        if let Content::Fact {
            entities,
            text,
            evidence,
        } = &suggestion.content
        {
            let since = since_of(suggestion, line_index)?;
            transaction.execute(
                "INSERT INTO facts VALUES (?1, ?2, ?3, ?4)",
                params![suggestion.id.0, text, since.episode, since.time_ms],
            )?;
            for entity in entities {
                transaction.execute(
                    "INSERT INTO fact_entities VALUES (?1, ?2)",
                    params![suggestion.id.0, entity],
                )?;
            }
            for line in evidence.lines() {
                transaction.execute(
                    "INSERT INTO fact_evidence VALUES (?1, ?2)",
                    params![suggestion.id.0, line.0],
                )?;
            }
        }
    }
    Ok(())
}

fn insert_scenes(
    transaction: &Transaction<'_>,
    approved: &[&Suggestion],
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<()> {
    for suggestion in approved {
        if let Content::Scene {
            start,
            end,
            place,
            summary,
        } = &suggestion.content
        {
            let start_line = line_time(suggestion, start, line_index)?;
            let end_line = line_time(suggestion, end, line_index)?;
            transaction.execute(
                "INSERT INTO scenes VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    suggestion.id.0,
                    start_line.episode,
                    start_line.start_ms,
                    end_line.end_ms,
                    place,
                    summary
                ],
            )?;
        }
    }
    Ok(())
}

/// The position where the evidence of a suggestion is known: the latest end of its lines.
fn since_of(suggestion: &Suggestion, line_index: &HashMap<&LineId, LineTime>) -> Result<Position> {
    let mut since: Option<Position> = None;
    for line in suggestion.content.evidence_lines() {
        let found = line_time(suggestion, &line, line_index)?;
        let position = Position {
            episode: found.episode,
            time_ms: found.end_ms,
        };
        since = Some(since.map_or(position, |current| current.max(position)));
    }
    Ok(since.expect("an entity or a fact has at least one evidence line"))
}

fn line_time(
    suggestion: &Suggestion,
    line: &LineId,
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<LineTime> {
    line_index
        .get(line)
        .copied()
        .ok_or_else(|| missing_line(&suggestion.id, line))
}

fn insert_approved_content(
    transaction: &Transaction<'_>,
    approved: &[&Suggestion],
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<ApprovedContent> {
    let mut content = ApprovedContent {
        labels: HashMap::new(),
        colors: HashMap::new(),
        line_speakers: HashMap::new(),
    };
    for suggestion in approved {
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
                content.labels.insert(
                    label.clone(),
                    Label {
                        kind: *kind,
                        character: character.clone().map(EntityId),
                    },
                );
            }
            Content::Color { color, character } => {
                transaction.execute(
                    "INSERT INTO speaker_colors VALUES (?1, ?2)",
                    params![color, character],
                )?;
                content.colors.insert(*color, EntityId(character.clone()));
            }
            Content::LineSpeaker { line, characters } => {
                if !line_index.contains_key(line) {
                    return Err(missing_line(&suggestion.id, line));
                }
                content.line_speakers.insert(
                    line.clone(),
                    characters.iter().cloned().map(EntityId).collect(),
                );
            }
            Content::Entity { .. } | Content::Fact { .. } | Content::Scene { .. } => {}
        }
    }
    Ok(content)
}

fn insert_episode_speakers(
    transaction: &Transaction<'_>,
    lines: &[EpisodeLine],
    content: &ApprovedContent,
) -> Result<()> {
    let mut episodes: BTreeMap<u32, Vec<(LineId, &ass::Line)>> = BTreeMap::new();
    for episode_line in lines {
        episodes
            .entry(episode_line.episode)
            .or_default()
            .push((episode_line.id.clone(), &episode_line.line));
    }
    for episode_lines in episodes.values() {
        let speakers = speakers::resolve(
            episode_lines,
            &content.labels,
            &content.colors,
            &content.line_speakers,
        );
        for ((line_id, _), speaker) in episode_lines.iter().zip(speakers) {
            insert_speaker(transaction, line_id, &speaker)?;
        }
    }
    Ok(())
}

fn insert_suggestions(transaction: &Transaction<'_>, data: &Data) -> Result<()> {
    for suggestion in &data.suggestions {
        let review = data.reviews[&suggestion.id];
        let answer_id = suggestion.id.sort_key().0;
        transaction.execute(
            "INSERT INTO suggestions VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                suggestion.id.0,
                answer_id,
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
fn is_data(review: Review) -> bool {
    review.status == Status::Approved && !review.blocked
}

fn missing_line(suggestion: &SuggestionId, line: &LineId) -> anyhow::Error {
    anyhow!("approved suggestion {suggestion} uses line {line}, which is not in lines")
}

fn insert_speaker(transaction: &Transaction<'_>, line: &LineId, speaker: &Speaker) -> Result<()> {
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
            let facts: u32 = self.connection.query_row(
                "SELECT COUNT(*) FROM facts WHERE since_episode = ?1",
                params![episode],
                |row| row.get(0),
            )?;
            progress.push(EpisodeProgress {
                episode,
                has_subtitle,
                unknown_lines,
                facts,
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
                Ok(format!("E{episode} {} {text}", format_clock(start_ms)))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(format!("missing line {line}")),
            Err(error) => Err(error.into()),
        }
    }

    /// The input of a Speakers prompt. It fails when an episode has no `Ja` subtitle.
    pub fn speakers_input(&self, episodes: &[u32]) -> Result<SpeakersInput> {
        let mut labels = BTreeSet::new();
        let mut colors = BTreeSet::new();
        for &episode in episodes {
            require_subtitle(&self.connection, episode)?;
            labels.extend(unapproved_labels(&self.connection, episode)?);
            colors.extend(unapproved_colors(&self.connection, episode)?);
        }
        Ok(SpeakersInput {
            lines: self.prompt_lines(episodes)?,
            entities: entities(&self.connection, Some(CHARACTER))?,
            labels: labels.into_iter().collect(),
            colors: colors.into_iter().collect(),
        })
    }

    /// The input of a Story prompt. It fails when an episode has no `Ja` subtitle. The known
    /// facts are the facts before the first episode.
    pub fn story_input(&self, episodes: &[u32]) -> Result<StoryInput> {
        for &episode in episodes {
            require_subtitle(&self.connection, episode)?;
        }
        let first = episodes.iter().min().copied().unwrap_or_default();
        Ok(StoryInput {
            lines: self.prompt_lines(episodes)?,
            entities: entities(&self.connection, None)?,
            facts: known_facts(&self.connection, first)?,
        })
    }

    /// The input of a context package at one position. It fails when the episode has no `Ja` subtitle.
    pub fn package_input(&self, at: Position) -> Result<PackageInput> {
        require_subtitle(&self.connection, at.episode)?;
        Ok(PackageInput {
            at,
            entities: known_entities(&self.connection, at)?,
            facts: package_facts(&self.connection, at)?,
            scenes: package_scenes(&self.connection, at)?,
            lines: package_lines(&self.connection, at)?,
        })
    }

    /// The lines of the episodes in order, with their current speakers.
    fn prompt_lines(&self, episodes: &[u32]) -> Result<Vec<PromptLine>> {
        let mut lines = Vec::new();
        for &episode in episodes {
            lines.extend(self.episode_lines(episode)?);
        }
        Ok(lines)
    }

    /// The lines of one episode in time order, with their current speakers.
    fn episode_lines(&self, episode: u32) -> Result<Vec<PromptLine>> {
        let speakers = speakers_of_episode(&self.connection, episode)?;
        let mut statement = self.connection.prepare(
            "SELECT id, start_ms, color, speaker_label, prompt_text FROM lines WHERE episode = ?1 ORDER BY rowid",
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
                    episode,
                    start_ms,
                    color,
                    label,
                    text,
                    speaker,
                })
            })
            .collect::<Result<Vec<PromptLine>>>()?;
        Ok(lines)
    }
}

/// Fails when the episode has no `Ja` subtitle. A prompt needs one.
fn require_subtitle(connection: &Connection, episode: u32) -> Result<()> {
    let has_subtitle: bool = connection.query_row(
        "SELECT COUNT(*) > 0 FROM sources WHERE episode = ?1 AND kind = 'subtitle' AND language = 'ja'",
        params![episode],
        |row| row.get(0),
    )?;
    if !has_subtitle {
        bail!("episode {episode} has no Ja subtitle");
    }
    Ok(())
}

/// The current speaker of each line of one episode. A line with no row is unknown.
fn speakers_of_episode(connection: &Connection, episode: u32) -> Result<HashMap<String, Speaker>> {
    let mut statement = connection.prepare(
        "SELECT s.line_id, s.entity_id, s.method FROM line_speakers s
         JOIN lines l ON l.id = s.line_id WHERE l.episode = ?1 ORDER BY s.rowid",
    )?;
    let rows = statement.query_map(params![episode], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
        ))
    })?;

    let mut grouped: BTreeMap<String, (Vec<EntityId>, Option<SpeakerMethod>, bool)> =
        BTreeMap::new();
    for row in rows {
        let (line, character, method) = row?;
        let entry = grouped.entry(line).or_insert((Vec::new(), None, false));
        match character {
            Some(character) => {
                entry.0.push(EntityId(character));
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

/// The approved entities in the order they were added. `kind` `None` gives all kinds.
fn entities(connection: &Connection, kind: Option<&str>) -> Result<Vec<Entity>> {
    let mut statement = connection.prepare(
        "SELECT id, kind, name, reading FROM entities
         WHERE ?1 IS NULL OR kind = ?1 ORDER BY since_episode, since_ms, id",
    )?;
    let entities = statement
        .query_map(params![kind], entity_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(entities)
}

fn entity_from_row(row: &Row<'_>) -> rusqlite::Result<Entity> {
    Ok(Entity {
        id: EntityId(row.get(0)?),
        kind: row.get(1)?,
        name: row.get(2)?,
        reading: row.get(3)?,
    })
}

/// The entities that are known at the position. A later entity is left out.
fn known_entities(connection: &Connection, at: Position) -> Result<Vec<Entity>> {
    let mut statement = connection.prepare(
        "SELECT id, kind, name, reading FROM entities
         WHERE (since_episode, since_ms) <= (?1, ?2) ORDER BY since_episode, since_ms, id",
    )?;
    let entities = statement
        .query_map(params![at.episode, at.time_ms], entity_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(entities)
}

/// The facts that are known at the position, with their lines.
fn package_facts(connection: &Connection, at: Position) -> Result<Vec<PackageFact>> {
    let mut statement = connection.prepare(
        "SELECT id, since_episode, since_ms, text FROM facts
         WHERE (since_episode, since_ms) <= (?1, ?2) ORDER BY since_episode, since_ms, id",
    )?;
    let rows = statement
        .query_map(params![at.episode, at.time_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Position {
                    episode: row.get(1)?,
                    time_ms: row.get(2)?,
                },
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut entities_statement = connection
        .prepare("SELECT entity_id FROM fact_entities WHERE fact_id = ?1 ORDER BY rowid")?;
    let mut evidence_statement = connection.prepare(
        "SELECT f.line_id FROM fact_evidence f JOIN lines l ON l.id = f.line_id
         WHERE f.fact_id = ?1 ORDER BY l.episode, l.start_ms",
    )?;
    let mut facts = Vec::new();
    for (id, since, text) in rows {
        let entities = entities_statement
            .query_map(params![id], |row| Ok(EntityId(row.get(0)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let evidence = evidence_statement
            .query_map(params![id], |row| row.get::<_, String>(0))?
            .map(|line| package_line(connection, &line?))
            .collect::<Result<Vec<_>>>()?;
        facts.push(PackageFact {
            since,
            entities,
            text,
            evidence,
        });
    }
    Ok(facts)
}

/// The scenes that start at or before the position. A scene in progress is included.
fn package_scenes(connection: &Connection, at: Position) -> Result<Vec<PackageScene>> {
    let mut statement = connection.prepare(
        "SELECT episode, start_ms, end_ms, place_id, summary FROM scenes
         WHERE (episode, start_ms) <= (?1, ?2) ORDER BY episode, start_ms, id",
    )?;
    let scenes = statement
        .query_map(params![at.episode, at.time_ms], |row| {
            Ok(PackageScene {
                episode: row.get(0)?,
                start_ms: row.get(1)?,
                end_ms: row.get(2)?,
                place: row.get::<_, Option<String>>(3)?.map(EntityId),
                summary: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(scenes)
}

/// The lines of the episode that end at or before the position, in time order.
fn package_lines(connection: &Connection, at: Position) -> Result<Vec<PackageLine>> {
    let mut statement = connection
        .prepare("SELECT id FROM lines WHERE episode = ?1 AND end_ms <= ?2 ORDER BY rowid")?;
    let ids: Vec<String> = statement
        .query_map(params![at.episode, at.time_ms], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    ids.iter().map(|id| package_line(connection, id)).collect()
}

/// One line with its speakers. A copied speaker is not a speaker of the package.
fn package_line(connection: &Connection, id: &str) -> Result<PackageLine> {
    let (episode, start_ms, text) = connection.query_row(
        "SELECT episode, start_ms, text FROM lines WHERE id = ?1",
        params![id],
        |row| {
            Ok((
                row.get::<_, u32>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )?;
    let mut statement = connection.prepare(
        "SELECT entity_id FROM line_speakers
         WHERE line_id = ?1 AND entity_id IS NOT NULL AND method IN ('label', 'color', 'ai')
         ORDER BY rowid",
    )?;
    let speakers = statement
        .query_map(params![id], |row| Ok(EntityId(row.get(0)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(PackageLine {
        episode,
        start_ms,
        text,
        speakers,
    })
}

/// The approved facts that start before the episode, in time order.
fn known_facts(connection: &Connection, episode: u32) -> Result<Vec<PromptFact>> {
    let mut statement = connection.prepare(
        "SELECT id, since_episode, since_ms, text FROM facts
         WHERE since_episode < ?1 ORDER BY since_episode, since_ms, id",
    )?;
    let rows = statement
        .query_map(params![episode], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut names = connection.prepare(
        "SELECT e.name FROM fact_entities f JOIN entities e ON e.id = f.entity_id
         WHERE f.fact_id = ?1 ORDER BY f.rowid",
    )?;
    let mut facts = Vec::new();
    for (id, since_episode, since_ms, text) in rows {
        let entities = names
            .query_map(params![id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        facts.push(PromptFact {
            since: Position {
                episode: since_episode,
                time_ms: since_ms,
            },
            entities,
            text,
        });
    }
    Ok(facts)
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
