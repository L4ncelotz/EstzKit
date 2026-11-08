# EstzKit

Developer Workstation Installer / Orchestration Layer — a desktop app that installs a dev environment from declarative profiles (Fullstack, DevOps, Rust, ...) via the packages already on the machine (winget on Windows in MVP).

Status: **design phase** — architecture docs are in `docs/`, code scaffold has not started yet.

## Planned Stack

| Layer | Choice |
|---|---|
| Desktop shell | Tauri |
| Core logic | Rust |
| Frontend | React + TypeScript |
| Local state | SQLite (`rusqlite`) |
| Manifest registry | Git repo + YAML |
| Build package manager | pnpm |
| Provider (MVP) | Winget only |

## Docs

- [System Design](docs/devkit-system-design.md) — requirements, data flow, state machine, provider trait, security model (MVP: no scripts, package-manager only)
- [Tech Stack ADR](docs/devkit-tech-stack-adr.md) — decision records ADR-001 … ADR-006, final stack, action items

## Layout (planned)

```
crates/core/   Rust library: manifest parser, dependency resolver, state machine, providers, SQLite store
src-tauri/     Tauri host: IPC commands/events
src/           React + TypeScript UI
manifests/     Bundled YAML packages & profiles (MVP)
```
