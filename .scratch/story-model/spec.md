# Spec: story model and context package

## Problem

A person writes fan fiction with an AI. The AI must know the characters, the world, and the story up to one point in the series. It must not know what comes after that point.

Today the app finds speakers only. It has no data about the characters, the places, or what happens. It cannot make the text that the person gives to the AI.

The AI also needs real lines. The lines are the primary source for how a character speaks and acts. A summary from an AI is not enough.

## Solution

The user works on one series directory. After the speakers step, the user does these steps:

1. The user makes a Story prompt for an episode and copies it to an AI chat. The prompt has the transcript with speakers, the known entities, and the known facts.
2. The user pastes the answer back. The answer becomes suggestions: new entities, facts, and scenes. Each entity and fact has evidence: lines of the subtitle.
3. The user approves or rejects each suggestion, as for the speakers.
4. The user selects a position (an episode and a time). The app makes a context package in YAML. The user copies it to an AI chat.

The package holds only what the story shows before the position. Each fact comes with its real lines.

## Decisions

### Principles

- The program only collects and sorts. It collects the data from before the position and puts it in time order.
- The AI that writes the fiction does the interpretation. It reads how the facts change over time and decides the current state. The AI that reads the transcript does not infer feelings or intent.
- The app never changes or deletes the past. All story data is added, never replaced. There is no "end" of a fact. Two facts can say different things at different times (for example, "A calls B 先輩" in episode 1 and "A calls B by the given name" in episode 5). Both stay.

### Model

| Word | Meaning |
| --- | --- |
| `Entity` | A noun in the world: a person, a place, an organization, an object, or a concept. It has a `kind` (free text) and evidence. It never changes. |
| `Fact` | One thing that lines show, with evidence. It refers to zero or more entities. |
| `Scene` | A range of lines in one episode, with an optional place and a one-sentence summary. |
| `Position` | A point in the story: an episode and a time in milliseconds. |

- The code reads only one value of `Entity.kind`: `character` (only a character can speak). It is the constant `CHARACTER` in `model`. A fact has no kind.
- A fact holds only what a line says or directly shows. It holds no interpretation, so it has no basis field.
- Rejected:
  - Items with an end, and events that start and end items. The program must decide when information ends, and this causes problems with ends, overwrites, and consistency checks.
  - A separate `Event`. An event is a fact, and the scene summaries tell the flow.
  - A kind for each fact. The code does not read it, and the prompt then needs a list of kinds in use.
  - A basis (fact or interpretation). The AI that writes does the interpretation.
  - A recap for each evidence line. The AI that writes it reads the whole episode, so a recap can leak what comes later. The app cannot check it.

### Position and time

- `Position { episode, time_ms }` derives `Ord`. It compares the episode, then the time.
- The `since` of an entity and of a fact is the largest `(episode, end_ms)` of its evidence lines. Do not use the earliest line, because the earliest line leaks later information. The prompt asks for the smallest set of lines, so the last line is the time when the information becomes known.
- An entity or a fact is known at P when `since <= P`. A scene is ended at P when `(episode, end_ms of its end line) <= P`.
- "A line before P" means a line with `(episode, end_ms) <= P`.
- The rebuild calculates `since` and stores it in `index.sqlite`. No file stores it.

### Suggestions and answers

- A suggestion ID is `<PromptId>-<n>`, as now. The ID of an entity, a fact, and a scene is the suggestion ID that made it.
- A suggestion depends on the suggestions that it refers to, as now. `Entity` has none. `Label`, `Color`, and `LineSpeaker` depend on entities. `Fact` depends on its entities. `Scene` depends on its place.
- A dependency always has a lower number. The Story answer is numbered in this order: new entities, facts, scenes. The Speakers answer keeps its order.
- Short references in a Story prompt: `L1` lines, `C1` entities of kind `character`, `T1` other entities. In an answer: `N1` for a new entity. Facts have no reference, because nothing refers to a fact.
- The Speakers answer format does not change. `new_characters` makes `Entity` suggestions with kind `character`. The answers that exist stay valid.
- An answer is all or nothing, as now. One bad entry makes the whole answer an error. The error names the section and the position, so the user can give it back to the AI.
- Rejected: skipping only the bad entries. It needs a list of skipped entries and cascade rules. The user can ask the AI to fix the answer.

### `model` (`src/model.rs`)

The plain types that more than one module uses. No logic, except the clock helpers.

Contracts:

```rust
pub const CHARACTER: &str = "character";

/// The suggestion ID that made the entity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EntityId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    pub id: EntityId,
    pub kind: String,
    pub name: String,
    pub reading: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position { pub episode: u32, pub time_ms: u32 }

pub fn format_clock(ms: u32) -> String;                  // "12:34"
pub fn parse_clock(text: &str) -> anyhow::Result<u32>;   // "12:34" to ms
```

Approach: `Entity` and `EntityId` replace `Character` and `CharacterId` in all code. The speaker rules, the prompts, and the index use `EntityId`. The clock code in `prompt.rs` (`time_text`) and `index.rs` (`evidence_text`) uses `format_clock`.

### `suggestions` (`src/suggestions/`)

Moves from `src/speakers/`: the suggestion, the prompt, and the answer code. `src/speakers/` keeps only the speaker rules.

The existing code fails two tests of `/codebase-design`:

- Change test. `series.rs` matches every variant of `Content` in `content_of`, `evidence_of`, and `data_key`. A new kind of suggestion changes four files. The fix: this knowledge moves to methods of `Content`. `series.rs` does not match on `Content`.
- Flag test. `prompt::make` takes a `kind` and holds the Speakers text. The fix: `src/suggestions/prompt/speakers.rs` and `src/suggestions/prompt/story.rs`, each with its own `make` and its own answer parser. The shared parts (`Prompt`, `PromptId`, `PromptKind`, `PromptLine`, `json_part`, the transcript line, the reference helpers) stay in `src/suggestions/prompt.rs`. `parse_answer` and `Series::create_prompt` are the only places that match on `PromptKind`.

Contracts:

```rust
/// One or more lines. A stored suggestion with no evidence cannot be read back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<LineId>", into = "Vec<LineId>")]
pub struct Evidence(Vec<LineId>);
impl Evidence {
    /// `None` for an empty list.
    pub fn new(lines: Vec<LineId>) -> Option<Evidence>;
    pub fn lines(&self) -> &[LineId];
}

/// A reference to an entity is the text of an `EntityId`.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Content {
    Entity { kind: String, name: String, reading: Option<String>, evidence: Evidence },
    Label { label: String, kind: LabelKind, character: Option<String> },
    Color { color: u32, character: String },
    LineSpeaker { line: LineId, characters: Vec<String> },
    Fact { entities: Vec<String>, text: String, evidence: Evidence },
    Scene { start: LineId, end: LineId, place: Option<String>, summary: String },
}

pub enum SuggestionKind { Entity, Label, Color, LineSpeaker, Fact, Scene }

impl Content {
    pub fn kind(&self) -> SuggestionKind;
    pub fn dependencies(&self) -> Vec<SuggestionId>;
    /// The lines that the suggestion is based on. Empty for a label and a color.
    /// For a scene: its start line and its end line.
    pub fn evidence_lines(&self) -> Vec<LineId>;
    /// For example `fact [明智あんな, 坂本みくる]: ...`. `names` maps an ID to an entity name.
    pub fn describe(&self, names: &HashMap<String, String>) -> String;
    /// The text that only one approved suggestion can hold: a label or a color.
    pub fn unique_key(&self) -> Option<String>;
}

// prompt
pub enum PromptKind { Speakers, Story }

pub struct SpeakersInput { /* the fields of the current `PromptInput`, with `entities: Vec<Entity>`
                              (kind `character` only) in place of `characters` */ }

pub struct StoryInput {
    pub lines: Vec<PromptLine>,
    pub entities: Vec<Entity>,    // all approved entities, all kinds
    pub facts: Vec<PromptFact>,   // approved facts with since_episode < episode, in time order
}
pub struct PromptFact {
    pub since: Position,
    pub entities: Vec<String>,    // entity names
    pub text: String,
}

pub enum AnswerError {
    NoJson,
    InvalidJson(serde_json::Error),
    UnknownRef(String),
    MissingEvidence { section: &'static str, position: usize },   // position from 1
    SceneOrder(usize),            // a scene whose end line is before its start line; position from 1
}

// speakers.rs and story.rs
pub fn make(id: PromptId, episode: u32, input: &SpeakersInput) -> Prompt;   // speakers.rs
pub fn make(id: PromptId, episode: u32, input: &StoryInput) -> Prompt;      // story.rs
// prompt.rs: matches on `prompt.kind` and calls the parser of that kind
pub fn parse_answer(prompt: &Prompt, text: &str) -> Result<Vec<Suggestion>, AnswerError>;
```

Approach:

- The Story answer:

```json
{
  "new_entities": [{ "ref": "N1", "kind": "place", "name": "...", "reading": "...", "evidence": ["L3"] }],
  "facts":  [{ "entities": ["C1", "N1"], "text": "...", "evidence": ["L12", "L15"] }],
  "scenes": [{ "start": "L1", "end": "L40", "place": "N1", "summary": "..." }]
}
```

- An entity reference is `C1`, `T1`, or `N1`. A line reference is `L1`. Any other reference gives `UnknownRef`.
- `entities` of a fact can be empty. `evidence` of a new entity and of a fact must not be empty (`MissingEvidence` with section `new_entities` or `facts`). The Speakers parser uses section `new_characters`.
- `SceneOrder`: the `L` number of `end` is lower than the `L` number of `start`. The `L` numbers of a Story prompt are one episode in time order.
- `prompts/story.md` holds the template. The code loads it with `include_str!` and fills each `{{name}}` with `str::replace`, as the Speakers prompt does. The transcript line has the same form as in the Speakers prompt. A known fact is one line: `E1 03:50 [明智あんな, 坂本みくる] text`, with `[]` when it has no entities.
- Draft of `prompts/story.md`:

```
Extract information about the story from this episode of an anime.

Known entities:
{{entities}}

Known facts before this episode:
{{facts}}

Transcript of episode {{episode}}. Each line has an ID, a time, a speaker label, the text, and the speaker ([?] is unknown, [-] is no speaker):
{{transcript}}

Tell me:
- new entities: a person, a place, an organization, an object, or a concept. Give each one a kind, for example character, place, organization, object, or concept.
- facts: one thing that the lines show about entities. For example, a trait that a character shows, a speech style, how one character calls another, a relationship, a goal, what a character knows, what happens, or a description of a place.
- scenes: a range of lines in one place, with the place and a one-sentence summary of what happens.

Rules:
- Use only this transcript, not other knowledge of the series.
- Write only what a line says or directly shows. Do not guess feelings or intent.
- Write only new or changed information. Do not repeat a known fact.
- Write the flow of the story in the scene summaries. Write facts that stay true about characters, relationships, and the world.
- Give evidence lines for each entity and fact. Give the smallest set of lines that shows it. The last evidence line is the time when it becomes known.
- To show an exchange, also give the line of the other speaker as evidence.
- For a speech style, use lines that the character speaks. For a way to call someone, use lines that have the term.

Reply with one JSON object in this format:
{{format}}
```

### `package` (`src/package.rs`)

Contracts:

```rust
pub struct PackageInput {
    pub at: Position,
    pub entities: Vec<Entity>,         // since <= at, all kinds, in since order
    pub facts: Vec<PackageFact>,       // since <= at, in since order
    pub scenes: Vec<PackageScene>,     // start line starts at or before `at`; includes a scene in progress
    pub lines: Vec<PackageLine>,       // the lines of `at.episode` with end_ms <= at.time_ms, in time order
}
pub struct PackageFact {
    pub since: Position,
    pub entities: Vec<EntityId>,
    pub text: String,
    pub evidence: Vec<PackageLine>,    // in time order
}
pub struct PackageScene {
    pub episode: u32, pub start_ms: u32, pub end_ms: u32,
    pub place: Option<EntityId>, pub summary: String,
}
pub struct PackageLine {
    pub episode: u32, pub start_ms: u32, pub end_ms: u32, pub text: String,
    pub speakers: Vec<EntityId>,       // only methods Label, Color, and Ai
}

pub fn build(input: &PackageInput) -> String;   // YAML
```

Approach:

- `build` is a pure function. `Index::package_input` reads the data, as `Index::prompt_input` does now. `package` does not know the index.
- The package:

```yaml
position: { episode: 12, time: "23:40" }
entities:
  - { name: "明智あんな", reading: "あけちあんな", kind: "character" }
scenes:
  - { episode: 1, start: "00:30", end: "04:10", place: "教室", summary: "..." }
facts:
  - episode: 5
    time: "14:02"
    entities: ["明智あんな", "坂本みくる"]
    text: "..."
    lines:
      - { episode: 5, time: "14:02", speaker: "明智あんな", text: "……みくる。" }
current_scene:
  place: "屋上"
  speakers: ["明智あんな", "坂本みくる"]
  lines:
    - { episode: 12, time: "22:05", speaker: "坂本みくる", text: "……来年も、ここで。" }
```

- `entities`: all entities in the input.
- `scenes`: the scenes that ended at `at`, in the order of their start `(episode, start_ms)`. A scene in progress is not in this section, because its summary can tell what comes after the position.
- `facts`: all facts, in `since` order. `time` is the time of `since`. `lines` are all evidence lines, in time order.
- `current_scene`: the current scene is the scene of `at.episode` with `start_ms <= at.time_ms < end_ms`. When there are two, use the one with the latest start. `lines` are the lines from the start of the current scene. When there is no current scene, `lines` are the lines after the end of the last ended scene of the episode, or from the start of the episode. There is no limit on the number of lines. `place` is the place of the current scene. `speakers` are the speakers of `lines`, each once, in the order of their first line.
- The package knows only the entities in `input.entities`. An entity that is not there is never written: it is left out of `entities` of a fact, `place` is left out, and a speaker is left out. This stops a name that the story shows later (for example, a real name) from leaking.
- A key whose value is not known is left out (`reading`, `place`, `speaker`). An empty list is written as `[]`.
- `time` is `format_clock` of `start_ms` for a line and of `since.time_ms` for a fact.
- `build` makes a tree of maps, lists, text, and numbers. A small private renderer writes it with 2 spaces for each level. Every text value is written with `serde_json::to_string`, which is valid YAML. A map whose values are all scalars is written on one line (`{ a: 1, b: "x" }`).
- Rejected: a YAML crate (`serde_yaml` has no maintainer, and the app only writes YAML); one `timeline` list that mixes scenes and facts (two kinds of entries in one list are hard to read); a filter by character or place (the program then decides what is related; this can come later); a fixed number of lines for the current scene (it crosses scene borders).

### `series` (`src/series.rs`, `src/series/`)

Contracts:

```rust
impl Series {
    // changed: PromptKind::Story is new
    pub fn create_prompt(&self, kind: PromptKind, episode: u32) -> Result<Prompt>;
    // new
    pub fn build_package(&self, at: Position) -> Result<String>;
}

pub struct EpisodeProgress {
    // existing fields, plus:
    pub facts: u32,        // approved facts with since_episode = episode
}
```

Approach:

- `Series::suggestions` uses `Content::describe` and `Content::evidence_lines`. `decide` uses `Content::unique_key`. Their behavior does not change.
- `create_prompt` reads the input for its kind (`Index::speakers_input` or `Index::story_input`) and calls the `make` of that kind. An episode with no `Ja` subtitle is an error, as now.
- `build_package` calls `Index::package_input(at)`, then `package::build`. An episode with no `Ja` subtitle is an error. It keeps no connection after it returns.
- The rebuild order: sources, lines, entities, labels, colors, line speakers, speakers of each episode, facts, scenes.
- An approved suggestion with a line that is not in `lines` is a rebuild error with the suggestion ID, as now. This applies to entities, facts, and scenes.
- Position checks use row values: `(since_episode, since_ms) <= (?1, ?2)`.
- `SCHEMA_VERSION` goes to 2. There is no migration. `Index::open` rebuilds when the version differs.
- Index tables (version 2). The table `characters` becomes `entities`. The column `character_id` in the existing tables becomes `entity_id`.

```sql
entities (id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL, reading TEXT,
          since_episode INTEGER NOT NULL, since_ms INTEGER NOT NULL);
facts (id TEXT PRIMARY KEY, text TEXT NOT NULL,
       since_episode INTEGER NOT NULL, since_ms INTEGER NOT NULL);
fact_entities (fact_id TEXT NOT NULL REFERENCES facts(id), entity_id TEXT NOT NULL REFERENCES entities(id));
fact_evidence (fact_id TEXT NOT NULL REFERENCES facts(id), line_id TEXT NOT NULL REFERENCES lines(id));
scenes (id TEXT PRIMARY KEY, episode INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
        place_id TEXT REFERENCES entities(id), summary TEXT NOT NULL);
```

- The rebuild code and the query code can be in separate files under `src/series/`.

### `ui` (`src/app.rs`)

Standard egui widgets only, as now.

- Prompts tab: add `[New Story prompt]` and the `facts` column.
- Suggestions tab: the kind filter has the new kinds.
- Package tab: an episode `ComboBox`, a time `TextEdit` (`mm:ss`; an invalid value is an error from `parse_clock`), `[Generate]`, `[Copy]`, and the result in a read-only `TextEdit`.
- `build_package` is a read. The UI runs it on a worker thread, like the other jobs.

## Seams

1. `Series` (integration tests in `tests/`). Prior art: `tests/speakers.rs`. The test creates a series in a `TempDir`, copies `testdata/raw.ass` as episode 1, and calls the public methods as the UI does. The AI answers are JSON files in `tests/fixtures/`. New tests:
   - `tests/story.rs`: a Story prompt has the known entities and the facts from before the episode; submit a Story answer and approve it in dependency order; a blocked or invalid suggestion is not data; `since` comes from the latest evidence line; a rebuild error names the suggestion when its evidence line is missing; the Speakers answers that exist still work.
   - `tests/package.rs`: the package has nothing from after its position (a scene in progress, a fact and an entity with a later `since`, a name of an entity with a later `since`); each section; scenes in start order and facts in `since` order; a fact shows all its evidence lines; `current_scene` stops at the scene start and has no limit; the fallback when no scene holds the position; a `PreviousLine` speaker gives no speaker; the index can be deleted and the package is the same after the rebuild.
2. Answer parsing (unit tests in `src/suggestions/prompt/`). Prior art: the tests of `src/speakers/prompt.rs`. They cover the numbering order, `UnknownRef`, `MissingEvidence`, `SceneOrder`, and a fact with no entities.

The speaker rules and the review rules keep their unit tests. The UI has no tests. `package::build` has no tests of its own. Tests reach it through `Series::build_package`.

## Docs

- `ARCHITECTURE.md`:
  - Mental model: the Files list names `src/suggestions/` in place of `src/speakers/`. The Library line says that it also builds the context package.
  - Code map: add `src/suggestions/` (suggestions, decisions, prompts, and answers) and the package (`src/`, because it is one file). Change `src/speakers/` to "speaker rules".
  - Invariants, add: every claim from an external model has evidence, which is lines of a source; a context package holds nothing from after its position; the app never changes or ends story data, it only adds it; the app calculates `since` when it rebuilds, and no file stores it.
- `docs/requirements.md`:
  - Purpose and Output: the context package is based on primary sources. It supports many kinds of sources (subtitles first, then audio and video). The AI that reads the package does the interpretation.
  - Output: the person chooses a point in the story (an episode and a time). The package holds the entities, the scenes, the facts with their real lines, and the lines of the current scene. It is YAML text. Remove "the characters, and the place" from the choice and remove "a confidence".
  - Scope: change "keep facts and interpretations apart, each with its source" to "keep facts, each with its source; the reading AI does the interpretation".

## Out of scope

- Audio and video analysis. A later kind of evidence will cover them.
- A limit on the size of the package, and a filter by character, place, or arc. The size grows with the number of episodes.
- A limit on the size of the Story prompt for a series with many facts.
- Adding evidence to an existing fact. The user rejects a duplicate, or the AI that writes reads past it.
- Example lines for a label or a color on the approval screen.
- A check that an evidence line supports its fact. The approval screen shows the lines. The user decides.
- Time inside the story (flashbacks), and a profile of the whole work.
- JSON output of the package, and saving the package to a file.
- The rule-based check of a draft for contradictions.
- Special episodes (`SP1`, `13.5`).
