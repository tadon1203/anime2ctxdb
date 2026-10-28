# anime2ctxdb

anime2ctxdb turns an anime into a context database for AI writing. It reads subtitles, asks an external AI to find the speakers and the story, and keeps only what you approve. Each fact points to the lines that show it. See [docs/requirements.md](docs/requirements.md) for what the product does.

## Build

```
cargo build --release
```

This makes two programs: `anime2ctxdb-gui` (the app) and `anime2ctxdb` (the command line).

## Use the app

1. Run `cargo run --release --bin anime2ctxdb-gui`.
2. Make a series. In the Sources tab, import subtitle files or folders.
3. In the Prompts tab, select episodes. The prompt shows on the right, with a Speakers tab and a Story tab. Copy it to an AI chat, paste the answer below it, and submit.
4. In the Suggestions tab, approve or reject the suggestions.
5. In the Package tab, make the context package for a point in the story.

The app does not save prompts. It makes a prompt again from the series each time.

## Let Claude Code answer the prompts

Claude Code can do step 3 for you. You still approve the suggestions.

1. Install the command line once: `cargo install --path . --bin anime2ctxdb`
2. Start Claude Code.
3. Paste the text of [docs/agents/answer-prompts.md](docs/agents/answer-prompts.md), and tell the series name.

Claude Code answers each pending episode and saves the answers. You can edit the instruction file to change how it works.

## Command line

All commands start with `anime2ctxdb --series <name>`. Add `--series-root <folder>` if the series are not in the app data folder.

| Command                                                      | What it does                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------- |
| `status`                                                     | Lists the episodes with a Ja subtitle and what has an answer. |
| `prompt show --kind speakers\|story --episodes 1,2`          | Prints a prompt. It does not save it.                         |
| `answer submit --kind speakers\|story --episodes 1,2 [FILE]` | Saves an answer. It reads stdin if there is no file.          |

An episode can have one answer of each kind.

## Develop

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

See [ARCHITECTURE.md](ARCHITECTURE.md) and [docs/coding-standards.md](docs/coding-standards.md).
