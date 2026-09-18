# REFX Architecture (as implemented)

This document describes what exists in the code today and how it maps to the
Master Specification. It is updated with every phase. The specification
remains authoritative for *intended* architecture.

## Crate dependency graph

```
refx-daemon ──► refx-core
     │
     └────────► refx-platform
```

`refx-core` and `refx-platform` do not depend on each other. The host process
(`refx-daemon`) wires them together. This keeps Core free of OS assumptions
(§21, §41) and free of any AI model or provider (§5).

## refx-core

| Module    | Spec   | Responsibility |
|-----------|--------|----------------|
| `event`   | §6     | `EventBus`: fan-out publish/subscribe. Every event carries a sequence number, timestamp and source for traceability (§2.7). |
| `task`    | §7     | `Task` record, `TaskState` machine with validated transitions, `TaskManager` which publishes task events. |
| `config`  | —      | Typed TOML config. Precedence: env → file → defaults. Unknown keys rejected. |
| `logging` | —      | Installs a `tracing` subscriber (stderr). Only the host process calls it. |
| `runtime` | §5     | `Service` trait and `Runtime`: ordered start, reverse-order stop, rollback on failed start. |

### Service lifecycle

```
register(a), register(b), register(c)
start():    a.start → b.start → c.start → RuntimeStarted
shutdown(): RuntimeStopping → c.stop → b.stop → a.stop → RuntimeStopped
start() with b failing: a.start → b.start ✗ → a.stop → error (no half-started system)
```

A failing `stop` does not prevent the remaining services from stopping; all
stop failures are reported together.

### Task state machine (ADR 0003)

Happy path: `CREATED → QUEUED → PLANNING → EXECUTING → COMPLETED`.
Simple commands may go `QUEUED → EXECUTING` directly. `WAITING` (user input,
permission) and `PAUSED` (resource manager) branch off and return.
Any non-terminal state can go to `CANCELLED`. Full table in ADR 0003.

Terminal: `COMPLETED`, `FAILED`, `CANCELLED`. `COMPLETED` is reachable only
from `EXECUTING`, so a task cannot be marked done without having run (§35).

## refx-platform

`Platform` trait + `PortablePlatform` (std only: OS family, arch, CPU count).
The Windows adapter is Phase 2.

## Not yet implemented

Scheduler, resource manager, security integration, state persistence (SQLite)
and everything in Phases 1–10.
