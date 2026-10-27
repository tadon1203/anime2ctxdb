# Spec: subtitle pipeline

## Problem

A person wants to write with an AI about an anime. The AI does not know the story well, or it knows the story past the point where the person writes. The person has the subtitle files and the video files of the series. They have no tool that turns these files into correct context for a given point in the story.

## Solution

A desktop app (egui) works on one series directory at a time.

1. The user imports subtitle and video files. The app detects the kind, the episode, and the language. The user corrects them in a table, then saves. The app never changes the original files.
2. The app reads the Japanese subtitle of each episode. It finds the speakers that rules can find: the label in parentheses, the text color, and the previous line.
3. The app makes a Speakers prompt for an episode. The user copies it to an AI chat and pastes the answer back. The answer becomes proposals. The user approves or rejects each proposal.
4. The app makes an Items prompt for an episode. It has the transcript with speakers and the data that is already approved. The answer becomes proposals for characters, terms, scenes, and items. The user approves or rejects them.
5. The user selects a position (an episode and a time), characters, and a place. The app makes a context package in Markdown. The package holds only information that the story shows before that position.

The UI is plain and rough, with standard egui widgets only. It is easy to use: the user can always see what to do next.

## Decisions

### Crate and dependencies

- One package with `src/lib.rs` and `src/main.rs`. All logic is in the library. `main.rs` and `src/ui.rs` hold the egui code.
- Dependencies: `eframe`, `rfd`, `rusqlite` (feature `bundled`), `serde`, `serde_json`, `sha2`, `regex`, `anyhow`, `thiserror`, `fs4`, `dirs`. Dev dependency: `tempfile`. Add other crates only when a ticket needs them.
- No `clap`, no `log`, no async runtime, no template engine, no ORM.

### Words

Use one word for one thing, in code and in the UI. The UI is in English.

| Word | Meaning |
| --- | --- |
| `Series` | One series. Also its directory. |
| `source` | A file that the user imported. Never "evidence". |
| `evidence` | The lines that an item or a proposal is based on. |
| `Dialogue` | One `Dialogue:` row of an ASS file. |
| `Line` | One or more `Dialogue` rows merged into one spoken line. |
| `SpeakerLabel` | The text in parentheses at the start of a line, for example `（あんなの母）`. |
| `Character` | A person in the story. |
| `Term` | A proper noun that is not a character: a place, an organization, an object, a technique. |
| `Item` | One piece of information about the story. Its `basis` is `Fact` or `Interpretation`. |
| `Scene` | A time range in an episode, with a place, characters, and a summary. |
| `Prompt` / `Answer` | The text that the user gives to an AI / the text that the AI gives back. |
| `Proposal` | A candidate piece of data from an answer. It becomes data only when the user approves it. |
| `Decision` | The user's action on one proposal: `approve`, `reject`, or `revoke`. |
| `Position` | A point in the story: an episode number and a time in milliseconds. |
| `ContextPackage` | The Markdown text that the app makes for one position. |

### Series directory

```
<series>/
  sources.jsonl          one record for each save of a source (append only)
  sources/<sha256>.ass   copies of the imported subtitle files
  prompts/<id>.json      the prompts, with their reference tables
  answers/<id>.json      the JSON part of each answer
  decisions.jsonl        decisions (append only)
  index.sqlite           derived, the app can delete it and build it again
```

- The series name is the directory name. There is no `series.json`.
- Video files stay where they are. `sources.jsonl` records their path, size, and SHA-256.
- The source of truth is all files except `index.sqlite`.

### Files and data

- A JSONL file gets one line for each record. The app writes each line with one `write` call and then calls `sync_all`.
- The app writes a prompt or an answer file to a temporary file in the same directory, then renames it.
- When a JSONL line or a JSON file is not valid, reading fails with the file path and the line number. The app does not skip it and does not repair it.
- In `sources.jsonl`, the last record for a source ID is the current one.
- In `decisions.jsonl`, the last decision for a proposal ID is the current one. The order of the lines gives the order of the decisions. A decision has no time field.

### IDs

All IDs are newtypes around `String`, except `PromptId`.

| ID | Value |
| --- | --- |
| `SourceId` | The SHA-256 of the file, as lowercase hex. |
| `LineId` | `<SourceId>:<n>`. `n` is the index of the first merged `Dialogue` in the file, from 0. The source file never changes, so the ID stays the same when the merge rules change. |
| `PromptId(u32)` | 1, 2, 3, ... The next ID is the largest ID in `prompts/` plus 1. |
| `ProposalId` | `<PromptId>-<n>`. `n` is the position of the proposal in the answer, from 1. Sort proposal IDs by the two numbers, not as text (`12-2` comes before `12-10`). |
| `CharacterId`, `TermId`, `ItemId`, `SceneId` | The `ProposalId` of the proposal that made it. There is no other ID scheme. |

Inside a prompt, the app uses short references: `L1` for lines, `C1` for characters, `T1` for terms, `I1` for items. The answer uses `N1`, `N2`, ... for new characters and new terms. The prompt file holds the table from short reference to real ID.

### Position and time

- `Position { episode: u32, time_ms: u32 }` derives `Ord`. Comparison is by episode, then by time.
- The `since` of an item, a character, a term, or a scene is the latest evidence line: the largest `(episode, end_ms)` of its evidence lines. Do not use the earliest line. The earliest line would leak information that the story shows later.
- An approved item B with `replaces: A` ends A. `A.until = B.since`. The rebuild calculates `until`. No file stores it.
- An item is valid at position P when `since <= P` and (`until` is null or `until > P`).

### Rebuild of the database

- `index.sqlite` is fully derived. There is no migration and no backward compatibility. `PRAGMA user_version` holds `db::SCHEMA_VERSION`. When the file is missing or the version is different, the app builds it again.
- Every write (save sources, decide, submit an answer) runs a full rebuild after it writes its file. A new prompt does not change the data, so it does not run a rebuild.
- The rebuild writes `index.sqlite.tmp`, then renames it to `index.sqlite`. A failed rebuild never leaves a half-built database.
- Rejected: rebuild later (the UI would have to know when the database is old); incremental update (the state would have to be synchronized by hand).

### Errors

- Use `anyhow::Result` for all functions, with `.with_context(...)` on each I/O call. The context names the path or the ID.
- `thiserror` enums exist only where a caller or a test matches the variant: `ass::AssError` and `prompt::AnswerError`.
- A bug panics. The app does not use `catch_unwind`. When the worker thread panics, the UI thread sees the closed channel and panics too.
- The UI shows every error in one line at the top: `Error: {:#}`. The line stays until the user clicks `Clear`.

### Threads

- `Series` holds only its directory path. It is `Clone + Send`. Each call opens its own SQLite connection.
- The UI runs each write and each slow call (`detect_files`, `save_sources`, `decide`, `submit_answer`, `new_prompt`) on a new `std::thread`. A UI view never keeps a connection open, so the rename at the end of a rebuild works on Windows. At most one job runs at a time. The UI shows `Working...` and disables the buttons while a job runs.
- The worker sends one message on a `std::sync::mpsc` channel:

  ```rust
  enum Done {
      Detected(Vec<SourceRecord>),
      Prompt(Prompt),
      Status(String),
  }
  // the channel carries anyhow::Result<Done>
  ```

- When a job ends, the UI reads its views again (sources, proposals, progress) on the UI thread with synchronous queries.

### One instance

`main` locks `<dirs::data_local_dir()>/anime2ctxdb/app.lock` with `fs4` `try_lock_exclusive` before it opens the window. When the lock fails, it shows `rfd::MessageDialog` with `anime2ctxdb is already running` and exits with code 1. The lock stays held until the process ends.

### `model` (`src/model.rs`)

The plain data types that more than one module uses. No logic, except `Position` ordering and `Display` for IDs.

```rust
pub struct SourceId(pub String);
pub struct LineId(pub String);
pub struct ProposalId(pub String);
pub struct CharacterId(pub String);
pub struct TermId(pub String);
pub struct ItemId(pub String);
pub struct SceneId(pub String);
pub struct PromptId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position { pub episode: u32, pub time_ms: u32 }

pub enum SourceKind { Subtitle, Video }
pub enum Language { Ja, Zh, En, Other }

pub struct Character { pub id: CharacterId, pub name: String, pub reading: Option<String> }
pub struct Item { pub id: ItemId, pub kind: ItemKind, pub subject: Option<String>, pub object: Option<String>,
                  pub text: String, pub basis: Basis, pub since: Position, pub until: Option<Position> }
pub struct Term { pub id: TermId, pub name: String, pub reading: Option<String>, pub category: TermCategory }
pub enum TermCategory { Place, Organization, Object, Technique, Other }

pub enum LabelKind { Character, Extra, Sound, Note }  // Extra: a person who is not a character, for example 女の子
pub enum SpeakerMethod { Label, Color, PreviousLine, Ai }

pub enum ItemKind { Summary, Trait, SpeechStyle, Relationship, AddressTerm, Appearance, Knowledge, Status, Description }
pub enum Basis { Fact, Interpretation }
```

All enums derive `serde` with `rename_all = "snake_case"`. The database stores an enum as that same snake_case text.

### `source` (`src/source.rs`)

Contract:

```rust
pub struct SourceRecord {
    pub id: SourceId,
    pub kind: SourceKind,
    pub path: PathBuf,              // the original path
    pub size: u64,
    pub episode: Option<u32>,       // None: the app ignores this source
    pub language: Option<Language>, // None for a video
}

pub fn detect(path: &Path) -> anyhow::Result<SourceRecord>;
pub fn read_records(series_dir: &Path) -> anyhow::Result<Vec<SourceRecord>>; // the current record for each ID
pub fn save_records(series_dir: &Path, records: &[SourceRecord]) -> anyhow::Result<()>;
```

Approach:

- Kind from the extension: `.ass` is a subtitle. `.mkv`, `.mp4`, `.webm`, and `.avi` are videos. Any other extension is an error.
- `detect` hashes the file with SHA-256 while it reads it in blocks.
- Episode from the file name, with the first pattern that matches: `第(\d+)話`, `[Ee][Pp]?(\d+)`, ` - (\d+)`, `\[(\d+)\]`, `#(\d+)`. No match gives `None`.
- Language from the subtitle text: any kana (U+3040 to U+30FF) gives `Ja`. Else any CJK ideograph gives `Zh`. Else any ASCII letter gives `En`. Else `Other`.
- When the ID is already in `sources.jsonl`, `detect` returns the current record (with the new path), not a new detection.
- `save_records` validates first: an episode is 1 or more, a video has no language, and an episode has at most one `Ja` subtitle, and the episode numbers of all records with an episode are 1, 2, 3, ... with no gap (counting the records that are already saved). A gap is an error that names the missing number. Then it copies each new subtitle to `sources/<id>.ass` and appends the records.

### `ass` (`src/ass.rs`)

Contract:

```rust
pub struct Line {
    pub index: u32,                     // index of the first merged Dialogue, from 0
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,                   // without tags, label, and ⚟⚞
    pub color: Option<u32>,             // RGB; None for the default color
    pub offscreen: bool,                // the text had ⚟ ... ⚞
    pub speaker_label: Option<String>,  // for example "あんな・みくる"
}

#[derive(Debug, thiserror::Error)]
pub enum AssError {
    #[error("no [Events] section")]
    MissingEventsSection,
    #[error("no Format line in [Events]")]
    MissingFormat,
    #[error("invalid Dialogue at line {line_number}")]
    InvalidDialogue { line_number: usize },
}

pub fn parse(text: &str) -> Result<Vec<Line>, AssError>;
```

Approach:

- The caller reads the file as UTF-8 and removes a BOM. Text that is not UTF-8 is an `anyhow` error in the caller.
- Find the field positions from the `Format:` line of `[Events]`. `Text` is the last field and can contain commas, so split with `splitn`.
- Time `H:MM:SS.cc` becomes milliseconds.
- ASS writes a color as BGR (`\c&H0000FFFF` is yellow). `parse` changes it to RGB (`0xFFFF00`). All other code uses RGB only.
- A `Dialogue` with `\fscy` less than 100 in its first override block is ruby. Drop it.
- Remove `{...}` blocks. Change `\N`, `\n`, and `\h` to a space. Remove `⚟` and `⚞`, and set `offscreen`. Trim the text. Drop a `Dialogue` whose text is then empty.
- When the text starts with `（...）`, the inner text is `speaker_label`. Parentheses at other places stay in the text as notes, for example `♬～（鼻歌）`.
- Merge a `Dialogue` into the previous `Line` when it has the same start and end, the same color, and no label. Join the texts with a space.
- Sort the lines by start time. Keep the file order for equal start times.

### `speaker` (`src/speaker.rs`)

Contract:

```rust
pub enum Speaker {
    Unknown,
    Nobody,                                       // a sound, an extra, or an approved empty list
    Characters(Vec<CharacterId>, SpeakerMethod),
}

pub struct Label { pub kind: LabelKind, pub character: Option<CharacterId> }

pub fn resolve(
    lines: &[(LineId, &ass::Line)],
    labels: &HashMap<String, Label>,              // approved labels
    colors: &HashMap<u32, CharacterId>,           // approved colors
    line_speakers: &HashMap<LineId, Vec<CharacterId>>, // approved line speakers; empty: nobody
) -> Vec<Speaker>; // same order as lines
```

Approach, for each line of one episode in time order:

1. An approved line speaker wins: `Characters(ids, Ai)`, or `Nobody` for an empty list. One line can have more than one speaker, for example when several characters shout the same line.
2. A label that is not a `Note`: split it at `・`. Each part must be an approved label, else `Unknown`. `Character` parts give `Characters(ids, Label)`. When all parts are `Extra` or `Sound`, the result is `Nobody`.
3. A color: an approved color gives `Characters([c], Color)`, else `Unknown`.
4. No color and no label (or a `Note` label): find the nearest earlier line with no color. When it has `Characters` with method `Label` or `PreviousLine`, and `this.start_ms - that.end_ms <= 10_000`, the result is `Characters(same, PreviousLine)`. Else `Unknown`.

### `proposal` (`src/proposal.rs`)

Contract:

```rust
pub struct Proposal { pub id: ProposalId, pub body: Body }

pub enum Body {
    Character { name: String, reading: Option<String>, evidence: Vec<LineId> },
    Term { name: String, reading: Option<String>, category: TermCategory, evidence: Vec<LineId> },
    Label { label: String, kind: LabelKind, character: Option<String> },
    Color { color: u32, character: String },
    LineSpeaker { line: LineId, characters: Vec<String> },  // empty: no speaker
    Scene { start: LineId, end: LineId, place: Option<String>, characters: Vec<String>, summary: String },
    Item { kind: ItemKind, subject: Option<String>, object: Option<String>, text: String,
           basis: Basis, evidence: Vec<LineId>, replaces: Option<String> },
}
// A String reference is a CharacterId, a TermId, an ItemId, or a ProposalId of a new
// character or term in the same answer. These are the same values (see IDs).

pub enum Action { Approve, Reject, Revoke }
pub struct Decision { pub proposal: ProposalId, pub action: Action }

pub enum Status { Pending, Approved, Rejected, Invalid }
pub struct State { pub status: Status, pub blocked: bool }

pub fn read_decisions(series_dir: &Path) -> anyhow::Result<Vec<Decision>>;
pub fn append_decisions(series_dir: &Path, decisions: &[Decision]) -> anyhow::Result<()>;
pub fn states(proposals: &[Proposal], decisions: &[Decision]) -> HashMap<ProposalId, State>;

impl Body { pub fn kind(&self) -> ProposalKind; }
pub enum ProposalKind { Character, Term, Label, Color, LineSpeaker, Scene, Item }
```

Approach:

- The own status of a proposal is its last decision: `approve` gives `Approved`, `reject` gives `Rejected`, `revoke` or no decision gives `Pending`.
- A proposal depends on each proposal that its references name. Its status is `Invalid` when a dependency is `Rejected` or `Invalid`. It is blocked when a dependency is `Pending`.
- The database holds a proposal as data only when it is `Approved` and not blocked.
- A decision line: `{"proposal":"12-4","action":"approve"}`.

### `prompt` (`src/prompt.rs`, `prompts/speakers.md`, `prompts/items.md`)

Contract:

```rust
pub enum PromptKind { Speakers, Items }

#[derive(Serialize, Deserialize)]
pub struct Prompt {
    pub id: PromptId,
    pub kind: PromptKind,
    pub episode: u32,
    pub text: String,
    pub refs: BTreeMap<String, String>, // short reference -> real ID
}

pub struct PromptInput {
    pub lines: Vec<PromptLine>,          // the episode transcript
    pub characters: Vec<Character>,
    pub terms: Vec<Term>,
    pub items: Vec<Item>,                // Items prompt: items valid at the start of the episode
    pub labels: Vec<String>,             // Speakers prompt: labels with no approved decision
    pub colors: Vec<u32>,                // Speakers prompt: colors with no approved decision
}
pub struct PromptLine { pub id: LineId, pub start_ms: u32, pub color: Option<u32>,
                        pub label: Option<String>, pub text: String, pub speaker: Speaker }

#[derive(Debug, thiserror::Error)]
pub enum AnswerError {
    #[error("no JSON object in the answer")]
    NoJson,
    #[error("invalid JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),
    #[error("unknown reference {0}")]
    UnknownRef(String),
    #[error("entry {0} has no evidence")]
    MissingEvidence(usize),
}

pub fn make(id: PromptId, kind: PromptKind, episode: u32, input: &PromptInput) -> Prompt;
pub fn parse_answer(prompt: &Prompt, text: &str) -> Result<Vec<Proposal>, AnswerError>;
```

Approach:

- `make` and `parse_answer` are pure. `series` reads the input from the database. `prompt` does not use `db`, so there is no cycle.
- The templates are in `prompts/*.md`. The code loads them with `include_str!` and fills `{{name}}` places with `str::replace`.
- `make` gives short references in order (`L1`, `C1`, `T1`, `I1`) and stores them in `refs`.
- A transcript line in the prompt: `L12 01:01 #FFFF00 （あんなの母） あの子のリボンが 風でとばされたって [C2]`. The color is RGB hex and is shown only when it is set. The speaker is `[C2]`, `[C1,C2]` for more than one speaker, `[-]` for no speaker, or `[?]` for unknown.
- `parse_answer` takes the content of the first ` ```json ` block, or else the whole text. It parses it with `serde_json` into the answer structs below. Then it changes each short reference to a real ID. A new entity `N1` becomes the `ProposalId` of its own proposal. An unknown reference gives `UnknownRef`.
- The proposals are numbered in this order: new characters, new terms, then the other entries in the order of the answer. So a new entity comes before the entries that use it.
- The style of a prompt: short; no role play; the goal, the input, and the JSON format. Only three rules: use only this transcript and not other knowledge of the series; give evidence line IDs (the smallest set of lines that shows the item); mark each item `fact` or `interpretation`. The instructions are in English. The values are in the language of the transcript.

Speakers answer:

```json
{
  "new_characters": [{ "ref": "N1", "name": "明智あんな", "reading": "あけちあんな", "evidence": ["L4"] }],
  "labels": [{ "label": "あんな", "kind": "character", "character": "N1" },
             { "label": "窓の開く音", "kind": "sound" }],
  "colors": [{ "color": "#FFFF00", "character": "N1" }],
  "lines": [{ "line": "L88", "characters": ["C2"] },
            { "line": "L89", "characters": ["C1", "C2", "C3"] },
            { "line": "L90", "characters": [] }]
}
```

Items answer:

```json
{
  "new_characters": [],
  "new_terms": [{ "ref": "N1", "name": "...", "reading": "...", "category": "place", "evidence": ["L3"] }],
  "scenes": [{ "start": "L1", "end": "L40", "place": "N1", "characters": ["C1", "C2"], "summary": "..." }],
  "items": [{ "kind": "trait", "subject": "C1", "object": null, "text": "...", "basis": "interpretation",
              "evidence": ["L12", "L15"], "replaces": "I40" }]
}
```

Draft of `prompts/speakers.md`:

```
Find who speaks each line in this episode of an anime.

Known characters:
{{characters}}

Transcript. Each line has an ID, a time, a text color, a speaker label, the text, and the current speaker ([?] is unknown, [-] is no speaker):
{{transcript}}

Tell me:
- what each of these speaker labels is: {{labels}}
- which character each of these text colors belongs to: {{colors}}
- who speaks each line marked [?] (a line can have more than one speaker, or none)

Use only this transcript, not other knowledge of the series. Leave out what you cannot tell from it.

Reply with one JSON object in this format:
{{format}}
```

Draft of `prompts/items.md`:

```
Extract information about the story from this episode of an anime.

Known characters:
{{characters}}

Known terms:
{{terms}}

Known items at the start of this episode:
{{items}}

Transcript of episode {{episode}}:
{{transcript}}

Give the scenes of this episode, one summary item for the whole episode, new terms, and new items:
traits, speech style, relationships, how characters address each other, appearance, what characters know,
status, and descriptions of terms. When an item changes a known item, set "replaces".

Use only this transcript, not other knowledge of the series. Give the smallest set of evidence lines for each item.
Mark each item "fact" when a line states it, or "interpretation" when you infer it.

Reply with one JSON object in this format:
{{format}}
```

### `db` (`src/db.rs`, `src/schema.sql`)

Contract:

```rust
pub const SCHEMA_VERSION: i32 = 1;

pub fn open(series_dir: &Path) -> anyhow::Result<rusqlite::Connection>; // rebuilds when missing or old
pub fn rebuild(series_dir: &Path) -> anyhow::Result<()>;
```

Plus the query functions that `series` and `context` use. Each takes `&Connection` and returns plain structs. The implementer names them.

Approach:

- The rebuild order: read `sources.jsonl` → read `decisions.jsonl` → read each prompt and its answer, and run `prompt::parse_answer` again to get the proposals → `proposal::states` → insert sources → parse each `Ja` subtitle with an episode, and insert lines → insert approved characters and terms → insert approved labels, colors, and line speakers → run `speaker::resolve` for each episode, and insert `line_speakers` → insert approved scenes and items, with `since` from the evidence → calculate `until` → insert all proposals with status and blocked.
- All inserts run in one transaction.
- The schema (`src/schema.sql`, loaded with `include_str!`):

```sql
CREATE TABLE sources (id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT NOT NULL, size INTEGER NOT NULL,
                      episode INTEGER, language TEXT);
CREATE TABLE lines (id TEXT PRIMARY KEY, source_id TEXT NOT NULL REFERENCES sources(id), episode INTEGER NOT NULL,
                    start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, text TEXT NOT NULL,
                    color INTEGER, offscreen INTEGER NOT NULL, speaker_label TEXT);
CREATE TABLE characters (id TEXT PRIMARY KEY, name TEXT NOT NULL, reading TEXT,
                         since_episode INTEGER NOT NULL, since_ms INTEGER NOT NULL);
CREATE TABLE terms (id TEXT PRIMARY KEY, name TEXT NOT NULL, reading TEXT, category TEXT NOT NULL,
                    since_episode INTEGER NOT NULL, since_ms INTEGER NOT NULL);
CREATE TABLE speaker_labels (label TEXT PRIMARY KEY, kind TEXT NOT NULL, character_id TEXT REFERENCES characters(id));
CREATE TABLE speaker_colors (color INTEGER PRIMARY KEY, character_id TEXT NOT NULL REFERENCES characters(id));
CREATE TABLE line_speakers (line_id TEXT NOT NULL REFERENCES lines(id), character_id TEXT REFERENCES characters(id),
                            method TEXT NOT NULL);  -- character_id NULL: no speaker
CREATE TABLE scenes (id TEXT PRIMARY KEY, episode INTEGER NOT NULL, start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL,
                     place_id TEXT REFERENCES terms(id), summary TEXT NOT NULL);
CREATE TABLE scene_characters (scene_id TEXT NOT NULL REFERENCES scenes(id), character_id TEXT NOT NULL REFERENCES characters(id),
                               PRIMARY KEY (scene_id, character_id));
CREATE TABLE items (id TEXT PRIMARY KEY, kind TEXT NOT NULL,
                    subject_character_id TEXT REFERENCES characters(id), subject_term_id TEXT REFERENCES terms(id),
                    object_character_id TEXT REFERENCES characters(id), text TEXT NOT NULL, basis TEXT NOT NULL,
                    since_episode INTEGER NOT NULL, since_ms INTEGER NOT NULL,
                    until_episode INTEGER, until_ms INTEGER, replaces TEXT REFERENCES items(id),
                    CHECK (subject_character_id IS NULL OR subject_term_id IS NULL));
CREATE TABLE item_evidence (item_id TEXT NOT NULL REFERENCES items(id), line_id TEXT NOT NULL REFERENCES lines(id),
                            PRIMARY KEY (item_id, line_id));
CREATE TABLE proposals (id TEXT PRIMARY KEY, prompt_id INTEGER NOT NULL, kind TEXT NOT NULL, status TEXT NOT NULL,
                        blocked INTEGER NOT NULL, body TEXT NOT NULL);  -- body: the Body as JSON
```

- A line with status `Unknown` has no row in `line_speakers`. A line with `Nobody` has one row with a null character. A line with more than one speaker has one row for each character.
- Position checks use row values: `(since_episode, since_ms) <= (?1, ?2)`.
- An approved proposal whose evidence line is not in `lines` (for example, the user changed the episode of the source to none) is a rebuild error with the proposal ID. The user revokes it or changes the source back.

### `context` (`src/context.rs`)

Contract:

```rust
pub struct PackageRequest {
    pub at: Position,
    pub characters: Vec<CharacterId>,
    pub place: Option<TermId>,
}

pub fn build(conn: &rusqlite::Connection, request: &PackageRequest) -> anyhow::Result<String>; // Markdown
```

Approach. Only valid items (see Position and time) go in. The sections, in this order:

1. `## Story so far`: the `Summary` items of earlier episodes. Then the summaries of the scenes in the current episode that end before `at`. A scene that is still in progress at `at` does not go in.
2. `## Characters`: for each selected character, its traits, speech style, appearance, status, and knowledge.
3. `## Relationships`: `Relationship` and `AddressTerm` items where the subject and the object are both selected.
4. `## Place`: when a place is selected, its `Description` and `Appearance` items.
5. `## Terms`: all terms with `since <= at`, with their `Description` items.
6. `## Example lines`: for each selected character, up to 10 lines before `at` that it speaks, with 8 or more characters, spread at equal time steps.

Each entry ends with its evidence, for example `[E3 12:34]`. An `Interpretation` entry starts with `(interpretation)`.

### `series` (`src/series.rs`)

The only module that the UI calls. Each write method writes its file and then runs `db::rebuild`.

```rust
#[derive(Clone)]
pub struct Series { dir: PathBuf }

impl Series {
    pub fn create(dir: &Path) -> anyhow::Result<Series>;  // dir must be missing or empty
    pub fn open(dir: &Path) -> anyhow::Result<Series>;    // checks the layout; db::open
    pub fn name(&self) -> &str;
    pub fn dir(&self) -> &Path;

    pub fn detect_files(&self, paths: &[PathBuf]) -> anyhow::Result<Vec<SourceRecord>>;
    pub fn save_sources(&self, records: &[SourceRecord]) -> anyhow::Result<String>; // "saved 3 sources"
    pub fn sources(&self) -> anyhow::Result<Vec<SourceRecord>>;

    pub fn proposals(&self, filter: &ProposalFilter) -> anyhow::Result<Vec<ProposalRow>>;
    pub fn decide(&self, ids: &[ProposalId], action: Action) -> anyhow::Result<String>; // "approved 40, skipped 3 (blocked)"

    pub fn progress(&self) -> anyhow::Result<Vec<EpisodeProgress>>;
    pub fn new_prompt(&self, kind: PromptKind, episode: u32) -> anyhow::Result<Prompt>;
    pub fn submit_answer(&self, prompt: PromptId, text: &str) -> anyhow::Result<String>; // "answer 12: 48 proposals"

    pub fn characters(&self, at: Position) -> anyhow::Result<Vec<Character>>;
    pub fn places(&self, at: Position) -> anyhow::Result<Vec<Term>>;
    pub fn build_package(&self, request: &PackageRequest) -> anyhow::Result<String>;
}

pub struct ProposalFilter { pub status: Option<Status>, pub kind: Option<ProposalKind> }

pub struct ProposalRow {
    pub id: ProposalId,
    pub kind: ProposalKind,
    pub content: String,        // for example "label \"あんな\" -> 明智あんな"
    pub evidence: Vec<String>,  // for example "E1 00:10 わたし 持つ！"
    pub status: Status,
    pub blocked: bool,
}

pub struct EpisodeProgress {
    pub episode: u32,
    pub has_subtitle: bool,        // has a Ja subtitle
    pub unknown_lines: Option<u32>, // None when there is no Ja subtitle
    pub items: u32,                // approved items with since_episode = episode
}
```

Approach:

- `decide` with `Approve` handles the IDs in the given order. It approves a proposal that is `Pending` and not blocked, after the approvals before it in the same call. It skips the others and counts them. `Reject` works on `Pending` proposals. `Revoke` works on `Approved` and `Rejected` proposals. It appends all decisions in one call to `append_decisions`.
- `proposals` returns the rows sorted by `ProposalId` (see IDs). A new character or term always comes before the entries in the same answer that use it, because `parse_answer` numbers it first.
- `new_prompt` reads a `PromptInput` from the database, calls `prompt::make`, and writes `prompts/<id>.json`. An episode without a `Ja` subtitle is an error.
- `submit_answer` reads the prompt, fails with `prompt {id} already has an answer` (a plain `anyhow` error) when `answers/<id>.json` exists, runs `prompt::parse_answer`, writes `answers/<id>.json` only when the answer is valid, and rebuilds. `AnswerError` goes up inside the `anyhow::Error`.
- An episode is done for speakers when `unknown_lines` is `Some(0)`.

### `ui` (`src/ui.rs`, `src/main.rs`)

Standard egui widgets only: `Button`, `Label`, `ComboBox`, `Checkbox`, `DragValue`, `Grid`, `TextEdit`, `ScrollArea`. No custom theme, color, icon, font, animation, modal, or keyboard shortcut. The app does not save state between runs.

```
[Open series] [New series]                          title: anime2ctxdb - <series path>
[Sources] [Proposals (23)] [Prompts] [Package]
Error: ...                                                                   [Clear]
```

- Sources: `[Import files]` opens an `rfd` file dialog and runs `detect_files`. The detected rows appear under `New files` with a `ComboBox` for the episode (`-`, 1 to 200) and the language. `[Save]` runs `save_sources`. The saved sources appear below in a `Grid` (`id` with 8 characters, `kind`, `path`, `episode`, `language`). Each row has the same editors and its own `[Save]`.
- Proposals: `ComboBox` for status (default `pending`) and kind (default `all`). `[Approve all shown]`. A `Grid` with `id`, `kind`, `content`, `evidence`, and buttons: `[Approve] [Reject]` for pending (`[Approve]` is disabled and the row shows `blocked` when it is blocked), `[Revoke]` for decided. The scroll position stays the same after a decision.
- Prompts: a `Grid` of `EpisodeProgress` (`episode`, `subtitle`, `speakers` as `done`, `N lines unknown`, or `-`, `items`). An episode `ComboBox` that starts at the first episode that is not done. `[New Speakers prompt]` and `[New Items prompt]`. The prompt text in a read-only `TextEdit` with `[Copy]`. An answer `TextEdit` with `[Submit]`. On success, the answer box becomes empty and the status line shows the message. On failure, the answer box keeps its text.
- Package: an episode `ComboBox`, a time `TextEdit` (`mm:ss`; an invalid value is an error), a place `ComboBox` (`none` and `series.places(at)`), a `Checkbox` for each of `series.characters(at)`, `[Generate]`, `[Copy]`, and the result in a read-only `TextEdit`.
- `main.rs` has `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`, takes the lock, and runs `eframe::run_native`.

## Seams

There is no code yet, so there is no prior art.

1. `Series` (integration tests in `tests/`): create a series in a `tempfile::TempDir`, then call the public methods as the UI does. The AI answers are JSON fixture files in `tests/fixtures/`. This seam covers `source`, `speaker`, `proposal`, `prompt`, `db`, and `context`. Use `testdata/raw.ass` as the subtitle. Copy it to the temp directory under a name with an episode number.
2. `ass::parse` (unit tests in `src/ass.rs`): ruby, tags, `⚟⚞`, labels, notes in parentheses, and the merge of rows with the same time. Use small ASS strings in the tests, and one test that parses `testdata/raw.ass` and `testdata/chinese.ass` without an error.

The UI has no tests.

## Docs

- `ARCHITECTURE.md`:
  - Mental model: the library (`src/`, Rust) owns all logic and the files of a series. The UI (`src/ui.rs`, `src/main.rs`, egui) calls only `Series`. SQLite (`index.sqlite`) is derived and holds no truth. Data flow: source files and answers → files of the series → rebuild → SQLite → UI views and the context package. Prompts flow out as text that the user copies.
  - Code map: `src/` (the library and the UI), `src/schema.sql` is a file so name the directory only, `prompts/` (prompt templates), `tests/` (integration tests through `Series`), `testdata/` (sample subtitle files).
  - Invariants, add: The app never changes a file that the user imported. A context package holds nothing from after its position. The `since` of derived data comes from its latest evidence line. The database has no migrations; a different schema version means a full rebuild. The UI calls only `Series`.
- `docs/requirements.md`: change "The user never enters data by hand" to "The user never enters story data by hand. The user can correct the episode and the language of an imported file." Change "The user carries job files to an external AI tool and brings the responses back" to "The user copies prompts to an external AI tool and pastes the answers back."
- `ARCHITECTURE.md` invariants: change the job file and response file words to prompt and answer in the same way.

## Out of scope

- Audio and video analysis. Still frames for appearance (a later `Evidence::Frame`; now evidence is only lines).
- The rule-based check of a draft for contradictions.
- Subtitles inside MKV files, `ffprobe`, `mpv`, and the time match between video and subtitle.
- The display of a translated subtitle next to the original.
- JSON output of the context package. Saving the package to a file.
- Special episodes (`SP1`, `13.5`).
- A limit on the size of the Items prompt when a series has many items.
- Keyboard shortcuts, themes, and saved UI state.
