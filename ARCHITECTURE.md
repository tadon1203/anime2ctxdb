# Architecture

This document is the map of the system. It also lists the rules that keep the system in shape.

## About this document

This document changes rarely. Keep it stable.

- Keep the sections fixed, as in the template below. Do not add, remove, or rename a section. Put new content in one of the existing sections.
- Do not put information here that changes often.
- Do not name a file in the Code map. Name a directory.
- Do not describe an implementation detail. The code and its comments describe it.
- Change this document only when the structure or a rule changes.

The template for this document. Replace each placeholder in `<>`. Keep all other text.

```markdown
# Architecture

<What this document is.>

## About this document

<The rules for changing this document.>

## Mental model

<The technology stack, by layer: its directory, its technology, what it owns, and what it must not hold.>

<How data flows through the layers: each path, its direction, and what it carries.>

## Code map

<Where each part lives, by directory.>

| Directory     | What it holds              |
| ------------- | -------------------------- |
| `<directory>` | <What it holds, in a line> |

## Invariants

- <A rule that must never break, so the design does not decay.>
```

## Mental model

The library owns the logic and the files of a series. The app calls only the library. The project is one Rust crate (edition 2024) with a library, a GUI binary, and a CLI binary. The GUI and the CLI use the same series folders in the app data folder.

- Files (`src/series/`, `src/sources/`, `src/answers/`; plain JSON, JSONL, and text files in a series directory): the source of truth. They own imported sources, parsed lines, answers, and the importance that the user sets by hand (`importance.json`). A prompt is not a file. The library makes it from the files when it is needed. They must not hold data that the app can derive from other files.
- Library (`src/`, without `bin/`; Rust, serde, regex): reads sources, parses subtitles, resolves speakers, builds the context package, and writes the files. It owns all logic and all file access. It must not depend on egui and must not have network code.
- Index (`src/series/`, SQLite through rusqlite): derived from the files. It owns fast queries for the views. It must not hold any truth. The app rebuilds it after each write that changes data.
- App (`src/bin/gui/`; egui through eframe, rfd for file dialogs): shows the views. It owns view state only. It calls only `Series`. It runs the slow write calls on worker threads. It must not read or write series files or the index directly.

Data flows in two directions:

- Source files → series files → rebuild → index → app views.
- Series → prompt text (made when needed) → user copies it to an external AI tool → user pastes the answer → answer file (checked, with real line IDs and entity names) → rebuild → index.

## Code map

The code is split by feature. A feature is a module with the same name as its directory. Only `Series` and a few plain types leave the library.

| Directory       | What it holds                                                                         |
| --------------- | ------------------------------------------------------------------------------------- |
| `src/`          | The library root and the context package. The root lists everything public.          |
| `src/bin/gui/`  | The GUI binary: the app state and one file for each tab.                              |
| `src/bin/cli/`  | The CLI binary: the commands for prompts and answers. It calls only `Series`.         |
| `src/series/`   | `Series`, the one entry point of the library, and the SQLite index.                   |
| `src/sources/`  | Import of subtitle files, and the subtitle parser.                                    |
| `src/speakers/` | Speaker rules.                                                                        |
| `src/answers/`  | The prompts for the external AI, and the answers: checked and stored.                 |
| `prompts/`      | The prompt templates that the library fills in.                                       |
| `tests/`        | Integration tests through `Series`.                                                   |
| `testdata/`     | Sample subtitle files.                                                                |

## Invariants

- The app has no network code. It writes prompts and reads answers. The user copies the prompts to an external tool and pastes the answers back.
- The source of truth is plain files. A database is an index that the app can build again from them.
- Source files never change, and the app never changes a file that the user imported. Derived data records its source and a version.
- The app appends to `sources.jsonl`. Only a source removal rewrites it, through a temporary file. A source that an answer uses cannot be removed, because the line IDs of the answer point to it.
- An entity is its name. An answer names an entity by its name, so the library can make the prompt again when the user submits the answer. A name that no answer introduces any more stays as text in a fact.
- An episode has one answer of each kind, in `answers/<kind>-<episode>.json`. A new answer replaces the old one whole. The prompt for an episode is made as if the episode had no answer of that kind.
- The app calls only `Series` and the plain types that the library root exports.
- The library checks the output of an external model when the user submits it, and stores it as data. There is no approval. A wrong answer is fixed by a new answer.
- The design does not assume that a series is finished. A new episode can be added at any time.
- Every claim from an external model has evidence, which is lines of a source.
- A context package holds nothing from after its position.
- The app does not edit story data by item. It replaces the answer of an episode whole. The one exception is the importance of an entity, which the user can set by hand.
- An importance is data about an entity, not a claim about the story, so it has no evidence. The latest answer up to an episode sets it, and a setting by hand wins over every answer.
- The app calculates `since` when it rebuilds. No file stores it.
