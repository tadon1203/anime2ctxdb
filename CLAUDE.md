# CLAUDE.md

Respond and communicate with the user in the language they are using.

## Project

`anime2ctxdb` turns anime (subtitles first, audio and video later) into a point-in-time, sourced context database for AI writing. The series can be finished or still airing. Rust, plain files as the source of truth, SQLite as an index that can be built again.

## Documents

Read a document only when its trigger applies. Do not read them all up front.

| Document                   | What it is                                          | Read when                       |
| -------------------------- | --------------------------------------------------- | ------------------------------- |
| `docs/requirements.md`     | What the product does                               | A task changes or depends on product behavior |
| `ARCHITECTURE.md`          | System map and rules that must not break            | You change structure or add a module |
| `docs/coding-standards.md` | Code rules that lint cannot check, and naming rules | You write or review code        |
| `docs/git-workflow.md`     | Commit branch and commit message rules              | You commit or prepare a commit  |
| `docs/agents/tickets.md`   | Spec and ticket files and their template            | You use a spec or ticket skill  |

## Writing

- Write code comments and documents (`*.md`, doc comments, commit messages) in Simplified Technical English (ASD-STE100): short sentences, one meaning for each word, active voice, and simple verb tenses.

## Commands

There is no CI and there are no commit hooks. Run the checks before you commit.

- Format: `cargo fmt --check`
- Lint: `cargo clippy --all-targets -- -D warnings`
- Test: `cargo test`

Do not ignore lint results. Do not suppress them with `allow` unless the user asks for it.

## Workflow

- Feature workflow: `/grilling` (design) → `/to-spec` → `/to-tickets` → `/implement` → `/commit-ticket`, one ticket at a time. Skip the spec and tickets for a small feature.
- Run `/clear` after each step. Exception: run `/to-spec` in the same session as `/grilling`.
