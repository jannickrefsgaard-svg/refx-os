# ADR 0001 — Cargo workspace with separate core, platform and host crates

Status: Accepted (Phase 0)

## Context
§2.6 requires replaceable components; §21 requires Core → Platform Interface → Windows Adapter.

## Decision
One Cargo workspace. `refx-core` and `refx-platform` are independent library
crates; `refx-daemon` is the only crate that knows about both. Further
subsystems (models, agents, voice, UI shell) become their own crates.

## Consequences
The compiler enforces that Core cannot call the OS directly. Adding the Tauri
UI in Phase 1 means a new crate that depends on `refx-core`, not changes to it.
