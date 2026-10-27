# Deepening

How to deepen a cluster of shallow modules safely, given its dependencies. Assumes the vocabulary in [SKILL.md](SKILL.md): **module**, **interface**, **seam**, **adapter**.

## Dependency categories

When assessing a candidate for deepening, classify its dependencies. The category determines how the deepened module is tested across its seam.

### 1. In-process

Pure computation, in-memory state, no I/O. Always deepenable: merge the modules and test through the new interface directly. No adapter needed.

### 2. Local-substitutable

Dependencies that have a real local substitute that the app also runs (PGLite for Postgres, in-memory filesystem). Deepenable if the substitute is real. The deepened module is tested with that dependency running in the test suite. No port at the module's external interface.

### 3. Remote but owned (Ports & Adapters)

Your own services across a network boundary (microservices, internal APIs). Define a **port** (interface) at the seam only when a second real adapter exists or is planned. The deep module owns the logic; the transport is injected as an **adapter**. The app runs the real transport in production and in its tests.

Recommendation shape: *"Define a port at the seam, implement an HTTP adapter for production and a queue adapter for the batch job, so the logic sits in one deep module even though it's deployed across a network."*

### 4. True external (Mock)

Third-party services (Stripe, Twilio, etc.) you don't control. The deepened module takes the external dependency as an injected port. Tests replace it only at the network edge, so the real client code still runs.

## Seam discipline

- **One adapter means a hypothetical seam. Two adapters means a real one.** Don't introduce a port unless at least two real adapters are justified. A test double does not count. A single-adapter seam is just indirection.
- **One external seam.** A deep module has one seam, at its interface. Do not add internal seams for tests.

## Testing strategy: replace, don't layer

- Old unit tests on shallow modules become waste once tests at the deepened module's interface exist; delete them.
- Write new tests at the deepened module's interface. The **interface is the test surface**.
- Tests assert on observable outcomes through the interface, not internal state.
- Tests should survive internal refactors, since they describe behaviour, not implementation. If a test has to change when the implementation changes, it's testing past the interface.
