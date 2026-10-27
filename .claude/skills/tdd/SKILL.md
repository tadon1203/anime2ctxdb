---
name: tdd
description: Test-driven development. Use when the user wants to build features or fix bugs test-first, mentions "red-green-refactor", or wants integration tests.
---

# Test-Driven Development

TDD is the red → green loop. This skill is the reference that makes that loop produce tests worth keeping: what a good test is, where tests go, how to replace a dependency, the anti-patterns, and the rules of the loop. Every section applies on every cycle: consult them before and during the loop, not after.

[The coding standards](../../../docs/coding-standards.md#tests) state the test rules of this project.

## What a good test is

Tests verify behavior through public interfaces, not implementation details. Code can change entirely; tests shouldn't. A good test reads like a specification: "user can checkout with valid cart" tells you exactly what capability exists, and it survives refactors because it doesn't care about internal structure.

- Tests behavior that a caller cares about.
- Uses the public API only.
- Describes WHAT, not HOW. The name says what the caller gets, not which internal call makes it happen.
- Survives internal refactors.
- Makes one logical assertion per test.

```typescript
// GOOD: observes the result through the interface
test("user can checkout with valid cart", async () => {
  const cart = createCart();
  cart.add(product);
  const result = await checkout(cart, paymentMethod);
  expect(result.status).toBe("confirmed");
});

// BAD: asserts a call inside the implementation
test("checkout calls paymentService.process", async () => {
  await checkout(cart, payment);
  expect(mockPayment.process).toHaveBeenCalledWith(cart.total);
});
```

## Seams: where tests go

A **seam** is the public boundary you test at: the interface where you observe behavior without reaching inside. Tests live at seams, never against internals.

**Test only at the chosen seams.** Use the seams that the ticket gives. If it gives none, choose them yourself before you write a test. You can't test everything, so the seams put the testing effort on the critical paths and complex logic instead of every edge case.

**There are no test-only seams.** Do not add an interface, a parameter, an export, or a hook only so that a test can reach or replace something. A test uses the code as the app runs it. If a behavior cannot be tested through the interface, the interface is the problem: change its shape, or accept a gap and say so.

When the shape of that interface is itself in question (how deep the module is, where the seam belongs, what the interface should expose), call the Skill tool with "codebase-design" for the vocabulary. It is the shared source of the module, interface, depth, seam, adapter, leverage and locality terms, and it is a reference to consult, not a session to run.

## Replacing a dependency

Replace a dependency only at a real seam. See [mocking.md](mocking.md) for the list of real seams and how to write them.

## Anti-patterns

- **Implementation-coupled**: mocks internal collaborators, tests private methods, or verifies through a side channel (querying the database instead of using the interface). The tell: the test breaks when you refactor but behavior hasn't changed. Example of the side channel: `db.query("SELECT ...")` after `createUser`, where `getUser(id)` would show the same fact through the interface.
- **Tautological**: the assertion recomputes the expected value the way the code does (`expect(add(a, b)).toBe(a + b)`, a snapshot derived by hand the same way, a constant asserted equal to itself), so it passes by construction and can never disagree with the code. Expected values must come from an independent source of truth: a known-good literal, a worked example, the spec.

  ```typescript
  // BAD: the expected value repeats the implementation
  expect(calculateTotal(items)).toBe(items.reduce((sum, i) => sum + i.price, 0));

  // GOOD: an independent literal
  expect(calculateTotal([{ price: 10 }, { price: 5 }])).toBe(15);
  ```

- **Asserting on call counts or order** of internal calls. Assert on the outcome.
- **Horizontal slicing**: writing all tests first, then all implementation. Bulk tests verify _imagined_ behavior: you test the _shape_ of things rather than user-facing behavior, the tests go insensitive to real changes, and you commit to test structure before understanding the implementation. Work in **vertical slices** instead: one test → one implementation → repeat, each test a **tracer bullet** that responds to what the last cycle taught you.

## Designing for testability

A testable interface comes from its shape, not from test hooks:

1. **Return results, don't produce side effects.** A calculation returns its value. A caller decides what to do with it. `calculateDiscount(cart): Discount` is testable with no setup; `applyDiscount(cart): void` needs a cart whose state the test then inspects.
2. **Take what the caller already has.** A value that the caller passes in is easy to test. Do not create a collaborator inside a function when a parameter that the app already supplies would do the same work.
3. **Keep the surface small.** Fewer methods and parameters mean fewer tests needed and simpler setup.

## Rules of the loop

- **Red before green.** Write the failing test first, then only enough code to pass it. Don't anticipate future tests or add speculative features.
- **One slice at a time.** One seam, one test, one minimal implementation per cycle.
- **Refactoring is not part of the loop.** It belongs to the review stage (see the `code-review` skill), not the red → green implementation cycle.
