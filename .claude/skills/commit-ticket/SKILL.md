---
name: commit-ticket
description: "Commit the implemented ticket and move the ticket file to issues/done/."
disable-model-invocation: true
---

Read `docs/agents/tickets.md` and `docs/git-workflow.md`. The ticket is the next ticket, or the path the user gives.

1. If the working tree has no changes, stop.
2. Move the ticket to `issues/done/`, as `docs/agents/tickets.md` says. Delete nothing.
3. Commit all changes. Put the ticket number and title in the message body.
