# Assessing

How to find the modules in existing code that are worth deepening. Uses the vocabulary and the tests in [SKILL.md](SKILL.md).

## Scope

The user names a module or a directory. If they name none, let the git history choose: the files that change most often, and the pairs of files in different modules that keep changing together. Start there. A shallow module that nobody touches costs nothing.

## Process

### 1. Read

Read the modules in scope in full, with their callers. A test needs the whole **interface**, and much of an interface is visible only at the call sites.

### 2. Apply the tests

Apply each test in [SKILL.md](SKILL.md) to each module. For the change test, use all three sources of a change.

### 3. Present the candidates

List the modules that fail a test. For each one give:

- The module and the test that it fails.
- The evidence: the files, the callers, the counts from the history.
- The fix, in one line.

Put first the candidates in the areas that change most often. Then give your own recommendation: which candidate to take first, and why.

### 4. Stop

Do not change code. When the user picks a candidate, continue with [DEEPENING.md](DEEPENING.md), and with [DESIGN-IT-TWICE.md](DESIGN-IT-TWICE.md) if the new interface is open.
