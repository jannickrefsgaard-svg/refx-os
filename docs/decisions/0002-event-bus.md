# ADR 0002 — Synchronous std-channel event bus for Phase 0

Status: Accepted (Phase 0) — expected to be revisited in Phase 1/4

## Context
§6 requires an event bus. No subsystem in Phase 0 needs async I/O, and §36.9
asks to avoid unnecessary dependencies.

## Decision
`EventBus` fans out cloned `EventEnvelope`s to per-subscriber
`std::sync::mpsc` channels. Sequence numbers are assigned under the
subscriber lock, so every subscriber sees events in `seq` order.
Dropped subscribers are pruned on the next publish.

## Consequences
- Zero extra dependencies; simple to test.
- Channels are unbounded: a subscriber that never drains grows memory.
  Acceptable while all subscribers are in-process and trusted.
- When model streaming / networking arrive (Phase 4) an async runtime
  (likely Tokio) will be introduced. The public `publish`/`subscribe` API is
  designed so that the backing implementation can change without touching
  publishers. Bounded channels and back-pressure are decided then.
