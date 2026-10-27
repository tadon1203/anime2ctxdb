# Replacing Dependencies

Replace a dependency only at a **real seam**: a boundary that the app itself runs across. A seam is real when the production code uses it the same way the test does.

This project has these real seams:

- **The file system and the clock.** Pass the directory path or the time as an argument. A test passes a temporary directory or a fixed time.
- **A trait with more than one real implementation**, such as two importers that write the same segment type. Write a test implementation of the trait only when the app also has a second real one.
- **The job files.** The app writes job files and reads response files. It never talks to a network. A test writes a response file by hand.

Everything else runs for real. Do not mock:

- Your own structs or modules.
- Internal collaborators, or anything the same module owns.
- SQLite. Use a real database in a temporary directory.

## Do not add a seam for a test

Do not add a trait, a parameter, an export, or a constructor argument only so that a test can replace something. A seam that only tests use is a second path through the code, and the app does not run it.

If a dependency has one real implementation and no real second one, do not put a trait in front of it. Pass the real object. If the dependency is an external tool, such as `ffmpeg`, the test must still use the real code path. Use a real local stand-in if one exists, or accept the gap and say so in the ticket.

## Do not patch internals

A test does not write to a private field or replace a function of the module under test. A module gets its dependencies as arguments, so a test replaces a dependency by passing the same kind of value that the app passes, at one of the seams above.
