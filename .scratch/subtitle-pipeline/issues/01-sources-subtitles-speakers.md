# 01: Import sources, parse subtitles, and the speakers loop

## What to build

A desktop app (egui) that works on one `Series` directory.

1. The user imports subtitle and video files. The app detects the kind, the episode, and the language. The user corrects them in a table and saves. The app never changes the original files.
2. The app reads the `Ja` subtitle of each episode and finds the speakers that rules can find: the `SpeakerLabel`, the text color, and the previous line.
3. The user makes a Speakers `Prompt` for an episode, copies it to an AI chat, and pastes the `Answer` back. The answer becomes `Proposal`s (`Character`, `Label`, `Color`, `LineSpeaker`). The user approves or rejects each proposal. Approved data changes the speakers of the lines.
4. The Prompts tab shows, for each episode, if it has a subtitle and how many lines have an unknown speaker.

The UI is in English. It uses standard egui widgets only. The user can always see what to do next.

Docs in this ticket:
- `ARCHITECTURE.md`:
  - Mental model: the library (`src/`) owns all logic and the files of a series. The UI (`src/ui.rs`, `src/main.rs`, egui) calls only `Series`. SQLite (`index.sqlite`) is derived and holds no truth. Data flow: source files and answers → files of the series → rebuild → SQLite → UI views. Prompts flow out as text that the user copies.
  - Code map: `src/` (the library and the UI), `prompts/` (prompt templates), `tests/` (integration tests through `Series`), `testdata/` (sample subtitle files). Name `src/schema.sql` by its directory only.
  - Invariants, add: the app never changes a file that the user imported; the `since` of derived data comes from its latest evidence line; the database has no migrations (a different schema version means a full rebuild); the UI calls only `Series`.
  - Change the words "job file" and "response file" to "prompt" and "answer".
- `docs/requirements.md`: change "The user never enters data by hand" to "The user never enters story data by hand. The user can correct the episode and the language of an imported file." Change "The user carries job files to an external AI tool and brings the responses back" to "The user copies prompts to an external AI tool and pastes the answers back."

## How

- One package with `src/lib.rs` and `src/main.rs`. All logic is in the library. `main.rs` and `src/ui.rs` hold the egui code.
- Dependencies: `eframe`, `rfd`, `rusqlite` (feature `bundled`), `serde`, `serde_json`, `sha2`, `regex`, `anyhow`, `thiserror`, `fs4`, `dirs`. Dev dependency: `tempfile`. No `clap`, no `log`, no async runtime, no template engine, no ORM. Add no other crate.
- Use `anyhow::Result` everywhere, with `.with_context(...)` on each I/O call. The context names the path or the ID. `thiserror` enums only for `ass::AssError` and `prompt::AnswerError`. A bug panics.
- Words: use one word for one thing, in code and in the UI. `source` is an imported file. `evidence` is the lines that a proposal is based on.
- Series directory (the series name is the directory name):

  ```
  <series>/
    sources.jsonl      one record for each save of a source (append only)
    sources/<sha256>.ass
    prompts/<id>.json
    answers/<id>.json  the JSON part of each answer
    decisions.jsonl    append only
    index.sqlite       derived
  ```

  Video files stay where they are. `sources.jsonl` records their path, size, and SHA-256. The source of truth is all files except `index.sqlite`.
- Files: a JSONL file gets one line for each record. Write each line with one `write` call, then `sync_all`. Write a prompt or an answer file to a temporary file in the same directory, then rename it. An invalid JSONL line or JSON file fails with the file path and the line number. Do not skip it and do not repair it. In `sources.jsonl`, the last record for a source ID is the current one. In `decisions.jsonl`, the last decision for a proposal ID is the current one. A decision has no time field.
- IDs are newtypes around `String`, except `PromptId(u32)`.
  - `SourceId`: SHA-256 of the file, lowercase hex.
  - `LineId`: `<SourceId>:<n>`. `n` is the index of the first merged `Dialogue`, from 0.
  - `PromptId`: 1, 2, 3, ... The next ID is the largest ID in `prompts/` plus 1.
  - `ProposalId`: `<PromptId>-<n>`. `n` is the position in the answer, from 1. Sort by the two numbers, not as text.
  - `CharacterId`: the `ProposalId` of the proposal that made it.
  - In a prompt, short references: `L1` for lines, `C1` for characters. The answer uses `N1`, `N2`, ... for new characters. The prompt file holds the table from short reference to real ID.
- `since` of a character is the largest `(episode, end_ms)` of its evidence lines. Do not use the earliest line.
- Database: `index.sqlite` is fully derived. `PRAGMA user_version` holds `db::SCHEMA_VERSION`. When the file is missing or the version is different, build it again. Every write (save sources, decide, submit an answer) writes its file, then runs a full `db::rebuild`. A new prompt does not rebuild. The rebuild writes `index.sqlite.tmp`, then renames it to `index.sqlite`. All inserts run in one transaction.
- Rebuild order: read `sources.jsonl` → read `decisions.jsonl` → read each prompt and its answer, and run `prompt::parse_answer` again to get the proposals → `proposal::states` → insert sources → parse each `Ja` subtitle with an episode, and insert lines → insert approved characters → insert approved labels, colors, and line speakers → run `speaker::resolve` for each episode, and insert `line_speakers` → insert all proposals with status and blocked.
- A line with speaker `Unknown` has no row in `line_speakers`. A line with `Nobody` has one row with a null character. A line with more than one speaker has one row for each character.
- An approved proposal whose evidence line is not in `lines` is a rebuild error with the proposal ID.
- Threads: `Series` holds only its directory path and is `Clone + Send`. Each call opens its own SQLite connection. The UI runs each write and each slow call (`detect_files`, `save_sources`, `decide`, `submit_answer`, `new_prompt`) on a new `std::thread`. A UI view never keeps a connection open. At most one job runs at a time. The UI shows `Working...` and disables the buttons while a job runs. The worker sends one `anyhow::Result<Done>` on a `std::sync::mpsc` channel. When a job ends, the UI reads its views again on the UI thread with synchronous queries. A worker panic closes the channel and the UI thread panics too.
- One instance: `main` locks `<dirs::data_local_dir()>/anime2ctxdb/app.lock` with `fs4` `try_lock_exclusive` before it opens the window. When the lock fails, show `rfd::MessageDialog` with `anime2ctxdb is already running` and exit with code 1. Hold the lock until the process ends. `main.rs` has `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` and runs `eframe::run_native`.
- Errors in the UI: one line at the top, `Error: {:#}`, with `[Clear]`. The line stays until the user clicks `Clear`.
- `source::detect`: kind from the extension (`.ass` subtitle; `.mkv`, `.mp4`, `.webm`, `.avi` video; any other is an error). Hash the file in blocks. Episode from the file name, first pattern that matches: `第(\d+)話`, `[Ee][Pp]?(\d+)`, ` - (\d+)`, `\[(\d+)\]`, `#(\d+)`. No match gives `None`. Language from the subtitle text: any kana (U+3040 to U+30FF) gives `Ja`; else any CJK ideograph gives `Zh`; else any ASCII letter gives `En`; else `Other`. When the ID is already in `sources.jsonl`, return the current record with the new path.
- `source::save_records` validates first: an episode is 1 or more, a video has no language, an episode has at most one `Ja` subtitle, and the episode numbers of all records with an episode are 1, 2, 3, ... with no gap (count the saved records too). A gap is an error that names the missing number. Then it copies each new subtitle to `sources/<id>.ass` and appends the records.
- `ass::parse`: the caller reads UTF-8 and removes a BOM; text that is not UTF-8 is an `anyhow` error in the caller. Find field positions from the `Format:` line of `[Events]`. `Text` is last and can hold commas, so use `splitn`. Time `H:MM:SS.cc` becomes ms. ASS color is BGR; `parse` changes it to RGB (`\c&H0000FFFF` becomes `0xFFFF00`). A `Dialogue` with `\fscy` less than 100 in its first override block is ruby: drop it. Remove `{...}` blocks. Change `\N`, `\n`, `\h` to a space. Remove `⚟` and `⚞` and set `offscreen`. Trim. Drop a `Dialogue` whose text is then empty. When the text starts with `（...）`, the inner text is `speaker_label`; parentheses at other places stay in the text. Merge a `Dialogue` into the previous `Line` when start, end, and color are equal and it has no label; join with a space. Sort by start time; keep file order for equal starts.
- `speaker::resolve`, for each line of one episode in time order:
  1. An approved line speaker wins: `Characters(ids, Ai)`, or `Nobody` for an empty list.
  2. A label that is not a `Note`: split at `・`. Each part must be an approved label, else `Unknown`. `Character` parts give `Characters(ids, Label)`. When all parts are `Extra` or `Sound`, the result is `Nobody`.
  3. A color: an approved color gives `Characters([c], Color)`, else `Unknown`.
  4. No color and no label (or a `Note` label): find the nearest earlier line with no color. When it has `Characters` with method `Label` or `PreviousLine`, and `this.start_ms - that.end_ms <= 10_000`, the result is `Characters(same, PreviousLine)`. Else `Unknown`.
- `proposal::states`: the own status is the last decision (`approve` gives `Approved`, `reject` gives `Rejected`, `revoke` or none gives `Pending`). A proposal depends on each proposal that its references name. Its status is `Invalid` when a dependency is `Rejected` or `Invalid`. It is blocked when a dependency is `Pending`. The database holds a proposal as data only when it is `Approved` and not blocked. A decision line: `{"proposal":"12-4","action":"approve"}`.
- `prompt`: `make` and `parse_answer` are pure. `series` reads the input from the database. `prompt` does not use `db`. Load `prompts/speakers.md` with `include_str!` and fill `{{name}}` places with `str::replace`. `make` gives short references in order (`L1`, `C1`) and stores them in `refs`. A transcript line: `L12 01:01 #FFFF00 （あんなの母） あの子のリボンが 風でとばされたって [C2]`. The color is RGB hex, shown only when set. The speaker is `[C2]`, `[C1,C2]` for more than one, `[-]` for none, `[?]` for unknown. `parse_answer` takes the content of the first ` ```json ` block, or else the whole text, parses it with `serde_json`, and changes each short reference to a real ID. A new character `N1` becomes the `ProposalId` of its own proposal. Number the proposals: new characters first, then the other entries in answer order. An unknown reference gives `UnknownRef`. Style of the prompt: short, no role play, the goal, the input, and the JSON format. Three rules only: use only this transcript, give evidence line IDs (the smallest set that shows it), mark each item `fact` or `interpretation` (the Speakers prompt has no item, so only the first two apply). The instructions are in English. The values are in the language of the transcript.
- `Series::decide`: with `Approve`, handle the IDs in the given order. Approve a proposal that is `Pending` and not blocked, after the approvals before it in the same call. Skip the others and count them. `Reject` works on `Pending`. `Revoke` works on `Approved` and `Rejected`. Append all decisions in one call to `append_decisions`.
- `Series::proposals`: rows sorted by `ProposalId` as two numbers.
- `Series::new_prompt`: read a `PromptInput` from the database, call `prompt::make`, write `prompts/<id>.json`. An episode without a `Ja` subtitle is an error. The `labels` input holds labels with no approved decision; `colors` the same.
- `Series::submit_answer`: read the prompt. Fail with `prompt {id} already has an answer` (a plain `anyhow` error) when `answers/<id>.json` exists. Run `parse_answer`. Write `answers/<id>.json` only when the answer is valid, then rebuild. `AnswerError` goes up inside the `anyhow::Error`.
- An episode is done for speakers when `unknown_lines` is `Some(0)`.
- UI layout:

  ```
  [Open series] [New series]                          title: anime2ctxdb - <series path>
  [Sources] [Proposals (23)] [Prompts]
  Error: ...                                                                   [Clear]
  ```

  - Sources: `[Import files]` opens an `rfd` file dialog and runs `detect_files`. Detected rows appear under `New files` with a `ComboBox` for the episode (`-`, 1 to 200) and the language. `[Save]` runs `save_sources`. Saved sources appear below in a `Grid` (`id` with 8 characters, `kind`, `path`, `episode`, `language`). Each row has the same editors and its own `[Save]`.
  - Proposals: `ComboBox` for status (default `pending`) and kind (default `all`). `[Approve all shown]`. A `Grid` with `id`, `kind`, `content`, `evidence`, and buttons: `[Approve] [Reject]` for pending (`[Approve]` is disabled and the row shows `blocked` when blocked), `[Revoke]` for decided. The scroll position stays the same after a decision.
  - Prompts: a `Grid` of `EpisodeProgress` (`episode`, `subtitle`, `speakers` as `done`, `N lines unknown`, or `-`). An episode `ComboBox` that starts at the first episode that is not done. `[New Speakers prompt]`. The prompt text in a read-only `TextEdit` with `[Copy]`. An answer `TextEdit` with `[Submit]`. On success, the answer box becomes empty and the status line shows the message. On failure, the answer box keeps its text.
  - Standard egui widgets only: `Button`, `Label`, `ComboBox`, `Checkbox`, `DragValue`, `Grid`, `TextEdit`, `ScrollArea`. No custom theme, color, icon, font, animation, modal, or keyboard shortcut. The app does not save state between runs.
- Scope limit of this ticket: do not add `Term`, `Scene`, or `Item` types, the Items prompt, `EpisodeProgress.items`, `Position`-based queries (`characters(at)`, `places(at)`), or the Package tab. Ticket 02 adds them.

New files: `Cargo.toml`, `.gitignore` (`/target`), `src/lib.rs`, `src/main.rs`, `src/ui.rs`, `src/model.rs`, `src/source.rs`, `src/ass.rs`, `src/speaker.rs`, `src/proposal.rs`, `src/prompt.rs`, `src/db.rs`, `src/schema.sql`, `src/series.rs`, `prompts/speakers.md`, `tests/sources.rs`, `tests/speakers.rs`, `tests/fixtures/*.json`. Edit: `ARCHITECTURE.md`, `docs/requirements.md`.

## Contracts

```rust
// model.rs
pub struct SourceId(pub String);
pub struct LineId(pub String);
pub struct ProposalId(pub String);
pub struct CharacterId(pub String);
pub struct PromptId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position { pub episode: u32, pub time_ms: u32 }

pub enum SourceKind { Subtitle, Video }
pub enum Language { Ja, Zh, En, Other }
pub struct Character { pub id: CharacterId, pub name: String, pub reading: Option<String> }
pub enum LabelKind { Character, Extra, Sound, Note } // Extra: a person who is not a character, for example 女の子
pub enum SpeakerMethod { Label, Color, PreviousLine, Ai }
// All enums derive serde with rename_all = "snake_case". The database stores the same text.
// IDs: Display for each. Position: Ord by episode, then time.

// source.rs
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

// ass.rs
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

// speaker.rs
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

// proposal.rs (ticket 02 adds Term, Scene, and Item variants)
pub struct Proposal { pub id: ProposalId, pub body: Body }
pub enum Body {
    Character { name: String, reading: Option<String>, evidence: Vec<LineId> },
    Label { label: String, kind: LabelKind, character: Option<String> },
    Color { color: u32, character: String },
    LineSpeaker { line: LineId, characters: Vec<String> },  // empty: no speaker
}
// A String reference is a CharacterId, or a ProposalId of a new character in the same answer. These are the same values.
pub enum Action { Approve, Reject, Revoke }
pub struct Decision { pub proposal: ProposalId, pub action: Action }
pub enum Status { Pending, Approved, Rejected, Invalid }
pub struct State { pub status: Status, pub blocked: bool }
pub fn read_decisions(series_dir: &Path) -> anyhow::Result<Vec<Decision>>;
pub fn append_decisions(series_dir: &Path, decisions: &[Decision]) -> anyhow::Result<()>;
pub fn states(proposals: &[Proposal], decisions: &[Decision]) -> HashMap<ProposalId, State>;
impl Body { pub fn kind(&self) -> ProposalKind; }
pub enum ProposalKind { Character, Label, Color, LineSpeaker }

// prompt.rs (ticket 02 adds the Items kind)
pub enum PromptKind { Speakers }
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
    pub labels: Vec<String>,             // labels with no approved decision
    pub colors: Vec<u32>,                // colors with no approved decision
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

// db.rs
pub const SCHEMA_VERSION: i32 = 1;
pub fn open(series_dir: &Path) -> anyhow::Result<rusqlite::Connection>; // rebuilds when missing or old
pub fn rebuild(series_dir: &Path) -> anyhow::Result<()>;
// Plus the query functions that `series` uses. Each takes &Connection and returns plain structs. The implementer names them.

// series.rs
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
    pub has_subtitle: bool,         // has a Ja subtitle
    pub unknown_lines: Option<u32>, // None when there is no Ja subtitle
}

// ui.rs (private to the binary side)
enum Done {
    Detected(Vec<SourceRecord>),
    Prompt(Prompt),
    Status(String),
}
// the channel carries anyhow::Result<Done>
```

Speakers answer format:

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

Schema (`src/schema.sql`, loaded with `include_str!`). Ticket 02 adds the tables `terms`, `scenes`, `scene_characters`, `items`, `item_evidence`:

```sql
CREATE TABLE sources (id TEXT PRIMARY KEY, kind TEXT NOT NULL, path TEXT NOT NULL, size INTEGER NOT NULL,
                      episode INTEGER, language TEXT);
CREATE TABLE lines (id TEXT PRIMARY KEY, source_id TEXT NOT NULL REFERENCES sources(id), episode INTEGER NOT NULL,
                    start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, text TEXT NOT NULL,
                    color INTEGER, offscreen INTEGER NOT NULL, speaker_label TEXT);
CREATE TABLE characters (id TEXT PRIMARY KEY, name TEXT NOT NULL, reading TEXT,
                         since_episode INTEGER NOT NULL, since_ms INTEGER NOT NULL);
CREATE TABLE speaker_labels (label TEXT PRIMARY KEY, kind TEXT NOT NULL, character_id TEXT REFERENCES characters(id));
CREATE TABLE speaker_colors (color INTEGER PRIMARY KEY, character_id TEXT NOT NULL REFERENCES characters(id));
CREATE TABLE line_speakers (line_id TEXT NOT NULL REFERENCES lines(id), character_id TEXT REFERENCES characters(id),
                            method TEXT NOT NULL);  -- character_id NULL: no speaker
CREATE TABLE proposals (id TEXT PRIMARY KEY, prompt_id INTEGER NOT NULL, kind TEXT NOT NULL, status TEXT NOT NULL,
                        blocked INTEGER NOT NULL, body TEXT NOT NULL);  -- body: the Body as JSON
```

## Tests

- Seam 1, `Series` (integration tests in `tests/`): create a series in a `tempfile::TempDir`, call the public methods as the UI does. Use `testdata/raw.ass` as the subtitle; copy it to the temp directory under a name with an episode number. AI answers are JSON fixtures in `tests/fixtures/`.
  - `tests/sources.rs`: detect (kind, episode patterns, language), save and validation errors (two `Ja` subtitles for one episode, episode 0, a gap in the episode numbers, video with a language), a repeated import returns the current record, original files are not changed, a broken `sources.jsonl` line fails with path and line number, `index.sqlite` is rebuilt when it is deleted.
  - `tests/speakers.rs`: new Speakers prompt, submit an answer, `already has an answer` error, an invalid answer does not write a file, approve and reject, blocked and invalid proposals (a reject of a new character makes its label `Invalid`), `decide` counts, `progress` shows `unknown_lines` fall after approvals, resolve order (line speaker, label, color, previous line within 10 s).
- Seam 2, `ass::parse` (unit tests in `src/ass.rs`): ruby, tags, `⚟⚞`, labels, notes in parentheses, the merge of rows with the same time, BGR to RGB. One test parses `testdata/raw.ass` and `testdata/chinese.ass` without an error.
- Unit tests in `src/prompt.rs` for `parse_answer` (code block or plain JSON, `UnknownRef`, numbering order).
- The UI has no tests.

## Acceptance criteria

- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass.
- [ ] No lint suppression (`allow`) in the code.
- [ ] The app starts, locks `app.lock`, and a second instance shows `anime2ctxdb is already running` and exits with code 1.
- [ ] A user can create a series, import `testdata/raw.ass` and a video, correct the episode and language, and save. The original files are unchanged.
- [ ] The Prompts tab shows the progress grid and a Speakers prompt for an episode with a `Ja` subtitle. A pasted answer makes proposals. Approve, reject, and revoke change the speakers and `unknown_lines` after the rebuild.
- [ ] `index.sqlite` can be deleted and the app builds it again with the same data.
- [ ] `ARCHITECTURE.md` and `docs/requirements.md` have the changes in the Docs list above.

## Manual checks

- [ ] Run the app. Do the whole flow from the acceptance criteria with a real AI chat. The UI is clear about what to do next, and `Working...` shows while a job runs.
