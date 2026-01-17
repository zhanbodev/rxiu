# RXIU file map

This list summarizes the purpose of each tracked file in the repository.

## Top-level

| Path | Purpose |
| --- | --- |
| Cargo.toml | Crate metadata, binary targets, and dependency list. |
| Cargo.lock | Dependency lockfile for reproducible builds. |
| build_windows.sh | Helper script to add Windows target and build release. |
| cmd.md | CLI/TUI command manual and usage examples (Chinese). |
| report/performance_optimization.md | RS download performance report and optimization proposals. |

## Binaries

| Path | Purpose |
| --- | --- |
| src/main.rs | Main TUI entrypoint; starts daemon if needed and configures logging. |
| src/bin/rxiu-daemon.rs | Background daemon entrypoint; runs P2P service, IPC server, and block server. |

## Crate root

| Path | Purpose |
| --- | --- |
| src/lib.rs | Crate module exports and public API re-exports. |
| src/error.rs | Application error types and Result alias. |
| src/config.rs | Persistent config load/save for RS concurrency and block size. |

## P2P networking

| Path | Purpose |
| --- | --- |
| src/p2p/mod.rs | P2P module wiring and re-exports. |
| src/p2p/messages.rs | P2P protocol message types and chunk size constants. |
| src/p2p/codec.rs | CBOR codec for libp2p request-response protocol. |
| src/p2p/node.rs | libp2p swarm setup, discovery, and request routing. |
| src/p2p/service.rs | Service layer and command/event loop with heartbeat and recovery. |
| src/p2p/recovery.rs | Network recovery logic for wake-from-sleep and stale peers. |
| src/p2p/protocol/mod.rs | P2P protocol extensions module. |
| src/p2p/protocol/peer_exchange.rs | Peer exchange data model and dialing helper. |

## Daemon and IPC

| Path | Purpose |
| --- | --- |
| src/daemon/mod.rs | Daemon module exports. |
| src/daemon/server.rs | IPC TCP server handling daemon requests. |
| src/daemon/block_server.rs | Binary block server for RS block transfer. |
| src/daemon/client.rs | Synchronous daemon IPC client for TUI. |
| src/daemon/protocol.rs | IPC request/response types and CBOR framing helpers. |
| src/daemon/rs_sync.rs | Background RS sync manager and scheduler. |
| src/daemon/proxy.rs | P2P proxy that calls daemon over IPC. |

## Storage (zones)

| Path | Purpose |
| --- | --- |
| src/storage/mod.rs | Storage module wiring and re-exports. |
| src/storage/backend.rs | StorageBackend trait definition. |
| src/storage/local.rs | Local filesystem backend implementation. |
| src/storage/metadata.rs | File metadata model and formatting helpers. |
| src/storage/zone.rs | Zone wrapper around a storage backend. |
| src/storage/manager.rs | ZoneManager with registry and active zone tracking. |

## RS (block sharing)

| Path | Purpose |
| --- | --- |
| src/rs/mod.rs | RS block store and metadata handling. |
| src/rs/sync.rs | RS sync helpers (HRW ownership, pruning). |

## CLI and REPL

| Path | Purpose |
| --- | --- |
| src/cli/mod.rs | CLI module wiring. |
| src/cli/commands.rs | Command handlers for zone operations and help output. |
| src/cli/repl.rs | Line-based REPL for basic zone commands. |

## TUI

| Path | Purpose |
| --- | --- |
| src/tui/mod.rs | TUI module wiring. |
| src/tui/app.rs | Main TUI state, event loop, transfers, and RS download logic. |
| src/tui/input.rs | Input handling and command dispatch. |
| src/tui/render.rs | Ratatui rendering for the UI. |
| src/tui/block_client.rs | Direct block client for RS binary block downloads. |

## Terminal UI utilities

| Path | Purpose |
| --- | --- |
| src/ui/mod.rs | UI module wiring. |
| src/ui/browser.rs | Interactive file browser for put/get flows. |
| src/ui/terminal.rs | Raw mode and terminal cleanup helpers. |
