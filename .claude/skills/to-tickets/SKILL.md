---
name: to-tickets
description: Break a spec into numbered tracer-bullet tickets that each meet the implementation-ready standard.
disable-model-invocation: true
---

Read `docs/agents/tickets.md` and `docs/agents/implementation-ready.md`. The input is `.scratch/<feature-slug>/spec.md`, or the path the user gives.

## Process

1. Read the spec. Explore the code that it touches. Look for a prefactoring that makes the change easy.
2. Draft the tickets:
   - List the slices. A slice is the smallest change that goes through every layer that it needs and is verifiable by itself. A prefactoring is a slice.
   - Put the slices in one line, so that each slice builds only on the slices before it. A prefactoring comes first. Apply the dependency rule.
   - Cut the line of slices into tickets. The dependency rule and the order of work decide the cuts first. The size rule decides second. Number the tickets in the order of work.
   - Copy into each ticket the decisions, contracts, and seams from the spec that it needs. Put each change from the spec's Docs section into the ticket that implements that behavior.
   - If a ticket needs a What or a How that the spec does not give, it is an open decision. Write it down for step 3.
3. Show the user the numbered list: title, what it delivers, F, L, and the cost, with the files behind them, and the open decisions. Iterate until the user approves. Do not ask whether the granularity feels right. The dependency rule and the size rule decide it.
4. Write the tickets with the template in `docs/agents/tickets.md`. Keep the spec.

## Dependency rule

Dependencies go one way: a ticket uses what the lower numbers left working, and it needs nothing from a higher number. After each ticket, every check in `CLAUDE.md` passes.

- A ticket adds code only together with the production code that uses it. A type, function, field, parameter, or config key that only a later ticket uses belongs to that later ticket. A test is not a user.
- A ticket never holds code for a later ticket: no lint suppression, no placeholder body, no stub, no flag that is always off.
- If a change has no user of its own (a new module that nothing calls yet), it is not a slice. Join it with the slice that first uses it.

Bad: ticket 01 adds the `Foo` module and its tests, ticket 02 calls it. Ticket 01 fails the lint with unused code. Good: ticket 01 adds the part of `Foo` that its first caller needs, with that caller. Ticket 02 extends both.

## Size rule

The agent reads, builds, checks, and reviews one ticket in one context of about 220k tokens, without `/clear` or compaction.

The work budget is about 150k: 220k, minus about 40k for the session start, minus about 30k for the review and its fixes.

The cost of a slice is `3k × F + 12 × L`.

- F is the number of files that the slice creates, edits, deletes, or renames.
- L is the current line count of the files that it edits, deletes, or renames. An edit needs the whole file read.

Measure F and L with git and `wc`. Do not guess. About 30 files fit one ticket. A file that a scripted replace changes costs about 0.3k, because the script does not read it. A generated file costs the same. Use the scripted cost only for one identical change in many files, and say in the ticket that it is scripted.

The constants come from measured tickets. If an implemented ticket peaks over 250k, measure again and fix them.

The order and the dependencies come first. The size is second: it decides the cuts only where the dependency rule leaves a choice. Fill the tickets in order: add the next slice to the current ticket while it still fits, and start a new ticket only when the next slice does not fit. Slices that read the same files pack well together, because each file costs only once. Only the last ticket may be far under the budget.

The budget is a guide, not a wall. A ticket may go over the budget, up to the cost of a 250k context (a cost of about 180k, or about 40 files), when the dependencies ask for it: the next slice cannot be verified or used without the ones before it, or the cut at the budget would leave a small rest ticket. Never go over 250k.

If one slice alone is over 250k, split it where each part is still verifiable and still follows the dependency rule. If no such place exists, add a prefactoring ticket first.

A **wide refactor** (one mechanical change with call sites across the codebase) cannot be one vertical slice. Number it expand → migrate in batches → contract, so that each ticket leaves the old form working until the last one. The expand ticket also migrates the first batch, so that the new form has a user. The last batch also does the contract. Size each batch with the size rule.
