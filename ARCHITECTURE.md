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

The layers are not decided yet.

## Code map

The directories are not decided yet.

## Invariants

- The app has no network code. It writes job files and reads response files. The user carries the files to an external tool.
- The source of truth is plain files. A database is an index that the app can build again from them.
- Source files never change. Derived data records its source and a version.
- Output from an external model is a proposal. It is not stored as fact until the user approves it.
- The user never enters data by hand. The user approves or rejects proposals.
- The design does not assume that a series is finished. A new episode can be added at any time.
