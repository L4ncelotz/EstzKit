# EstzKit — System Design Document

*Developer Workstation Installer / Orchestration Layer*
*Status: implemented scaffold 2026-11-08 (Tauri v2 + Rust core + React 19; decisions ADR-007 … ADR-010). Section 2.1 เป็น reference design ของ core pipeline; contracts ที่ implement จริงอยู่ที่ `crates/core` (Rust) / `src-tauri` (IPC) / `src` (React).*

---

## 1. Requirements Gathering

### 1.1 Functional Requirements

| # | Requirement |
|---|---|
| F1 | ตรวจจับซอฟต์แวร์ที่ติดตั้งอยู่แล้วบนเครื่อง (ไม่ reinstall ซ้ำ) — แบบหลายชั้น (verify command → winget list → registry keys) ให้ผล 3 สถานะ `missing` / `installed_ok` / `installed_version_mismatch` (ADR-010) |
| F2 | ให้ผู้ใช้เลือกซอฟต์แวร์รายตัว หรือเลือกเป็น "Developer Profile" (Fullstack, DevOps, Rust ฯลฯ) |
| F3 | Resolve dependency ระหว่างซอฟต์แวร์ (เช่น Docker ต้องมี WSL2) |
| F4 | ติดตั้งผ่าน package manager ที่มีอยู่ (winget เป็นหลักใน MVP) |
| F5 | Verify ผลลัพธ์หลังติดตั้ง (รันคำสั่งตรวจสอบ version) |
| F6 | แสดง state การติดตั้งแบบ real-time พร้อม log ที่ diagnose ได้ |
| F7 | Manifest เป็นไฟล์ประกาศ (declarative, YAML) ที่ community ส่ง PR เพิ่มได้ |
| F8 | รองรับ retry เมื่อ step ใด step หนึ่งล้มเหลว โดยไม่ต้องเริ่มใหม่ทั้งหมด |

### 1.2 Non-Functional Requirements

| # | Requirement | เป้าหมาย |
|---|---|---|
| N1 | Startup time | < 1 วินาที (Tauri, ไม่ใช่ Electron) |
| N2 | Binary size | < 20MB (installer เอง ไม่รวมของที่ไป install) |
| N3 | Security | manifest จาก community ต้อง**ไม่สามารถ**รัน arbitrary code โดยไม่ผ่าน review |
| N4 | Extensibility | เพิ่ม package/provider ใหม่โดยไม่แก้ core binary |
| N5 | Reliability | Partial failure ต้อง resumable ไม่ทิ้งระบบไว้ในสถานะค้าง |
| N6 | Auditability | ทุก action ที่ทำกับเครื่อง user ต้อง log และอธิบายได้ว่าทำไมทำ |

### 1.3 Constraints

- ทีมพัฒนา: เริ่มจาก 1 คน (solo, open source ในอนาคต)
- Timeline: ต้องการ MVP ที่ใช้งานได้จริงเร็ว ไม่ใช่ platform สมบูรณ์แบบตั้งแต่วันแรก
- Stack ที่เลือกแล้ว: Tauri + Rust (core) + React/TypeScript (UI)
- Scope เวอร์ชันแรก: **Windows + winget เท่านั้น** — แต่ core ต้อง design ให้ไม่ผูกกับ Windows

---

## 2. High-Level Design

### 2.1 Component Diagram

```
┌────────────────────────────────────────────────────┐
│                  React + TypeScript UI              │
│   (Profile picker, package list, progress view)     │
└───────────────────────┬──────────────────────────────┘
                         │ Tauri IPC (commands/events, ts-rs)
┌───────────────────────▼──────────────────────────────┐
│                     Rust Core (crates/core)          │
│                  ไม่รู้เรื่อง Tauri — emit ผ่าน EventSink│
│                                                       │
│  ┌───────────────┐   ┌──────────────────┐            │
│  │ Manifest       │   │ Dependency        │            │
│  │ Source trait   │──▶│ Resolver (topo)   │            │
│  │ (Bundled/Dir)  │   └────────┬──────────┘            │
│  └───────────────┘            │                       │
│                       ┌────────▼──────────┐            │
│                       │     Planner        │            │
│                       │ (build install     │            │
│                       │  plan + UAC warn)  │            │
│                       └────────┬──────────┘            │
│                                │                       │
│                       ┌────────▼──────────┐            │
│                       │  State Machine     │            │
│                       │  Executor          │            │
│                       └────────┬──────────┘            │
│                                │                       │
│              ┌─────────────────┼─────────────────┐     │
│              │                 │                 │     │
│        ┌─────▼─────┐   ┌───────▼──────┐   ┌──────▼───┐ │
│        │ Provider   │   │  Detect (L1→L3)│   │  Store   │ │
│        │ (Winget)   │   │  Verify engine │   │ (SQLite) │ │
│        └────────────┘   └──────────────┘   └──────────┘ │
└──────────────────────────────────────────────────────────┘
        ↑                                  │
   (src-tauri thin IPC        estzkit-cli (crates/cli)
    + EventSink→emit)          EventSink→stdout
                         │
              ┌──────────▼──────────┐
              │  Manifests (bundled)│
              │  manifests/: YAML   │
              │  GitSource ใน v2    │
              └─────────────────────┘
```

### 2.2 Data Flow (ติดตั้ง 1 profile)

```
User เลือก Profile "Fullstack Developer"
        ↓
UI → Tauri command: plan_install(package_ids)  (กรองจาก profile.packages)
        ↓
Core: โหลด manifest จาก ManifestSource (BundledSource / DirSource)
        ↓
Dependency Resolver: สร้าง topological order (เช่น WSL2 ก่อน Docker)
        ↓
Planner: detect 3-state → skip installed_ok → สร้าง InstallPlan (+uac_warnings, scope)
        ↓
UI แสดง plan → user ยืนยัน → execute_plan(plan_id)
        ↓
State Machine Executor (SQLite persist ทุก transition):
        pending → resolving → downloading → installing → verifying → completed
                                          ├─e1223→ paused (UAC cancel → retry)
                                          ├─e3010→ reboot_required (restart แล้ว resume)
                                          └─eอื่น→ failed (retry 3x แบบ backoff เฉพาะ network)
        ↓
Provider (Winget): winget install --id X --scope user|machine …
        ↓
Detect/Verify (L1 verify command + PATH reload, L2 winget list, L3 registry keys)
        ↓
Core EventSink → Tauri emit "install:state_changed" → UI real-time (CLI: stdout)
```

### 2.3 API Contract (Tauri Commands)

```rust
// Commands (UI → Rust) — TS types gen จาก Rust ด้วย ts-rs; core ไม่รู้เรื่อง Tauri
detect_installed() -> DetectedPackage[]
list_profiles() -> Profile[]
list_packages() -> PackageSummary[]        // สำหรับ package checklist ใน UI
plan_install(packages: string[]) -> InstallPlan   // มี uac_warnings + step.scope
execute_plan(plan: InstallPlan, packages: string[]) -> void   // plan ส่งกลับมา (local same-process, ไม่ต้อง rehydrate จาก store ใน MVP)
cancel_plan(plan_id: string) -> void
retry_step(plan_id: string, step_id: string) -> void

// Events (Rust → UI, streamed) — core ส่งผ่าน trait EventSink; src-tauri แปลเป็น Tauri event
event: "install:state_changed"
payload: { plan_id, package_id, state: StepState, message?, error? }
```

### 2.4 Storage Choices

- **Manifest source**: MVP — ฝัง `manifests/` (YAML) เข้า binary ตอน build (`BundledSource` + `DirSource` override สำหรับ dev); v2 — Git repository + `GitSource` pin ด้วย commit hash, versioning ได้ฟรีจาก git history, PR review flow ธรรมชาติ
- **Local state**: SQLite ไฟล์เดียวใน `%APPDATA%/estzkit/state.db` (หรือผ่าน `$ESTZKIT_STORE` / `estzkit.db`) — เก็บ install history, current plan state (สำหรับ resume หลัง restart app)
- **Config**: TOML ไฟล์ local สำหรับ user preference (เช่น default profile, proxy settings)

---

## 3. Deep Dive

### 3.1 Data Model

```yaml
# Package manifest (ตัวอย่างเต็ม — ดู manifests/packages/*.yaml สำหรับของจริง)
schema_version: 1
name: node
category: runtime
windows:
  provider: winget
  package: OpenJS.NodeJS.LTS
  version: "24.6.0"
macos:
  provider: brew
  package: node
linux:
  provider: apt
  package: nodejs
depends_on: []          # list ของ package name ที่ต้องติดตั้งก่อน (เช่น docker depends_on: [wsl2])
install:
  strategy: package_manager   # | direct | script (v2+) — MVP รับ package_manager เท่านั้น
  scope: user                 # user | machine — user เมื่อ package รองรับ (ลด UAC prompt, ADR-008)
verify:
  command: node
  args: ["--version"]
  expect_regex: "^v\\d+\\.\\d+\\.\\d+$"
detect:                 # optional — ไม่ระบุให้ใช้ verify แทน (ADR-010)
  command: code
  args: ["--version"]
  # หรือ (GUI app ที่ไม่มี CLI ใน PATH):
  # winget_id: Microsoft.VisualStudioCode
security:
  allow_scripts: false
  allow_elevation: false   # true = "คาดว่าจะขอ UAC" → planner นับเป็น uac_warnings (ADR-008)
```

```yaml
# Profile manifest
schema_version: 1
name: fullstack-developer
display_name: "Fullstack Developer"
packages:
  - node
  - pnpm
  - bun
  - git
  - vscode
  - docker
  - postgresql
  - redis
```

### 3.2 Provider Abstraction (Trait)

```rust
// core ไม่รู้จัก syntax ของ `winget install --id X --version Y` เลย — รู้แค่เรียก provider.install(...)
// แล้วรอ ExitCode เพื่อ map เป็น state (ดู §3.5). progress/stream ผ่าน EventSink (trait) ไม่ใช่ Tauri.
trait Provider {
    fn kind(&self) -> ProviderKind;
    fn is_available(&self) -> bool;
    fn install(&self, spec: &OsProvider, scope: InstallScope,
               sink: &mut dyn EventSink, package_id: &str) -> Result<ExitCode>;
    fn uninstall(&self, spec: &OsProvider) -> Result<()>;
}

// L2 detect ใช้ `winget list --id <X> --exact` + session cache (ช้า + parse เปราะ);
// L1 (verify command) + L3 (registry keys) อยู่ฝั่ง detect.rs — ดู §3.1/ADR-010.
struct WingetProvider;
struct BrewProvider;   // v2
struct AptProvider;    // v2
```

### 3.3 State Machine (explicit, ไม่ใช่ binary success/fail)

```
pending → resolving → downloading → installing → verifying → completed
                                          ├─ e1223 → paused    (UAC cancel → user retry)
                                          ├─ e3010 → reboot_required (restart → resume จาก SQLite)
                                          ├─ eอื่น  → failed    ──▶ retry / rollback
                                          └─ skip ที่ planner (installed_ok) → skipped
```

แต่ละ state transition ถูก log และ emit เป็น event ทำให้ UI แสดง:

```
Docker
✓ Downloaded
✓ Installed
✗ Verification failed
  Reason: WSL2 is not enabled
  [Retry]  [View Logs]  [Fix WSL2]
```

### 3.4 Dependency Resolution

ใช้ **topological sort แบบง่าย** (ไม่ใช่ SAT solver) เพราะ dependency ของ dev tool ส่วนใหญ่เป็น flat "A ต้องมาก่อน B" ไม่มี version conflict ซับซ้อนแบบ npm:

```
graph = build_graph(selected_packages)
if has_cycle(graph): error("circular dependency in manifest")
order = topological_sort(graph)
```

### 3.5 Error Handling & Retry Logic

| ประเภท error | การจัดการ |
|---|---|
| Network unreachable | Retry 3 ครั้ง (exponential backoff) → ถ้ายังพัง แสดง diagnostics |
| Provider ไม่พบ package | Fail step นั้น, ไม่ block package อื่นที่ไม่ depend มัน |
| Verification ล้มเหลว | ไม่ rollback อัตโนมัติ — แสดง reason + suggested fix ให้ user ตัดสินใจ |
| Elevation ถูกปฏิเสธ (UAC cancel, exit code 1223 ERROR_CANCELLED) | `paused` ไม่ใช่ `failed` — ให้ user retry เมื่อพร้อม (ADR-008) |
| ต้อง restart เครื่อง (exit code 3010, โดยเฉพาะหลัง WSL2) | `reboot_required` — บันทึก plan ลง SQLite, แสดง banner, resume หลัง reboot (RunOnce / ผู้ใช้เปิดแอปเอง) |
| Exit code อื่น | `failed` + message + ปุ่ม retry; **ห้าม hardcode mapping เอง — ต้องทดสอบบน VM จริงแล้วทำตารางจากผลที่เห็น** |

### 3.6 Security Model (MVP scope)

- `allow_scripts: false` เป็นค่า **hardcoded** ใน MVP — ยังไม่รองรับ `strategy: script` เลย เพราะเป็น attack surface ใหญ่สุด
- Manifest ทุกตัวผ่าน JSON Schema validation ก่อน merge (CI) — `schemas/*.schema.json` + `pnpm run validate:manifests`
- Community PR ต้องผ่าน human review อย่างน้อย 1 คนก่อน merge เข้า registry
- Local: manifest ที่ดึงจาก registry (v2 `GitSource`) ควร pin ด้วย commit hash หรือ signed release ไม่ใช่ HEAD ของ branch แบบ live
- **ห้าม `requireAdministrator` บนแอป**: `allow_elevation: true` ใน manifest หมายความว่า "package นี้คาดว่าจะขอ UAC" เพื่อเตือนผู้ใช้ก่อนเริ่มเท่านั้น — แอปรันด้วยสิทธิ์ปกติ (ADR-008 Option A; Option C ถูกปฏิเสธเพราะ webview ทั้งตัวจะรันเป็น admin)

---

## 4. Scale & Reliability

*(หมายเหตุ: EstzKit เป็น desktop client-side tool ไม่ใช่ web service ดังนั้น "scale" หมายถึง scale ของจำนวน package/profile และจำนวน installation ต่อเครื่อง ไม่ใช่ concurrent request)*

- **Load ที่แท้จริง**: จำนวน package ใน registry (คาดว่าจะโตจาก ~15 → หลายร้อยเมื่อ community มีส่วนร่วม) — โครงสร้าง manifest แบบไฟล์แยก + lazy load ทำให้ scale ได้โดยไม่กระทบ startup time
- **Failover**: ถ้า provider หนึ่งใช้ไม่ได้ (เช่น winget source ล่ม) ระบบควร fallback ไป strategy อื่นถ้ามี (v2+)
- **Monitoring**: เนื่องจากเป็น client-side ไม่มี server ให้ monitor — สิ่งที่ควรมีแทนคือ **opt-in anonymous telemetry** เพื่อรู้ pattern ของ failure จริงจาก user จำนวนมาก (design ไว้ตั้งแต่ต้น, ไม่ต้อง implement ใน MVP)
- **Resumability**: SQLite local state ทำให้ปิดโปรแกรมกลาง install แล้วเปิดใหม่ resume ได้จาก step ล่าสุดที่ยังไม่ completed

---

## 5. Trade-off Analysis

| การตัดสินใจ | ข้อดี | ข้อเสีย / สิ่งที่ยอมแลก |
|---|---|---|
| **Tauri แทน Electron** | Binary เล็ก, startup เร็ว, memory ต่ำ | Ecosystem ปลั๊กอินน้อยกว่า, IPC ต้องเขียนเอง |
| **Manifest = YAML ใน Git** แทน database + API server | ไม่ต้องมี backend, PR review เป็น audit trail ธรรมชาติ, ฟรี hosting (GitHub) | ไม่มี dynamic query, ต้อง sync/pull registry เอง, versioning ต้องคิดเรื่อง cache |
| **Provider abstraction ตั้งแต่ MVP** แม้รองรับแค่ winget | ขยาย OS ทีหลังไม่ต้อง refactor core | เพิ่ม complexity เล็กน้อยตั้งแต่วันแรกที่ยังไม่ได้ใช้ |
| **ห้าม `strategy: script` ใน MVP** | ตัดความเสี่ยง security ใหญ่สุดออกไปก่อน | บาง tool ที่ไม่มีใน package manager จะติดตั้งผ่าน EstzKit ไม่ได้ในเวอร์ชันแรก |
| **Topological sort แทน full dependency graph solver** | เร็ว เขียนง่าย พอกับ use case จริง | ถ้าอนาคตมี version conflict ซับซ้อนแบบ npm จะต้อง redesign |
| **SQLite local state แทน stateless** | Resume ได้หลัง crash/restart | เพิ่ม complexity ของ state migration เมื่อ schema state เปลี่ยน |
| **แอปรันสิทธิ์ปกติ (UAC option A) แทน `requireAdministrator`** | ไม่เปิด attack surface ของ webview-as-admin; คัมมา `allow_elevation` เป็นแค่คำเตือน | UAC เด้งหลายครั้งต่อ plan; elevated helper ใน v2 ถ้าจำเป็น |

### สิ่งที่ควร "revisit" เมื่อระบบโตขึ้น

1. เมื่อมี provider หลายตัว (brew, apt, dnf) — ต้องทบทวนว่า version resolution ข้าม OS ทำงานสอดคล้องกันไหม (เช่น `node@24.6.0` มีจริงทุก provider หรือไม่)
2. เมื่อ registry มี package หลายร้อยตัว — ทบทวนว่ายัง load ทั้งหมดตอน startup ได้ไหม หรือต้องทำ lazy/paginated fetch
3. เมื่อเริ่มมี community PR จำนวนมาก — ทบทวน security model จาก "human review" เป็น "automated sandbox + policy check" ตามที่ design ไว้ล่วงหน้า
4. เมื่อรองรับ `strategy: direct` หรือ `script` ในอนาคต — ต้องทบทวน threat model ใหม่ทั้งหมด ไม่ใช่แค่ต่อยอดจากของเดิม
