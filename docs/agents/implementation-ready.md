# Implementation-ready standard

A ticket is complete by itself. The implementer reads the ticket and the code, not the spec. The implementer can start without a design decision. The ticket gives three things:

- How (the most important): the approach in a few lines. For example, where the state lives and which mechanism does the work.
- What: the public types and signatures, and the messages that cross a process or thread boundary, as code. No function bodies. Public means used from outside the file that defines it.
- Where: the files to create, change, and delete, test files included. This is a guide, not a contract. A glob or "all callers of `X`" is correct.

A decision is open when the What or the How is not known. Open decisions go to the user, never to the implementer. The implementer decides all other things.
