# ADR 0003 — Task state transitions and task IDs

Status: Accepted (Phase 0)

## Context
§7 lists the task states but not which transitions are legal.

## Decision
Transitions:

| From      | To |
|-----------|----|
| CREATED   | QUEUED |
| QUEUED    | PLANNING, EXECUTING (simple commands skip planning, §14) |
| PLANNING  | EXECUTING, WAITING, FAILED |
| EXECUTING | WAITING, PAUSED, FAILED, COMPLETED |
| WAITING   | PLANNING, EXECUTING, FAILED (waiting for user / permission) |
| PAUSED    | QUEUED, EXECUTING (e.g. resource manager pause, §28) |
| any non-terminal | CANCELLED |

FAILED, CANCELLED and COMPLETED are terminal. Retrying (§35) creates a new
task rather than resurrecting a failed one, so history stays truthful.
Every transition is recorded with timestamp and optional reason.

Task IDs are a per-runtime monotonic `u64` for now.

## Consequences
When tasks are persisted (SQLite), IDs must become globally unique
(e.g. UUIDv7). `TaskId` is a newtype so that change is local.
Retry-as-new-task will need a `retry_of: Option<TaskId>` link — to be added
with the recovery logic.
