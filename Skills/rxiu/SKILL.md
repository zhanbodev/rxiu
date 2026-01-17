---
name: rxiu
description: Optimize the RXIU decentralized storage network (p2p protocol, download path, RS block sharing) and navigate its codebase; use for performance tuning, protocol changes, and module-level refactors.
---

# RXIU Skill

Use this skill when improving RXIU's decentralized storage and P2P transfer performance, modifying the network protocol, or refactoring the download/RS pipeline.

## Quick start

- Review `report/performance_optimization.md` for existing bottleneck analysis.
- Use `cmd.md` to understand user-facing CLI/TUI commands and workflows.
- Use `references/files.md` for a per-file purpose map before editing.

## Workflow (typical optimization task)

1. Identify the performance target (latency, throughput, block download, peer discovery).
2. Map the change across modules (p2p protocol/messages, daemon IPC, TUI download path, RS storage/sync).
3. Adjust configuration defaults in `src/config.rs` when safe.
4. Validate logic in `src/p2p/service.rs`, `src/p2p/node.rs`, and `src/tui/app.rs`.
5. Update docs in `cmd.md` or `report/performance_optimization.md` if behavior changes.

## Key areas

- P2P protocol: `src/p2p/messages.rs`, `src/p2p/codec.rs`, `src/p2p/node.rs`
- Service orchestration: `src/p2p/service.rs`, `src/daemon/server.rs`, `src/daemon/rs_sync.rs`
- RS block transfer: `src/tui/block_client.rs`, `src/rs/mod.rs`, `src/rs/sync.rs`
- TUI workflow: `src/tui/app.rs`, `src/tui/input.rs`, `src/tui/render.rs`

## References

- File-by-file purposes: `Skills/rxiu/references/files.md`
