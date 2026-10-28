# Answer the pending prompts

This file is the instruction for Claude Code. It answers the pending prompts of one series.

## How to run

Build and install the CLI once:

```
cargo install --path . --bin anime2ctxdb
```

Run from the repository root. Add the series name at the end:

```
claude -p --bare --allowedTools "Bash(anime2ctxdb:*)" "$(cat docs/agents/answer-prompts.md)

Series name: <name>"
```

`--bare` does not load `CLAUDE.md` or skills. This file has everything the run needs.

## Instruction

You answer the prompts of one series in the anime2ctxdb project. The series name is in the last line of this message, as `Series name: <name>`.

Use the `anime2ctxdb` command. Each command starts with `anime2ctxdb --series <name>`.

1. Run `anime2ctxdb --series <name> prompt list --pending`. Each line has the prompt ID, the kind, the episodes, and the state, separated by tabs.
2. For each pending prompt, in ID order:
   1. Run `anime2ctxdb --series <name> prompt show <ID>`. The output is the full prompt. Follow its instructions.
   2. Write the answer in the format that the prompt asks for. Keep the JSON block.
   3. Submit the answer with a here-document:

      ```
      anime2ctxdb --series <name> answer submit <ID> <<'EOF'
      <the answer>
      EOF
      ```

   4. If the submit fails, read the error. Fix the answer and submit it again. After three failed tries, go to the next prompt.
3. When you finish, print two short lists: the IDs that were saved, and the IDs that failed with the last error.

Do not change any file in the repository. Do not change the series folder by hand. Use only the `anime2ctxdb` commands above.
