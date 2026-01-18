# Project audit file map

Directory-level map with key modules and entrypoints for architectural review and audits.

## Top-level

| Path | Purpose |
| --- | --- |
| Cargo.toml | Crate metadata, binaries, and dependency list. |
| Cargo.lock | Dependency lockfile. |
| build_windows.sh | Helper script to build Windows release. |
| cmd.md | CLI/TUI command manual and usage examples (Chinese). |
| report/ | Performance report(s) and optimization notes. |
| src/ | Application source code. |

## Binaries

| Path | Purpose |
| --- | --- |
| src/main.rs | Main TUI entrypoint; starts daemon and configures logging. |
| src/bin/rxiu-daemon.rs | Background daemon entrypoint; runs P2P service and IPC servers. |

## Crate root

| Path | Purpose |
| --- | --- |
| src/lib.rs | Crate module exports and public API re-exports. |
| src/error.rs | Application error types and Result alias. |
| src/config.rs | Persistent config load/save for RS concurrency and block size. |

## P2P networking

| Path | Purpose |
| --- | --- |
| src/p2p/ | P2P module wiring, protocol, and swarm logic. |
| src/p2p/node.rs | libp2p swarm setup, discovery, and request routing. |
| src/p2p/service.rs | Service layer and command/event loop with heartbeat and recovery. |
| src/p2p/messages.rs | Protocol message types and chunk size constants. |
| src/p2p/codec.rs | CBOR codec for libp2p request-response protocol. |
| src/p2p/recovery.rs | Network recovery logic for wake-from-sleep and stale peers. |
| src/p2p/protocol/ | Protocol extensions and peer exchange helpers. |

## Daemon and IPC

| Path | Purpose |
| --- | --- |
| src/daemon/ | Daemon module wiring and IPC layers. |
| src/daemon/server.rs | IPC TCP server handling daemon requests. |
| src/daemon/block_server.rs | Binary block server for RS block transfer. |
| src/daemon/rs_sync.rs | Background RS sync manager and scheduler. |
| src/daemon/client.rs | Synchronous daemon IPC client for TUI. |
| src/daemon/protocol.rs | IPC request/response types and framing helpers. |
| src/daemon/proxy.rs | P2P proxy that calls daemon over IPC. |

## RS (block sharing)

| Path | Purpose |
| --- | --- |
| src/rs/ | RS block store and sync helpers. |
| src/rs/mod.rs | RS block store and metadata handling. |
| src/rs/sync.rs | RS sync helpers (HRW ownership, pruning). |

## Storage (zones)

| Path | Purpose |
| --- | --- |
| src/storage/ | Storage module wiring and backends. |
| src/storage/backend.rs | StorageBackend trait definition. |
| src/storage/local.rs | Local filesystem backend implementation. |
| src/storage/metadata.rs | File metadata model and formatting helpers. |
| src/storage/zone.rs | Zone wrapper around a storage backend. |
| src/storage/manager.rs | ZoneManager with registry and active zone tracking. |

## CLI and REPL

| Path | Purpose |
| --- | --- |
| src/cli/ | CLI module wiring. |
| src/cli/commands.rs | Command handlers for zone operations and help output. |
| src/cli/repl.rs | Line-based REPL for basic zone commands. |

## TUI

| Path | Purpose |
| --- | --- |
| src/tui/ | TUI module wiring, state, and rendering. |
| src/tui/app.rs | Main TUI state, event loop, transfers, and RS download logic. |
| src/tui/input.rs | Input handling and command dispatch. |
| src/tui/render.rs | Ratatui rendering for the UI. |
| src/tui/block_client.rs | Direct block client for RS binary block downloads. |

## UI utilities

| Path | Purpose |
| --- | --- |
| src/ui/ | Terminal UI helpers and interactive browser. |
| src/ui/browser.rs | Interactive file browser for put/get flows. |
| src/ui/terminal.rs | Raw mode and terminal cleanup helpers. |
