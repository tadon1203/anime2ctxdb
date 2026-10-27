# Coding standards

General best practice for Rust takes precedence over this document. If a rule here conflicts with standard community practice, change the rule.

Do not write a rule here if a linter or a formatter enforces it. The tool configuration is the source of truth for those rules.

## Documentation and general rules

- Write comments and documentation in English. Follow ASD-STE100 (Simplified Technical English).
- Documentation is the single source of truth. Keep each fact in one file. Link to it. Do not copy it.
- Lock the dependency versions with the lock file.

## Design principles

- Keep the code simple and plain. Do not add a speculative abstraction. Do not add a layer that only forwards calls. Do not add a trait that has one implementation.
- Add a seam only where behavior varies or at an I/O boundary.
- Give each piece of state one owner. Calculate a derived value when it is needed. Do not store it and synchronize it by hand.
- Validate data where it enters the program, such as I/O and user input. Trust validated data inside the program.
- Do not write a fallback that hides a bug. Do not swallow an error.

## Naming

Names are boring, plain, and conventional. Do not use an ornate or enterprise-style name.

- Follow the [Rust API Guidelines, Naming](https://rust-lang.github.io/api-guidelines/naming.html).
- Name a thing for what it is, not for how clever the name sounds. Use a concrete noun for a type or a value. Use an active verb for a function.
- Do not use a metaphor or project jargon when a standard, boring term exists.
- Abbreviate only a term that everyone knows. Write every other word in full.

## Errors and panics

- A failure that can happen in a normal environment is a `Result`. Examples: a missing file, a lost network or device, invalid input.
- A bug panics. Examples: a broken internal invariant, an impossible condition. Do not write a defensive check that creates dead code.
- Validate at system boundaries. Pass validated values with strong types.
- Keep the cause of an error. Use `?`, `source`, or an error context library. Do not discard the context.

## Rust

- Standard idioms take precedence. Use plain functions and standard structs. Do not build a complex generic hierarchy.
- Use `anyhow::Result` where the application only passes an error up. Use a `thiserror` enum at a domain boundary where a caller matches specific variants.
- Handle every `Result`. Log the error, or pass it up. Do not write `let _ = ...` unless you drop the result on purpose, such as a send to a closed channel.

## Testing

- Test through the public interface. Do not test internal or private details.
- Replace a dependency only at a real boundary, such as an OS or hardware interface, a timer, or external I/O. Do not mock an internal helper.
- Keep unit tests close to the code that they test. Keep integration tests on behavior that a user can see.
