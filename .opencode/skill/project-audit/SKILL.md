---
name: project-audit
description: Audit the RXIU codebase for architecture mapping, performance hotspots, and risks (unused modules, dead code, config drift).
---

# Project Audit Skill

Use this skill to produce a directory-level module map, review performance bottlenecks, and surface risk flags in the RXIU repository.

## Quick start

- Review `cmd.md` for CLI/TUI workflows and user-facing entry points.
- Review `report/performance_optimization.md` for known bottlenecks and proposed fixes.
- Use `references/files.md` to navigate key modules and their roles.

## Workflow

1. Map architecture: summarize each top-level directory and core modules; verify entrypoints (`src/main.rs`, `src/bin/rxiu-daemon.rs`).
2. Performance scan: align hot paths with `report/performance_optimization.md` and check related modules (P2P, RS sync, TUI block download).
3. Risk scan: look for unused modules, dead code candidates, and config mismatches (`src/config.rs`, feature flags, protocol constants).
4. Produce audit outputs: module map, bottleneck notes, and risk list with suggested next checks.

## Key areas

- P2P networking: `src/p2p/`
- Daemon + IPC: `src/daemon/`
- RS block sharing: `src/rs/`
- Storage zones: `src/storage/`
- TUI workflow: `src/tui/`
- CLI + REPL: `src/cli/`
- UI helpers: `src/ui/`
- Config + errors: `src/config.rs`, `src/error.rs`

## Outputs

- Directory-level module map with key file roles.
- Performance bottleneck summary linked to existing report items.
- Risk list (unused modules, potential dead code, config drift) with pointers.

## References

- CLI manual: `cmd.md`
- Performance report: `report/performance_optimization.md`
- Module map: `references/files.md`
