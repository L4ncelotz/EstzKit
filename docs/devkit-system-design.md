# DevKit — System Design Document

*Developer Workstation Installer / Orchestration Layer*

---

## 1. Requirements Gathering

### 1.1 Functional Requirements

| # | Requirement |
|---|---|
| F1 | ตรวจจับซอฟต์แวร์ที่ติดตั้งอยู่แล้วบนเครื่อง (ไม่ reinstall ซ้ำ) |
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
                         │ Tauri IPC (commands/events)
┌───────────────────────▼──────────────────────────────┐
│                     Rust Core                        │
│                                                       │
│  ┌───────────────┐   ┌──────────────────┐            │
│  │ Manifest       │   │ Dependency        │            │
│  │ Loader/Parser  │──▶│ Resolver (topo)   │            │
│  └───────────────┘   └────────┬──────────┘            │
│                                │                       │
│                       ┌────────▼──────────┐            │
│                       │     Planner        │            │
│                       │ (build install     │            │
│                       │  execution plan)   │            │
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
│        │ Provider   │   │ Verification │   │ Recovery  │ │
│        │ (Winget)   │   │   Engine     │   │  Engine   │ │
│        └────────────┘   └──────────────┘   └───────────┘ │
└──────────────────────────────────────────────────────┘
                         │
              ┌──────────▼──────────┐
              │  Manifest Registry   │
              │  (Git repo: YAML)    │
              │  packages/ profiles/ │
              └───────────────────────┘
```

### 2.2 Data Flow (ติดตั้ง 1 profile)

```
User เลือก Profile "Fullstack Developer"
        ↓
UI → Tauri command: install_profile("fullstack")
        ↓
Rust: โหลด manifest ของทุก package ใน profile
        ↓
Dependency Resolver: สร้าง topological order
        (เช่น WSL2 ก่อน Docker)
        ↓
Planner: เช็คของที่มีอยู่แล้ว (skip) → สร้าง execution plan
        ↓
State Machine Executor: รันทีละ step, emit event กลับ UI
        pending → resolving → downloading → installing → verifying → completed
        ↓
Provider (Winget): เรียก winget install จริง
        ↓
Verification Engine: รัน verify.command ตาม manifest
        ↓
ผลลัพธ์ส่งกลับ UI แบบ real-time ผ่าน Tauri event
```

### 2.3 API Contract (Tauri Commands)

```rust
// Commands (UI → Rust)
detect_installed() -> Vec<DetectedPackage>
list_profiles() -> Vec<Profile>
plan_install(package_ids: Vec<String>) -> InstallPlan
execute_plan(plan_id: String) -> ()
cancel_plan(plan_id: String) -> ()
retry_step(plan_id: String, step_id: String) -> ()

// Events (Rust → UI, streamed)
event: "install:state_changed"
payload: { plan_id, package_id, state: StepState, message?, error? }
```

### 2.4 Storage Choices

- **Manifest Registry**: Git repository (YAML files) — ไม่ต้องใช้ database, versioning ได้ฟรีจาก git history, PR review flow ธรรมชาติ
- **Local state**: SQLite ไฟล์เดียวใน `%APPDATA%/devkit/state.db` — เก็บ install history, current plan state (สำหรับ resume หลัง restart app)
- **Config**: TOML ไฟล์ local สำหรับ user preference (เช่น default profile, proxy settings)

---

## 3. Deep Dive

### 3.1 Data Model

```yaml
# Package manifest
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
depends_on: []          # list ของ package name ที่ต้องติดตั้งก่อน
install:
  strategy: package_manager   # | direct | script (v2+)
verify:
  command: node
  args: ["--version"]
  expect_regex: "^v\\d+\\.\\d+\\.\\d+$"
security:
  allow_scripts: false
  allow_elevation: false
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
trait Provider {
    fn is_available(&self) -> bool;
    fn is_installed(&self, spec: &PackageSpec) -> Result<bool>;
    fn install(&self, spec: &PackageSpec) -> Result<InstallHandle>;
    fn uninstall(&self, spec: &PackageSpec) -> Result<()>;
}

struct WingetProvider;
struct BrewProvider;   // v2
struct AptProvider;    // v2
```

Core ไม่รู้จัก syntax ของ `winget install --id X --version Y` เลย — รู้แค่เรียก `provider.install(spec)` แล้วรอ `Result`

### 3.3 State Machine (explicit, ไม่ใช่ binary success/fail)

```
pending → resolving → downloading → installing → verifying → completed
                                          │
                                          ▼
                                       failed ──▶ retry / rollback
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
| Elevation ถูกปฏิเสธ (UAC cancel) | Pause plan, ให้ user retry เมื่อพร้อม |

### 3.6 Security Model (MVP scope)

- `allow_scripts: false` เป็นค่า **hardcoded** ใน MVP — ยังไม่รองรับ `strategy: script` เลย เพราะเป็น attack surface ใหญ่สุด
- Manifest ทุกตัวผ่าน JSON Schema validation ก่อน merge (CI)
- Community PR ต้องผ่าน human review อย่างน้อย 1 คนก่อน merge เข้า registry
- Local: manifest ที่ดึงจาก registry ควร pin ด้วย commit hash หรือ signed release ไม่ใช่ HEAD ของ branch แบบ live

---

## 4. Scale & Reliability

*(หมายเหตุ: DevKit เป็น desktop client-side tool ไม่ใช่ web service ดังนั้น "scale" หมายถึง scale ของจำนวน package/profile และจำนวน installation ต่อเครื่อง ไม่ใช่ concurrent request)*

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
| **ห้าม `strategy: script` ใน MVP** | ตัดความเสี่ยง security ใหญ่สุดออกไปก่อน | บาง tool ที่ไม่มีใน package manager จะติดตั้งผ่าน DevKit ไม่ได้ในเวอร์ชันแรก |
| **Topological sort แทน full dependency graph solver** | เร็ว เขียนง่าย พอกับ use case จริง | ถ้าอนาคตมี version conflict ซับซ้อนแบบ npm จะต้อง redesign |
| **SQLite local state แทน stateless** | Resume ได้หลัง crash/restart | เพิ่ม complexity ของ state migration เมื่อ schema state เปลี่ยน |

### สิ่งที่ควร "revisit" เมื่อระบบโตขึ้น

1. เมื่อมี provider หลายตัว (brew, apt, dnf) — ต้องทบทวนว่า version resolution ข้าม OS ทำงานสอดคล้องกันไหม (เช่น `node@24.6.0` มีจริงทุก provider หรือไม่)
2. เมื่อ registry มี package หลายร้อยตัว — ทบทวนว่ายัง load ทั้งหมดตอน startup ได้ไหม หรือต้องทำ lazy/paginated fetch
3. เมื่อเริ่มมี community PR จำนวนมาก — ทบทวน security model จาก "human review" เป็น "automated sandbox + policy check" ตามที่ design ไว้ล่วงหน้า
4. เมื่อรองรับ `strategy: direct` หรือ `script` ในอนาคต — ต้องทบทวน threat model ใหม่ทั้งหมด ไม่ใช่แค่ต่อยอดจากของเดิม
