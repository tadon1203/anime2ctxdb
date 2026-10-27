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

The library owns the logic and the files of a series. The app calls only the library.

- Library (the modules that do not depend on egui): reads sources, parses subtitles, resolves speakers, and holds the files of a series. The files are the source of truth.
- Index (`index.sqlite`): derived from the files. It holds no truth. The app rebuilds it after each write that changes data.
- App (egui, the binary): shows the views. It calls only `Series`. It runs the slow write calls on worker threads.

Data flows in two directions:

- Source files → series files → rebuild → index → app views.
- Series → prompt text → user copies it to an external AI tool → user pastes the answer → answer file → suggestions → user decisions → rebuild → index.

## Code map

The code is split by feature. A feature is a module with the same name as its directory. Only `Series` and a few plain types leave the library.

| Directory        | What it holds                                                                      |
| ---------------- | ---------------------------------------------------------------------------------- |
| `src/`           | The app (the binary) and the library root. The root lists everything public.       |
| `src/series/`    | `Series`, the one entry point of the library, and the SQLite index.                |
| `src/sources/`   | Import of subtitle files, and the subtitle parser.                                 |
| `src/speakers/`  | Speaker rules, the prompts for the external AI, and the suggestions from its answers. |
| `prompts/`       | The prompt templates that the library fills in.                                    |
| `tests/`         | Integration tests through `Series`.                                                |
| `testdata/`      | Sample subtitle files.                                                             |

## Invariants

- The app has no network code. It writes prompts and reads answers. The user copies the prompts to an external tool and pastes the answers back.
- The source of truth is plain files. A database is an index that the app can build again from them.
- The app never changes a file that the user imported.
- Source files never change. Derived data records its source and a version.
- The `since` of derived data comes from its latest evidence line.
- The database has no migrations. A different schema version means a full rebuild.
- The app calls only `Series` and the plain types that the library root exports.
- Output from an external model is a suggestion. It is not stored as fact until the user approves it.
- The user never enters story data by hand. The user can correct the episode and the language of an imported file. The user approves or rejects suggestions.
- The design does not assume that a series is finished. A new episode can be added at any time.
