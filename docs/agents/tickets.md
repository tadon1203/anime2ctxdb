# Specs and tickets

A feature lives in `.scratch/<feature-slug>/` while it is in progress.

- `/to-spec` writes `spec.md`. The spec carries the decisions from the design session to `/to-tickets`. `/to-tickets` keeps it. No skill deletes it. Tickets do not link to it.
- `/to-tickets` writes one file per ticket: `issues/<NN>-<slug>.md`, numbered from `01`.
- The tickets are done in number order. A ticket depends only on tickets with lower numbers.
- The next ticket is the lowest-numbered file directly in `issues/`. Ignore `issues/done/`.
- `/commit-ticket` commits a finished ticket and moves the ticket file to `issues/done/`, with the same file name. Create `done/` when it is needed. No skill deletes a ticket, the spec, or the feature directory.

## Implementation-ready standard

The standard that every ticket meets is in [implementation-ready.md](implementation-ready.md).

## Ticket template

```md
# <NN>: <title>

## What to build

The behavior this ticket makes work. Use domain terms.

## How

The approach. Name the path of each new file or module. Do not list existing files.

## Contracts

## Tests

The seams to test at, and the test files.

## Acceptance criteria

- [ ] Criterion

## Manual checks

- [ ] A check that only the user can do. Omit this section if there is none.
```

The implementer does not do the manual checks and does not report them as passed. It lists them for the user when it stops.

When the implementer stops, it adds a `## Comments` section at the bottom and writes the problem there.
