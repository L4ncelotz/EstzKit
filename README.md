# EstzKit

Developer Workstation Installer / Orchestration Layer — a desktop app that installs a dev environment from declarative profiles (Fullstack, DevOps, Rust, ...) via the packages already on the machine (winget on Windows in MVP).

Status: **v0.1-rc** — Core installer engine, dependency resolver, SQLite resume engine, 17 packages, 3 profiles, and CI workflow in place.

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

## Why not `winget configure`?

`winget configure` applies DSC (Desired State Configuration) YAML files directly. While useful for static environments, it leaves critical developer workflow gaps:

- **No profiles / dynamic curation:** Users must maintain manual DSC scripts rather than choosing composable profiles (e.g. Fullstack, DevOps, Rust).
- **No topological dependency resolution:** Does not resolve complex prerequisite orderings (such as enabling WSL2 before installing Docker Desktop).
- **No 3-state detection:** Only checks basic package presence, unable to differentiate `missing`, `installed_ok`, and `installed_version_mismatch` via active verification commands.
- **No verification loop:** Does not execute tool verification commands (`node --version`, `rustup --version`) or validate outputs against regular expressions.
- **No ACID resumability:** Cannot persist in-flight execution plans across unexpected interruptions or required system reboots.

EstzKit provides this reliability layer on top of native package managers (winget on Windows).

## Windows SmartScreen Notice (v0.1)

v0.1 installer builds are currently **unsigned** to keep the project open and free without upfront certificate costs.

When launching the installer (`.msi` / `.exe`), Windows Defender SmartScreen may display:

> **Windows protected your PC**  
> Microsoft Defender SmartScreen prevented an unrecognized app from starting.

**To run the installer:**
1. Click **More info**.
2. Click **Run anyway**.

Trusted EV code signing certificates will be introduced in future community releases.

## Screenshots

### Profile & Package Selection
![Profile Picker](docs/screenshots/profile.png)

### Plan Preview & UAC Warnings
![Plan Preview](docs/screenshots/plan.png)

### Real-time Progress & Persisted Store Logs
![Install Progress](docs/screenshots/progress.png)

## License

EstzKit is dual-licensed under:
- [MIT License](LICENSE-MIT)
- [Apache License 2.0](LICENSE-APACHE)

See [LICENSE](LICENSE) for details.
