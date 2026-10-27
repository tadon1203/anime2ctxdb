---
name: to-spec
description: "Write the decisions of the current design session into a spec for /to-tickets."
disable-model-invocation: true
---

Run this in the same session as the design discussion. Read `docs/agents/tickets.md` and `docs/agents/implementation-ready.md`.

1. Explore the code as necessary.
2. Find the seams to test at. Prefer existing seams and the highest seam possible. The fewer seams, the better. Confirm them with the user.
3. Run /codebase-design. Apply its tests to each module that the design adds or changes, before its What is fixed. If an existing module that the change touches fails a test, put the fix in the Decisions. If the interface of a new module is still open, offer Design It Twice to the user. Confirm the results with the user.
4. Make sure that the What and the How of each change are known. A ticket must be able to meet the standard in `docs/agents/implementation-ready.md` from the spec alone: each change needs its What (public types, signatures, messages that cross a process or thread boundary), its How (the approach, with the mechanism and where the state lives), and its Where (the files, as a guide). Ask the user about each open decision.
5. Write `.scratch/<feature-slug>/spec.md`:

<spec-template>

## Problem

The problem, from the user's perspective.

## Solution

The solution, from the user's perspective.

## Decisions

Decisions that apply to more than one change, then one subsection per module with its Contracts and Approach. Name the path of each new file or module. For a decision whose solution was not obvious, give each rejected alternative and its reason in one line.

## Seams

The seams to test at, with prior art in the codebase.

## Docs

The changes to the durable documents that `CLAUDE.md` lists.

## Out of scope

</spec-template>
