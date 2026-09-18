# REFX OS

AI-native computing environment for Windows. See the Master Specification v0.1
for vision and scope; this repository implements it phase by phase.

**Current status: Phase 0 — Foundation.** Nothing user-facing exists yet.

## Layout

```
crates/
  refx-core/      Event bus, task system, config, logging, service runtime (§5–7)
  refx-platform/  Platform interface; portable adapter (Windows adapter: Phase 2)
  refx-daemon/    `refxd`, the runtime host process
config/           Example configuration
docs/             Architecture, coding standards, decision records (ADRs)
```

## Build & test

Requires Rust 1.85+ (stable).

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p refx-daemon -- --smoke --config config/refx.example.toml
```

`refxd --smoke` starts the runtime, drives one task through its lifecycle and
shuts down; CI runs it on Windows and Linux.

## Configuration

Copy `config/refx.example.toml` to `refx.toml` (or pass `--config`).
`REFX_LOG` overrides the log level, e.g. `REFX_LOG=debug` or
`REFX_LOG=info,refx_core=trace`. Logs go to stderr.
