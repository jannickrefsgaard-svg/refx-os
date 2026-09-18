# REFX Coding Standards

These complement §36 (Development Rules) of the Master Specification.

## Rust

- Edition 2021, MSRV 1.85. `cargo fmt` and `cargo clippy -D warnings` must pass.
- `unsafe` is forbidden workspace-wide. Lifting this for a crate (e.g. Windows
  FFI in Phase 2) requires an ADR and `// SAFETY:` comments on every block.
- No `unwrap()` in non-test code (lint: `clippy::unwrap_used`). Use `?`,
  `expect("why this cannot fail")`, or handle the error. In tests prefer
  `expect("…")` for readable failures.
- Errors: `thiserror` enums in libraries; `Box<dyn Error>` only at the binary edge.
- Poisoned mutexes are recovered (`unwrap_or_else(|p| p.into_inner())`) only
  where the protected data cannot be left logically inconsistent; say so in a comment.
- Never hold a lock while publishing events or calling foreign code.

## Architecture

- Components talk through events and traits, not concrete types from other subsystems (§2.6, §6).
- `refx-core` must not depend on a model provider, UI, or host OS.
- New dependencies need a one-line justification in the PR. Prefer std.
- Libraries never initialize global state (logger, allocator, panic hook).

## Observability

- Every state change of a task, service, agent or model is logged **and** published as an event.
- Log with structured fields (`task = %id`), not formatted strings.

## Testing

- Unit tests live next to the code (`#[cfg(test)]`); cross-module behavior
  goes in `crates/<crate>/tests/`.
- Every bug fix gets a regression test.
- Test error paths, not just the happy path.
- A feature is not done because it compiles (§38, §40).

## Decisions

Non-obvious design decisions are recorded in `docs/decisions/NNNN-title.md`.
Changes to the specified architecture use the Architecture Change Request
format in §37 and wait for approval.
