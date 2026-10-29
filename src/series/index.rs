use crate::answers::answer::{self, Answer};
use crate::answers::prompt::{PromptFact, PromptKind, PromptLine, SpeakersInput, StoryInput};
use crate::files::{enum_from_text, enum_text};
use crate::importance::{self, Declaration, Importance, Importances};
use crate::model::{CHARACTER, Entity, EntityId, Position, format_clock};
use crate::package::{PackageFact, PackageInput, PackageLine, PackageSample, PackageScene};
use crate::sources::ass::{self, LineId};
use crate::sources::{self, Language, Source, SourceId, SourceKind};
use crate::speakers::{self, Label, Speaker, SpeakerMethod};
use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OpenFlags, Transaction, params};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// The version of the tables in `schema.sql`. A different version means a full rebuild.
pub const SCHEMA_VERSION: i32 = 6;

/// How many sample lines of each character the package gets.
const SAMPLE_LINES: u32 = 5;

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpisodeProgress {
    pub episode: u32,
    /// The episode has a `Ja` subtitle.
    pub has_subtitle: bool,
    /// `None` when there is no `Ja` subtitle.
    pub unknown_lines: Option<u32>,
    /// The facts that start in this episode.
    pub facts: u32,
}

/// An entity, as the Data view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityRow {
    pub name: String,
    pub kind: String,
    pub reading: Option<String>,
    pub since: Position,
    /// The importance after the last episode.
    pub importance: Importance,
    /// True: the user set the importance by hand.
    pub manual: bool,
}

/// A fact, as the Data view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactRow {
    pub since: Position,
    pub entities: Vec<String>,
    pub text: String,
    /// For example `E1 00:10 わたし 持つ！`.
    pub evidence: Vec<String>,
}

/// A scene, as the Data view shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneRow {
    pub episode: u32,
    pub start_ms: u32,
    pub end_ms: u32,
    pub place: Option<String>,
    pub summary: String,
}

/// What the answers gave, for the Data view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataView {
    pub entities: Vec<EntityRow>,
    pub facts: Vec<FactRow>,
    pub scenes: Vec<SceneRow>,
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
    /// In order: by episode, and then speakers before story.
    answers: Vec<Answer>,
    /// The importance that the user set by hand.
    overrides: BTreeMap<String, Importance>,
}

/// The labels, colors, and line speakers that the answers set. A later episode wins.
struct SpeakerData {
    labels: HashMap<String, Label>,
    colors: HashMap<u32, EntityId>,
    line_speakers: HashMap<LineId, Vec<EntityId>>,
}

/// An entity from the answers. Answers with one name give one entity.
struct MergedEntity {
    kind: String,
    reading: Option<String>,
    since: Position,
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
    let data = load(series_dir, None)?;
    let temporary = series_dir.join("index.sqlite.tmp");
    if temporary.exists() {
        fs::remove_file(&temporary)
            .with_context(|| format!("cannot remove {}", temporary.display()))?;
    }
    {
        let mut connection = Connection::open(&temporary)
            .with_context(|| format!("cannot create {}", temporary.display()))?;
        fill(&mut connection, &data)?;
    }
    let index = index_path(series_dir);
    fs::rename(&temporary, &index)
        .with_context(|| format!("cannot replace {}", index.display()))?;
    Ok(())
}

/// Makes the tables and fills them.
fn fill(connection: &mut Connection, data: &Data) -> Result<()> {
    connection.execute_batch(SCHEMA)?;
    connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    let transaction = connection.transaction()?;
    write_all(&transaction, data)?;
    transaction.commit()?;
    Ok(())
}

/// Reads the files of the series. `skip` leaves out the answer of one kind to one episode.
fn load(series_dir: &Path, skip: Option<(PromptKind, u32)>) -> Result<Data> {
    let records = sources::load(series_dir)?;
    let answers = answer::read_answers(series_dir)?
        .into_iter()
        .filter(|answer| Some((answer.kind(), answer.episode())) != skip)
        .collect();

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
        answers,
        overrides: importance::load_overrides(series_dir)?,
    })
}

fn write_all(transaction: &Transaction<'_>, data: &Data) -> Result<()> {
    insert_sources(transaction, &data.records)?;
    insert_lines(transaction, &data.lines)?;
    for (name, value) in &data.overrides {
        transaction.execute(
            "INSERT INTO importance_overrides VALUES (?1, ?2)",
            params![name, enum_text(value)],
        )?;
    }

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

    insert_entities(transaction, &data.answers, &line_index)?;
    insert_story(transaction, &data.answers, &line_index)?;
    let speaker_data = insert_speaker_data(transaction, &data.answers, &line_index)?;
    insert_episode_speakers(transaction, &data.lines, &speaker_data)
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

/// Writes the entities that the answers introduce. An entity is its name. When answers give one
/// name more than once, the later answer sets the kind and the reading, and the entity is known
/// from the earliest evidence.
fn insert_entities(
    transaction: &Transaction<'_>,
    answers: &[Answer],
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<()> {
    let mut merged: BTreeMap<&str, MergedEntity> = BTreeMap::new();
    for answer in answers {
        let introduced = match answer {
            Answer::Speakers { characters, .. } => characters,
            Answer::Story { entities, .. } => entities,
        };
        for entity in introduced {
            if let Some(value) = entity.importance {
                insert_declaration(transaction, &entity.name, answer.episode(), value)?;
            }
            let since = since_of(entity.evidence.lines(), line_index, answer)?;
            match merged.get_mut(entity.name.as_str()) {
                Some(old) => {
                    old.kind.clone_from(&entity.kind);
                    old.reading = entity.reading.clone().or(old.reading.take());
                    old.since = old.since.min(since);
                }
                None => {
                    merged.insert(
                        &entity.name,
                        MergedEntity {
                            kind: entity.kind.clone(),
                            reading: entity.reading.clone(),
                            since,
                        },
                    );
                }
            }
        }
        if let Answer::Story { importance, .. } = answer {
            for change in importance {
                insert_declaration(
                    transaction,
                    &change.name,
                    answer.episode(),
                    change.importance,
                )?;
            }
        }
    }
    for (name, entity) in merged {
        transaction.execute(
            "INSERT INTO entities VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                name,
                entity.kind,
                entity.reading,
                entity.since.episode,
                entity.since.time_ms
            ],
        )?;
    }
    Ok(())
}

fn insert_declaration(
    transaction: &Transaction<'_>,
    name: &str,
    episode: u32,
    importance: Importance,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO importance_declarations VALUES (?1, ?2, ?3)",
        params![name, episode, enum_text(&importance)],
    )?;
    Ok(())
}

/// Writes the facts and the scenes of the story answers.
fn insert_story(
    transaction: &Transaction<'_>,
    answers: &[Answer],
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<()> {
    let mut episode_ends: HashMap<u32, u32> = HashMap::new();
    for time in line_index.values() {
        let end = episode_ends.entry(time.episode).or_insert(0);
        *end = (*end).max(time.end_ms);
    }
    for answer in answers {
        let Answer::Story { facts, scenes, .. } = answer else {
            continue;
        };
        let name = answer_name(answer);
        for (number, fact) in facts.iter().enumerate() {
            let id = format!("{name}-fact-{}", number + 1);
            let since = since_of(fact.evidence.lines(), line_index, answer)?;
            transaction.execute(
                "INSERT INTO facts VALUES (?1, ?2, ?3, ?4)",
                params![id, fact.text, since.episode, since.time_ms],
            )?;
            for entity in &fact.entities {
                transaction.execute(
                    "INSERT INTO fact_entities VALUES (?1, ?2)",
                    params![id, entity],
                )?;
            }
            for line in fact.evidence.lines() {
                transaction.execute(
                    "INSERT INTO fact_evidence VALUES (?1, ?2)",
                    params![id, line.0],
                )?;
            }
        }
        // A scene runs to the start of the next scene, and the last one to the end of the episode.
        let starts = scenes
            .iter()
            .map(|scene| line_time(answer, &scene.start, line_index))
            .collect::<Result<Vec<LineTime>>>()?;
        for (number, scene) in scenes.iter().enumerate() {
            let start = starts[number];
            let end_ms = match starts.get(number + 1) {
                Some(next) => next.start_ms,
                None => episode_ends[&start.episode],
            };
            transaction.execute(
                "INSERT INTO scenes VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    format!("{name}-scene-{}", number + 1),
                    start.episode,
                    start.start_ms,
                    end_ms.max(start.start_ms),
                    scene.place,
                    scene.summary
                ],
            )?;
        }
    }
    Ok(())
}

/// The name of an answer in messages and in IDs, for example `story-3`.
fn answer_name(answer: &Answer) -> String {
    format!("{}-{}", enum_text(&answer.kind()), answer.episode())
}

/// The position where the evidence is known: the latest end of its lines.
fn since_of(
    lines: &[LineId],
    line_index: &HashMap<&LineId, LineTime>,
    answer: &Answer,
) -> Result<Position> {
    let mut since: Option<Position> = None;
    for line in lines {
        let found = line_time(answer, line, line_index)?;
        let position = Position {
            episode: found.episode,
            time_ms: found.end_ms,
        };
        since = Some(since.map_or(position, |current| current.max(position)));
    }
    Ok(since.expect("an entity or a fact has at least one evidence line"))
}

fn line_time(
    answer: &Answer,
    line: &LineId,
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<LineTime> {
    line_index
        .get(line)
        .copied()
        .ok_or_else(|| missing_line(answer, line))
}

/// Writes the labels, the colors, and the line speakers of the speakers answers. A later episode
/// wins over an earlier one.
fn insert_speaker_data(
    transaction: &Transaction<'_>,
    answers: &[Answer],
    line_index: &HashMap<&LineId, LineTime>,
) -> Result<SpeakerData> {
    let mut data = SpeakerData {
        labels: HashMap::new(),
        colors: HashMap::new(),
        line_speakers: HashMap::new(),
    };
    for answer in answers {
        let Answer::Speakers {
            labels,
            colors,
            lines,
            ..
        } = answer
        else {
            continue;
        };
        for entry in labels {
            data.labels.insert(
                entry.label.clone(),
                Label {
                    kind: entry.kind,
                    character: entry.character.clone().map(EntityId),
                },
            );
        }
        for entry in colors {
            data.colors
                .insert(entry.color, EntityId(entry.character.clone()));
        }
        for entry in lines {
            if !line_index.contains_key(&entry.line) {
                return Err(missing_line(answer, &entry.line));
            }
            data.line_speakers.insert(
                entry.line.clone(),
                entry.characters.iter().cloned().map(EntityId).collect(),
            );
        }
    }
    for (label, entry) in &data.labels {
        transaction.execute(
            "INSERT INTO speaker_labels VALUES (?1, ?2, ?3)",
            params![
                label,
                enum_text(&entry.kind),
                entry.character.as_ref().map(|id| &id.0)
            ],
        )?;
    }
    for (color, character) in &data.colors {
        transaction.execute(
            "INSERT INTO speaker_colors VALUES (?1, ?2)",
            params![color, character.0],
        )?;
    }
    Ok(data)
}

fn insert_episode_speakers(
    transaction: &Transaction<'_>,
    lines: &[EpisodeLine],
    data: &SpeakerData,
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
            &data.labels,
            &data.colors,
            &data.line_speakers,
        );
        for ((line_id, _), speaker) in episode_lines.iter().zip(speakers) {
            insert_speaker(transaction, line_id, &speaker)?;
        }
    }
    Ok(())
}

fn missing_line(answer: &Answer, line: &LineId) -> anyhow::Error {
    anyhow!(
        "answer {} uses line {line}, which is not in lines",
        answer_name(answer)
    )
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

    /// An index in memory, as it would be without the answer of one kind to one episode. A prompt
    /// for that episode and kind comes from it, so a new answer replaces the old one whole.
    pub fn without(series_dir: &Path, kind: PromptKind, episode: u32) -> Result<Index> {
        let data = load(series_dir, Some((kind, episode)))?;
        let mut connection = Connection::open_in_memory()?;
        fill(&mut connection, &data)?;
        Ok(Index { connection })
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

    /// The entities, the facts, and the scenes that the answers gave, in story order.
    pub fn data(&self) -> Result<DataView> {
        let importances = load_importances(&self.connection)?;
        let entities = self
            .connection
            .prepare(
                "SELECT name, kind, reading, since_episode, since_ms FROM entities
                 ORDER BY since_episode, since_ms, name",
            )?
            .query_map([], |row| {
                let name: String = row.get(0)?;
                Ok(EntityRow {
                    importance: importances.of(&name, u32::MAX),
                    manual: importances.manual(&name).is_some(),
                    name,
                    kind: row.get(1)?,
                    reading: row.get(2)?,
                    since: Position {
                        episode: row.get(3)?,
                        time_ms: row.get(4)?,
                    },
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let fact_rows = self
            .connection
            .prepare(
                "SELECT id, since_episode, since_ms, text FROM facts
                 ORDER BY since_episode, since_ms, id",
            )?
            .query_map([], |row| {
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
        let mut entities_statement = self
            .connection
            .prepare("SELECT entity_id FROM fact_entities WHERE fact_id = ?1 ORDER BY rowid")?;
        let mut evidence_statement = self.connection.prepare(
            "SELECT f.line_id FROM fact_evidence f JOIN lines l ON l.id = f.line_id
             WHERE f.fact_id = ?1 ORDER BY l.episode, l.start_ms",
        )?;
        let mut facts = Vec::new();
        for (id, since, text) in fact_rows {
            let names = entities_statement
                .query_map(params![id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let evidence = evidence_statement
                .query_map(params![id], |row| row.get::<_, String>(0))?
                .map(|line| self.evidence_text(&LineId(line?)))
                .collect::<Result<Vec<_>>>()?;
            facts.push(FactRow {
                since,
                entities: names,
                text,
                evidence,
            });
        }

        let scenes = self
            .connection
            .prepare(
                "SELECT episode, start_ms, end_ms, place_id, summary FROM scenes
                 ORDER BY episode, start_ms, id",
            )?
            .query_map([], |row| {
                Ok(SceneRow {
                    episode: row.get(0)?,
                    start_ms: row.get(1)?,
                    end_ms: row.get(2)?,
                    place: row.get(3)?,
                    summary: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(DataView {
            entities,
            facts,
            scenes,
        })
    }

    /// True when an answer introduces an entity with this name.
    pub fn has_entity(&self, name: &str) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) > 0 FROM entities WHERE name = ?1",
            params![name],
            |row| row.get(0),
        )?)
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
    pub fn speakers_input(&self, episode: u32) -> Result<SpeakersInput> {
        require_subtitle(&self.connection, episode)?;
        Ok(SpeakersInput {
            lines: self.episode_lines(episode)?,
            entities: prompt_entities(&self.connection, Some(CHARACTER), episode)?,
            labels: unmapped_labels(&self.connection, episode)?,
            colors: unmapped_colors(&self.connection, episode)?,
        })
    }

    /// The input of a Story prompt. It fails when an episode has no `Ja` subtitle. The known
    /// entities are the ones that `prompt_entities` lists. The known facts are the facts before the
    /// episode that have no entity, or an entity that the prompt lists.
    pub fn story_input(&self, episode: u32) -> Result<StoryInput> {
        require_subtitle(&self.connection, episode)?;
        let entities = prompt_entities(&self.connection, None, episode)?;
        let listed: HashSet<&str> = entities.iter().map(|entity| entity.name.as_str()).collect();
        let facts = known_facts(&self.connection, episode)?
            .into_iter()
            .filter(|fact| {
                fact.entities.is_empty()
                    || fact
                        .entities
                        .iter()
                        .any(|name| listed.contains(name.as_str()))
            })
            .collect();
        Ok(StoryInput {
            lines: self.episode_lines(episode)?,
            entities,
            facts,
        })
    }

    /// The input of a context package at one position. It fails when the episode has no `Ja` subtitle.
    pub fn package_input(&self, at: Position) -> Result<PackageInput> {
        require_subtitle(&self.connection, at.episode)?;
        let entities = known_entities(&self.connection, at)?;
        let samples = package_samples(&self.connection, at, &entities)?;
        Ok(PackageInput {
            at,
            entities,
            facts: package_facts(&self.connection, at)?,
            scenes: package_scenes(&self.connection, at)?,
            lines: package_lines(&self.connection, at)?,
            samples,
        })
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

/// The declarations of the answers and the hand settings.
fn load_importances(connection: &Connection) -> Result<Importances> {
    let declarations = connection
        .prepare("SELECT name, episode, importance FROM importance_declarations ORDER BY rowid")?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|(name, episode, importance)| {
            Ok(Declaration {
                name,
                episode,
                importance: enum_from_text(&importance)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let overrides = connection
        .prepare("SELECT name, importance FROM importance_overrides")?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|(name, importance)| Ok((name, enum_from_text(&importance)?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(Importances::new(declarations, overrides))
}

/// Makes the entities of the rows. An entity is its name, so the name is also its ID. The
/// importance is the one at `up_to_episode`.
fn entities_of_rows(
    rows: Vec<(String, String, Option<String>)>,
    importances: &Importances,
    up_to_episode: u32,
) -> Vec<Entity> {
    rows.into_iter()
        .map(|(name, kind, reading)| Entity {
            id: EntityId(name.clone()),
            kind,
            reading,
            importance: importances.of(&name, up_to_episode),
            name,
        })
        .collect()
}

/// The entities that a prompt for the episode lists, in the order they were added. `kind` `None`
/// gives all kinds. An entity must be known by the end of the episode. A `main` or `recurring`
/// entity is always listed. A `minor` entity is listed only when it appeared in the last episode.
fn prompt_entities(
    connection: &Connection,
    kind: Option<&str>,
    episode: u32,
) -> Result<Vec<Entity>> {
    let importances = load_importances(connection)?;
    let appeared = appeared_before(connection, episode)?;
    let rows = connection
        .prepare(
            "SELECT name, kind, reading FROM entities
             WHERE (?1 IS NULL OR kind = ?1) AND since_episode <= ?2
             ORDER BY since_episode, since_ms, name",
        )?
        .query_map(params![kind, episode], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(entities_of_rows(rows, &importances, episode)
        .into_iter()
        .filter(|entity| {
            entity.importance != Importance::Minor || appeared.contains(entity.name.as_str())
        })
        .collect())
}

/// The names of the entities that appeared in the episode before `episode`, or later: the entity
/// became known, a fact has it, a scene has it as place, or it speaks in a line.
fn appeared_before(connection: &Connection, episode: u32) -> Result<HashSet<String>> {
    let mut statement = connection.prepare(
        "SELECT name FROM entities WHERE since_episode + 1 >= ?1
         UNION SELECT e.entity_id FROM fact_entities e JOIN facts f ON f.id = e.fact_id
               WHERE f.since_episode + 1 = ?1
         UNION SELECT place_id FROM scenes WHERE episode + 1 = ?1 AND place_id IS NOT NULL
         UNION SELECT s.entity_id FROM line_speakers s JOIN lines l ON l.id = s.line_id
               WHERE l.episode + 1 = ?1 AND s.entity_id IS NOT NULL",
    )?;
    let names = statement
        .query_map(params![episode], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(names)
}

/// The entities that are known at the position. A later entity is left out. The importance is
/// the one at the episode of the position.
fn known_entities(connection: &Connection, at: Position) -> Result<Vec<Entity>> {
    let importances = load_importances(connection)?;
    let rows = connection
        .prepare(
            "SELECT name, kind, reading FROM entities
             WHERE (since_episode, since_ms) <= (?1, ?2) ORDER BY since_episode, since_ms, name",
        )?
        .query_map(params![at.episode, at.time_ms], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(entities_of_rows(rows, &importances, at.episode))
}

/// The last lines of each character that are known at the position, oldest first. The lines have a
/// speaker from a label, a color, or an AI answer.
fn package_samples(
    connection: &Connection,
    at: Position,
    entities: &[Entity],
) -> Result<Vec<PackageSample>> {
    let mut statement = connection.prepare(
        "SELECT l.id FROM line_speakers s JOIN lines l ON l.id = s.line_id
         WHERE s.entity_id = ?1 AND s.method IN ('label', 'color', 'ai')
           AND (l.episode < ?2 OR (l.episode = ?2 AND l.end_ms <= ?3))
         ORDER BY l.episode DESC, l.start_ms DESC LIMIT ?4",
    )?;
    let mut samples = Vec::new();
    for entity in entities.iter().filter(|entity| entity.kind == CHARACTER) {
        let ids = statement
            .query_map(
                params![entity.name, at.episode, at.time_ms, SAMPLE_LINES],
                |row| row.get::<_, String>(0),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let lines = ids
            .iter()
            .rev()
            .map(|id| package_line(connection, id))
            .collect::<Result<Vec<_>>>()?;
        if !lines.is_empty() {
            samples.push(PackageSample {
                character: entity.id.clone(),
                lines,
            });
        }
    }
    Ok(samples)
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

/// The facts that start before the episode, in time order.
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

    let mut names = connection
        .prepare("SELECT entity_id FROM fact_entities WHERE fact_id = ?1 ORDER BY rowid")?;
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

/// The parts of the labels of one episode that have no meaning yet.
fn unmapped_labels(connection: &Connection, episode: u32) -> Result<Vec<String>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT speaker_label FROM lines WHERE episode = ?1 AND speaker_label IS NOT NULL",
    )?;
    let labels: Vec<String> = statement
        .query_map(params![episode], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;

    let mut parts: BTreeSet<String> = BTreeSet::new();
    for label in labels {
        for part in label.split('・') {
            let mapped: bool = connection.query_row(
                "SELECT COUNT(*) > 0 FROM speaker_labels WHERE label = ?1",
                params![part],
                |row| row.get(0),
            )?;
            if !mapped {
                parts.insert(part.to_string());
            }
        }
    }
    Ok(parts.into_iter().collect())
}

fn unmapped_colors(connection: &Connection, episode: u32) -> Result<Vec<u32>> {
    let mut statement = connection.prepare(
        "SELECT DISTINCT color FROM lines WHERE episode = ?1 AND color IS NOT NULL
         AND color NOT IN (SELECT color FROM speaker_colors) ORDER BY color",
    )?;
    let colors = statement
        .query_map(params![episode], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(colors)
}
