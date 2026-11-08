# EstzKit

Developer Workstation Installer / Orchestration Layer — a desktop app that installs a dev environment from declarative profiles (Fullstack, DevOps, Rust, ...) via the packages already on the machine (winget on Windows in MVP).

Status: **scaffolded** — architecture decisions locked (ADR-001 … ADR-010), Cargo workspace + React UI + bundled manifests created 2026-11-08. Rust build pending toolchain install (rustup + MSVC); frontend + manifest validation verified.

## Stack (locked — see ADR)

| Layer | Choice |
|---|---|
| Desktop shell | Tauri v2, pin `~2.12` (2.12.5 latest stable at check time) |
| Core logic | Rust 1.99 (`rust-toolchain.toml`), `crates/core` has no tauri dep |
| CLI | `crates/cli` (estzkit-cli) — vertical slice calling core directly |
| Frontend | React 19 + TypeScript 7, `src/`, types via ts-rs |
| Local state | SQLite (`rusqlite`, bundled) |
| Manifest source (MVP) | Bundled YAML + `ESTZKIT_MANIFESTS`/`--manifest-dir` override; Git registry in v2 |
| Elevation (MVP) | Standard user (option A); no `requireAdministrator` |
| Build package manager | pnpm 12 (`packageManager: pnpm@12.18.0`) |
| Provider (MVP) | Winget only, `--scope user` when supported |
| Install strategy (MVP) | `package_manager` only (no scripts) |

## Docs

- [System Design](docs/devkit-system-design.md) — requirements, data flow, state machine, provider trait, error codes, security model
- [Tech Stack ADR](docs/devkit-tech-stack-adr.md) — decisions ADR-001 … ADR-010, final stack, action items

## Layout

```
Cargo.toml          [workspace] members = ["crates/*", "src-tauri"]
rust-toolchain.toml  channel 1.99
pnpm-workspace.yaml  packages: ["."]
crates/
  core/             Rust library: manifest/, resolver, planner, executor, provider/, detect, verify, store — no tauri dep, emits via EventSink
  cli/              estzkit-cli: detect / profiles / packages / plan / install (core directly, stdout events)
src-tauri/          Tauri v2 host: IPC commands + events only (~thin adapter)
src/                React + TS UI (profile picker, package checklist, progress view)
manifests/          packages/*.yaml + profiles/*.yaml (bundled at build via rust-embed; ESTZKIT_MANIFESTS override)
schemas/            JSON Schemas: package.schema.json, profile.schema.json
scripts/            validate-manifests.mjs (ajv + js-yaml, runs in CI)
docs/               system design + ADRs
```

## Quick start

```bash
pnpm install
pnpm run validate:manifests   # JSON Schema check on manifests/
pnpm run dev                  # vite dev server (frontend only; needs Rust build for IPC)
```

Rust side requires Rust 1.99 (rustup) + MSVC Build Tools + WebView2 SDK on Windows — `cargo test` once the toolchain is installed.
