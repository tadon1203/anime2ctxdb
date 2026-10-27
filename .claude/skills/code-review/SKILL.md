---
name: code-review
description: "Review the changes from a base to the working tree along two axes: Standards (does the code follow this repo's documented coding standards, and are the docs still true after the change?) and Spec (does the code match the given ticket or spec?). Runs both reviews in parallel sub-agents and reports them side by side. Use when the user wants to review uncommitted changes, a branch, or asks to \"review since X\"."
---

Two-axis review of the changes from a base to the working tree:

- **Standards**: does the code conform to this repo's documented coding standards, and are the docs still true after the change?
- **Spec**: does the code faithfully implement the given ticket or spec?

Both axes run as **parallel sub-agents** so they don't pollute each other's context, then this skill aggregates their findings.

## Process

### 1. Pin the diff

The arguments are an optional base (default `HEAD`) and an optional ticket or spec path.

The diff is `git diff $(git merge-base <base> HEAD)` plus the untracked files (`git ls-files --others --exclude-standard`). With the default base, this is the uncommitted work. With `main`, it is the branch and the uncommitted work.

If the base does not resolve or the diff is empty, stop here.

### 2. Identify the spec source

The spec source is the path in the arguments. If there is none, skip the **Spec** sub-agent and report "no spec available".

### 3. Identify the standards sources

Anything in the repo that documents how code should be written, such as `CODING_STANDARDS.md`, `CONTRIBUTING.md`, or `ARCHITECTURE.md`. Start from the entry documents (the README, the agent instructions) and follow their links: the standards often live in files that those name. Find the lint and formatter configuration too, because it shows what tooling already enforces.

On top of whatever the repo documents, the Standards axis always carries the **smell baseline** below: a fixed set of code smells, most of them from Fowler (_Refactoring_, ch.3), that applies even when a repo documents nothing. A smell is on the list only if a reviewer can point at it in a hunk and two reviewers would agree it is there; they may still disagree on whether to fix it. Three rules bind it:

- **The repo overrides.** A documented repo standard always wins; where it endorses something the baseline would flag, suppress the smell.
- **Always a judgement call.** Each smell is a labelled heuristic ("possible Feature Envy"), never a hard violation. Like any standard here, skip anything tooling already enforces.
- **Distance sets the weight.** The further apart the coupled pieces sit (same function, same file, across modules), the more the smell matters.

Each smell reads *what it is* → *how to fix*; match it against the diff:

- **Mysterious Name**: a function, variable, or type whose name doesn't reveal what it does or holds. → rename it; if no honest name comes, the design's murky.
- **Duplicated Code**: the same logic shape appears in more than one hunk or file in the change. → extract the shared shape, call it from both.
- **Feature Envy**: a method that reaches into another object's data more than its own. → move the method onto the data it envies.
- **Data Clumps**: the same few fields or params keep travelling together (a type wanting to be born). → bundle them into one type, pass that.
- **Primitive Obsession**: a primitive or string standing in for a domain concept that deserves its own type. → give the concept its own small type.
- **Repeated Switches**: the same `switch`/`if`-cascade on the same type recurs across the change. → replace with polymorphism, or one map both sites share.
- **Shotgun Surgery**: one logical change forces scattered edits across many files in the diff. → gather what changes together into one module.
- **Divergent Change**: one file or module is edited for several unrelated reasons. → split so each module changes for one reason.
- **Speculative Generality**: abstraction, parameters, or hooks added for needs the spec doesn't have. → delete it; inline back until a real need shows.
- **Message Chains**: long `a.b().c().d()` navigation the caller shouldn't depend on. → hide the walk behind one method on the first object.
- **Middle Man**: a class or function that mostly just delegates onward. → cut it, call the real target direct.
- **Refused Bequest**: a subclass or implementer that ignores or overrides most of what it inherits. → drop the inheritance, use composition.
- **Global Data**: data that any code can read and write: a global variable, a mutable static, a singleton's mutable field. → wrap it in one module, reach it only through that module's functions.
- **Temporary Field**: a field that holds a value only in some circumstances and is empty the rest of the time. → move it and the code that uses it into its own type, or make it a local.
- **Comment Repeats Code**: a comment that says only what the code next to it already says. → delete it, or say why instead.
- **Shotgun Parsing**: the same input is validated in several places, and its type is no narrower after each check. → parse once at the edge into a type that carries the proof.
- **Representable Illegal State**: a type that can hold a combination of values that must never occur together; name one such combination, or it isn't this smell. → reshape it into one variant per state.

### 4. Spawn both sub-agents in parallel

**Standards sub-agent prompt** should include:

- The diff commands.
- The list of standards-source files you found in step 3, **plus the smell baseline from step 3** pasted in full (the sub-agent has no other access to it).
- The lint and formatter configuration files you found in step 3.
- The brief: "Report, per file/hunk where relevant, (a) every place the diff violates a documented standard: cite the standard (file + the rule); (b) any baseline smell you spot: name it and quote the hunk; and (c) every statement in the repo's docs that the diff makes false: quote the statement. Distinguish hard violations from judgement calls: documented-standard breaches can be hard, but baseline smells are always judgement calls, and a documented repo standard overrides the baseline. Skip anything tooling enforces; the configuration files show what that is. Before you report, check each finding again against the code and the text of the rule, and drop what does not hold: one wrong finding makes the reader distrust the right ones. Report every (a) and (c) finding. Keep (b) under 400 words."

**Spec sub-agent prompt** should include:

- The diff commands.
- The spec path.
- The brief: "Report: (a) requirements the spec asked for that are missing or partial; (b) behaviour in the diff that wasn't asked for (scope creep); (c) requirements that look implemented but where the implementation looks wrong; (d) places where the diff departs from the spec's Contracts or How. Quote the spec line for each finding. Under 400 words."

### 5. Aggregate

Present the two reports under `## Standards` and `## Spec` headings, verbatim or lightly cleaned. Do **not** merge or rerank findings, because the two axes are deliberately separate (see _Why two axes_).

End with a one-line summary: total findings per axis, and the worst issue _within each axis_ (if any). Don't pick a single winner across axes: that's the reranking the separation exists to prevent.

## Why two axes

A change can pass one axis and fail the other:

- Code that follows every standard but implements the wrong thing → **Standards pass, Spec fail.**
- Code that does exactly what the issue asked but breaks the project's conventions → **Spec pass, Standards fail.**

Reporting them separately stops one axis from masking the other.
