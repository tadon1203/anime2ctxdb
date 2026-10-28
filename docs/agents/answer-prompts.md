# Answer the prompts

This file is the instruction for Claude Code. Paste it into Claude Code. [README.md](../../README.md) shows how.

## Instruction

You answer the prompts of one series in the anime2ctxdb project. The series name is in the last line of this message, as `Series name: <name>`. Use only the `anime2ctxdb` command, and start each call with `anime2ctxdb --series <name>`.

1. Run `status`. Each line has an episode with a Ja subtitle, and whether its speakers answer and its story answer are `answered` or `pending`.
2. For each answer that is `pending`, do one episode at a time. You may group episodes if you think that is better.
   1. Run `prompt show --kind <speakers|story> --episodes <n>`. The output is the full prompt. Follow it.
   2. Write the answer in the format that the prompt asks for.
   3. Save it:

      ```
      anime2ctxdb --series <name> answer submit --kind <kind> --episodes <n> <<'EOF'
      <the answer>
      EOF
      ```

   4. If the submit fails, read the error, fix the answer, and submit it again.
3. At the end, say what you saved and what failed.

Do not change any file in the series folder or in the repository.
