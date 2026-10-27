# 02: Items, scenes, and the context package

## What to build

1. The user makes an Items `Prompt` for an episode. It has the transcript with speakers and the data that is already approved. The `Answer` becomes `Proposal`s for new characters, terms, scenes, and items. The user approves or rejects them in the Proposals tab.
2. The Prompts tab shows how many approved items start in each episode.
3. The user selects a `Position` (an episode and a time), characters, and a place. The app makes a `ContextPackage` in Markdown. The package holds only information that the story shows before that position.

Docs in this ticket: `ARCHITECTURE.md` invariants, add: a context package holds nothing from after its position.

## How

Ticket 01 is done. Use its code and its rules (IDs, files, rebuild, threads, errors, UI limits). Extend these files; do not rewrite them.

- `model.rs`: add `SourceId`-style newtypes `TermId`, `ItemId`, `SceneId` (value: the `ProposalId` of the proposal that made it), `Term`, `TermCategory`, `Item`, `ItemKind`, `Basis`.
- `proposal.rs`: add the variants `Term`, `Scene`, `Item` to `Body` and `ProposalKind`. A reference to a character, term, or item (`replaces`) makes a dependency, as in ticket 01.
- `prompt.rs`: add `PromptKind::Items`, `PromptInput.terms` and `PromptInput.items`, short references `T1` for terms and `I1` for items, and `prompts/items.md`. `parse_answer` for the Items kind: new characters, then new terms, then the other entries in answer order (scenes, items). A new entity `N1` becomes the `ProposalId` of its own proposal. An item with no evidence gives `MissingEvidence`.
- Position rules:
  - The `since` of an item, a term, a character, and a scene is the largest `(episode, end_ms)` of its evidence lines (for a scene: the end line). Do not use the earliest line.
  - An approved item B with `replaces: A` ends A. `A.until = B.since`. The rebuild calculates `until`. No file stores it.
  - An item is valid at position P when `since <= P` and (`until` is null or `until > P`). Use row values in SQL: `(since_episode, since_ms) <= (?1, ?2)`.
- `db.rs` and `schema.sql`: add the tables below. Extend the rebuild: insert approved terms next to characters; after labels, colors, and line speakers, insert approved scenes and items with `since` from the evidence; calculate `until`; insert `item_evidence`. `SCHEMA_VERSION` goes to 2. An approved proposal whose evidence line is not in `lines` is a rebuild error with the proposal ID.
- `Series`: `new_prompt` accepts `PromptKind::Items` (items valid at the start of the episode go in the input). `progress` fills `items` (approved items with `since_episode = episode`). `proposals` and `decide` handle the new kinds. Add `characters(at)`, `places(at)`, and `build_package`. `characters(at)` and `places(at)` return the entities with `since <= at`.
- `context::build` (`src/context.rs`). Only valid items go in. Sections, in this order:
  1. `## Story so far`: the `Summary` items of earlier episodes. Then the summaries of the scenes in the current episode that end before `at`. A scene that is still in progress at `at` does not go in.
  2. `## Characters`: for each selected character, its traits, speech style, appearance, status, and knowledge.
  3. `## Relationships`: `Relationship` and `AddressTerm` items where the subject and the object are both selected.
  4. `## Place`: when a place is selected, its `Description` and `Appearance` items.
  5. `## Terms`: all terms with `since <= at`, with their `Description` items.
  6. `## Example lines`: for each selected character, up to 10 lines before `at` that it speaks, with 8 or more characters, spread at equal time steps.

  Each entry ends with its evidence, for example `[E3 12:34]`. An `Interpretation` entry starts with `(interpretation)`.
- UI (`src/ui.rs`): add the `[New Items prompt]` button and the `items` column. Add the Package tab: an episode `ComboBox`, a time `TextEdit` (`mm:ss`; an invalid value is an error), a place `ComboBox` (`none` and `series.places(at)`), a `Checkbox` for each of `series.characters(at)`, `[Generate]`, `[Copy]`, and the result in a read-only `TextEdit`. Generate runs on the UI thread or a worker as ticket 01 rule says for slow calls; it holds no connection after it ends. Standard egui widgets only.
- Prompt style: as in ticket 01. All three rules apply (only this transcript; smallest evidence set; `fact` or `interpretation`).

New files: `src/context.rs`, `prompts/items.md`, `tests/context.rs`, `tests/fixtures/*.json`. Edit: `src/lib.rs`, `src/model.rs`, `src/proposal.rs`, `src/prompt.rs`, `src/db.rs`, `src/schema.sql`, `src/series.rs`, `src/ui.rs`, `tests/speakers.rs` (only if the changed schema needs it), `ARCHITECTURE.md`.

## Contracts

```rust
// model.rs
pub struct TermId(pub String);
pub struct ItemId(pub String);
pub struct SceneId(pub String);
pub struct Term { pub id: TermId, pub name: String, pub reading: Option<String>, pub category: TermCategory }
pub enum TermCategory { Place, Organization, Object, Technique, Other }
pub struct Item { pub id: ItemId, pub kind: ItemKind, pub subject: Option<String>, pub object: Option<String>,
                  pub text: String, pub basis: Basis, pub since: Position, pub until: Option<Position> }
pub enum ItemKind { Summary, Trait, SpeechStyle, Relationship, AddressTerm, Appearance, Knowledge, Status, Description }
pub enum Basis { Fact, Interpretation }

// proposal.rs: new variants
pub enum Body {
    // Character, Label, Color, LineSpeaker as in ticket 01, plus:
    Term { name: String, reading: Option<String>, category: TermCategory, evidence: Vec<LineId> },
    Scene { start: LineId, end: LineId, place: Option<String>, characters: Vec<String>, summary: String },
    Item { kind: ItemKind, subject: Option<String>, object: Option<String>, text: String,
           basis: Basis, evidence: Vec<LineId>, replaces: Option<String> },
}
// A String reference is a CharacterId, a TermId, an ItemId, or a ProposalId of a new
// character or term in the same answer. These are the same values.
pub enum ProposalKind { Character, Term, Label, Color, LineSpeaker, Scene, Item }

// prompt.rs
pub enum PromptKind { Speakers, Items }
pub struct PromptInput {
    pub lines: Vec<PromptLine>,
    pub characters: Vec<Character>,
    pub terms: Vec<Term>,
    pub items: Vec<Item>,                // Items prompt: items valid at the start of the episode
    pub labels: Vec<String>,
    pub colors: Vec<u32>,
}

// context.rs
pub struct PackageRequest {
    pub at: Position,
    pub characters: Vec<CharacterId>,
    pub place: Option<TermId>,
}
pub fn build(conn: &rusqlite::Connection, request: &PackageRequest) -> anyhow::Result<String>; // Markdown

// series.rs: new methods
impl Series {
    pub fn characters(&self, at: Position) -> anyhow::Result<Vec<Character>>;
    pub fn places(&self, at: Position) -> anyhow::Result<Vec<Term>>;
    pub fn build_package(&self, request: &PackageRequest) -> anyhow::Result<String>;
}
pub struct EpisodeProgress {
    pub episode: u32,
    pub has_subtitle: bool,
    pub unknown_lines: Option<u32>,
    pub items: u32,                // approved items with since_episode = episode
}
```

Items answer format:

```json
{
  "new_characters": [],
  "new_terms": [{ "ref": "N1", "name": "...", "reading": "...", "category": "place", "evidence": ["L3"] }],
  "scenes": [{ "start": "L1", "end": "L40", "place": "N1", "characters": ["C1", "C2"], "summary": "..." }],
  "items": [{ "kind": "trait", "subject": "C1", "object": null, "text": "...", "basis": "interpretation",
              "evidence": ["L12", "L15"], "replaces": "I40" }]
}
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

Schema additions (`src/schema.sql`). `Item.subject` is a character or a term, so the table has two subject columns:

```sql
CREATE TABLE terms (id TEXT PRIMARY KEY, name TEXT NOT NULL, reading TEXT, category TEXT NOT NULL,
                    since_episode INTEGER NOT NULL, since_ms INTEGER NOT NULL);
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
```

## Tests

- Seam 1, `Series` (integration tests in `tests/`), same setup as ticket 01. Items answers are JSON fixtures.
  - `tests/context.rs`:
    - An Items prompt contains the approved characters, terms, and items valid at the start of the episode.
    - Submit an Items answer. Approve in dependency order. A blocked or invalid proposal is not data.
    - `until`: an approved item that replaces another ends it at its own `since`; the old item is not valid after that position.
    - `since` comes from the latest evidence line, not the earliest.
    - `build_package` leaks nothing: a scene in progress at `at`, an item with `since > at`, and an item that is already replaced are all absent.
    - Each section, `(interpretation)` marking, evidence marks, and the example-lines rule (8 or more characters, up to 10, equal time steps).
    - `progress.items` counts approved items with `since_episode = episode`.
    - A rebuild error names the proposal ID when its evidence line is missing.
- Unit tests in `src/prompt.rs` for the Items answer (numbering order, `MissingEvidence`, `UnknownRef`).
- The UI has no tests.

## Acceptance criteria

- [ ] `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` pass.
- [ ] No lint suppression (`allow`) in the code.
- [ ] An Items prompt can be made for an episode with a `Ja` subtitle, and its answer makes proposals of the new kinds that the user can approve, reject, and revoke.
- [ ] The Package tab makes Markdown for an episode, a time, characters, and a place. The package holds nothing from after the position.
- [ ] `index.sqlite` can be deleted and the app builds it again with the same data (schema version 2).
- [ ] `ARCHITECTURE.md` has the new invariant.

## Manual checks

- [ ] Run the app with a real series. Make an Items prompt, paste a real answer, approve the proposals, and make a package at an early time and at a late time. The early package has no later story information.
