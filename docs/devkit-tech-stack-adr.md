# DevKit — Architecture Decision Records (Tech Stack)

**Deciders:** Estz (solo maintainer, future OSS contributors)
**Date:** 2026-09-03
**Context:** เลือก stack ที่ solo dev คนเดียวดูแลได้จริง แต่ยัง scale ไปเป็น open source community project ได้โดยไม่ต้อง rewrite

---

## ADR-001: Desktop Shell — Tauri vs Electron vs Native

**Status:** Accepted

### Context
DevKit เป็น system utility ที่ต้องเรียก OS-level command (winget, registry, WSL check) บ่อย ต้อง startup เร็ว และ binary ต้องไม่ใหญ่เกินไปเพราะเป็น "installer" ตัวเอง — ถ้าตัวมันเองหนัก 150MB จะดูย้อนแย้ง

### Options Considered

| Dimension | Tauri | Electron | Native (WinUI3/C#) |
|---|---|---|---|
| Binary size | ~10-15MB | ~120-150MB | ~15-30MB |
| Startup time | เร็ว (native webview) | ช้ากว่า (bundled Chromium) | เร็วที่สุด |
| Cross-platform | ดี (Win/Mac/Linux) | ดี | ไม่มี (Windows only) |
| System-call ergonomics | ดีมาก (Rust core) | ต้องพึ่ง Node native module | ดีมาก |
| Ecosystem/plugin | เล็กกว่า | ใหญ่ที่สุด | เฉพาะ .NET |
| Team familiarity (solo) | ต้องเรียน Rust | เขียนได้ไวถ้าถนัด JS | ผูก Windows only ตัดโอกาส cross-platform ในอนาคต |

**Pros (Tauri):** เบา, เร็ว, บังคับให้ core logic อยู่ใน Rust ซึ่งเหมาะกับงาน system-level อยู่แล้ว, เปิดทาง cross-platform ในอนาคตโดยไม่ต้อง rewrite
**Cons (Tauri):** ต้องเขียน Rust ให้คล่อง (learning curve ถ้ายังไม่ถนัด), community/plugin เล็กกว่า Electron

### Decision
**Tauri** — เหตุผลหลักคือ requirement N1/N2 (startup เร็ว, binary เล็ก) เป็น non-negotiable สำหรับ tool ประเภทนี้ และ Rust core จำเป็นอยู่แล้วสำหรับงาน dependency-resolution/state-machine ที่ซับซ้อน

---

## ADR-002: Core Logic Language — Rust (ผูกกับ ADR-001)

**Status:** Accepted

### Context
เมื่อเลือก Tauri แล้ว core ต้องเป็น Rust โดยธรรมชาติ (ไม่ใช่ทางเลือกอิสระอีกต่อไป) แต่ยังต้องตัดสินใจว่าจะเขียน "business logic ทั้งหมด" ใน Rust หรือ offload บางส่วนไป Node sidecar

### Options Considered

| Dimension | All-in-Rust | Rust shell + Node sidecar for logic |
|---|---|---|
| Complexity | เดียว, สม่ำเสมอ | สอง runtime, ต้องทำ IPC ระหว่างกัน |
| Performance | ดี | มี overhead ข้าม process |
| Contributor onboarding (OSS) | ต้องรู้ Rust | เข้าถึงง่ายกว่าถ้าใช้ TS ทั้งคู่ (UI+logic) |
| Debugging | debug จุดเดียว | ต้อง debug 2 process |

### Decision
**All-in-Rust สำหรับ core** (manifest engine, resolver, state machine, provider) — การแยก sidecar เพิ่ม complexity โดยไม่ได้อะไรกลับมาคุ้มค่า สำหรับ solo maintainer ยิ่งต้อง debug น้อย runtime ยิ่งดี ส่วนเรื่อง contributor onboarding แก้ด้วยการทำ manifest เป็น YAML (ไม่ต้องรู้ Rust ก็ contribute package ใหม่ได้ — เข้าถึงง่ายที่สุดอยู่แล้วโดยไม่ต้องเปลี่ยน core language)

---

## ADR-003: Frontend Framework — React vs Svelte vs SolidJS

**Status:** Accepted

### Context
UI เป็นแค่ profile picker + progress view ไม่ได้ซับซ้อนมาก แต่ต้องคิดถึง "ใครจะมาช่วย contribute UI ในอนาคต"

### Options Considered

| Dimension | React | Svelte | SolidJS |
|---|---|---|---|
| Bundle size | ใหญ่กว่า | เล็กกว่า | เล็ก, เร็ว |
| Contributor pool (หา OSS contributor ง่าย) | ใหญ่ที่สุด | เล็กกว่า | เล็กที่สุด |
| Tauri official template support | ดี | ดี | ดี |
| Ecosystem components (progress bar, tree view ฯลฯ) | มากที่สุด | น้อยกว่า | น้อยที่สุด |

**สำหรับ desktop app ขนาดนี้ bundle size ไม่ใช่คอขวดจริง** เพราะไม่ได้โหลดผ่าน network — รันจาก local webview ทั้งหมด ดังนั้นข้อได้เปรียบเรื่อง bundle size ของ Svelte/Solid มีน้ำหนักน้อยกว่าตอนเทียบกับเว็บทั่วไป

### Decision
**React + TypeScript** — เหตุผลหลักคือ N4 (extensibility) ผ่าน community: ถ้าอยากให้ OSS project โตเร็ว การเลือก framework ที่มีคนรู้จักเยอะที่สุดลดแรงเสียดทานตอนมีคนอยากมาช่วย ข้อเสียเรื่อง bundle size ไม่กระทบ use case นี้จริง

---

## ADR-004: Local State Persistence — SQLite vs Flat File (JSON/TOML)

**Status:** Accepted

### Context
ต้องเก็บ: current install plan state (สำหรับ resume หลัง crash/restart), install history, detected package cache

### Options Considered

| Dimension | SQLite (rusqlite) | Flat JSON file + atomic write |
|---|---|---|
| Complexity | เพิ่ม dependency + schema migration | ง่ายกว่า, ไม่ต้องเรียน SQL |
| Crash safety | ACID transaction ป้องกัน state เสียหายกลาง write | เสี่ยง corrupt ถ้า crash ระหว่างเขียนไฟล์ (ต้อง handle เอง) |
| Query ในอนาคต (install history, reporting) | ทำได้ตรงไปตรงมา | ต้อง parse ทั้งไฟล์ทุกครั้ง |
| ขนาด dependency | +1 crate (rusqlite ~lightweight) | ไม่มี dependency เพิ่ม |

### Decision
**SQLite** — เหตุผลคือ N5 (reliability/resumability) เป็น requirement ที่ระบุไว้ชัดเจนแล้ว การจัดการ atomic write + partial-corruption ของ flat file ด้วยมือ จะกลายเป็นโค้ดที่ซับซ้อนพอ ๆ กับการ integrate SQLite ตั้งแต่แรก แต่ได้ ACID guarantee ฟรี และเปิดทางให้ทำ install-history/report ในอนาคตโดยไม่ต้อง migrate data model

**ข้อแลกที่ยอมรับ:** เพิ่ม 1 dependency และต้องคิดเรื่อง schema migration (ใช้ `refinery` หรือ migration file แบบง่ายพอสำหรับ local db)

---

## ADR-005: Manifest Registry Distribution — Git-based YAML vs Hosted API+DB

**Status:** Accepted

### Context
Manifest ของ package/profile ต้องกระจายให้ client ดึงไปใช้ และต้องรองรับ community PR ในอนาคต

### Options Considered

| Dimension | Git repo (YAML files) | Hosted API + Database |
|---|---|---|
| Infra cost | ศูนย์ (GitHub ฟรี) | ต้องมี server, ต้อง maintain uptime |
| Review flow | PR review เป็น audit trail ธรรมชาติ | ต้องสร้าง admin panel เอง |
| Versioning | ได้ฟรีจาก git history/tag | ต้องสร้างเอง |
| Offline/caching | Clone/pull เก็บ local ได้ง่าย | ต้องพึ่ง network เสมอ (หรือทำ cache layer เอง) |
| Dynamic query (เช่น search by category) | จำกัดกว่า ต้อง index เองฝั่ง client | ทำได้ native |

### Decision
**Git-based YAML registry** — สำหรับ solo maintainer ที่ไม่อยากดูแล server, และสำหรับ OSS project ที่อยาก community contribute ง่าย นี่คือทางเลือกที่ maintenance cost ต่ำสุดและ trust model (PR review) แข็งแรงที่สุด ข้อจำกัดเรื่อง dynamic query แก้ได้ด้วยการ build local index (SQLite เดียวกับ ADR-004) ตอน client sync manifest ลงมา

---

## ADR-006: Package Manager สำหรับ build ตัว DevKit เอง — pnpm vs npm vs Bun

**Status:** Accepted

### Context
Ironic แต่ต้องเลือก: ตัว DevKit เองก็ต้องมี JS package manager สำหรับ build frontend

### Decision
**pnpm** — เร็วกว่า npm, disk-efficient (content-addressable store), lockfile deterministic, เสถียรกว่า Bun ในแง่ ecosystem compatibility ตอนนี้ (2026) Bun น่าสนใจสำหรับ runtime แต่ในฐานะ package manager ของ build tool ที่ต้องการความเสถียรสูงสุด (เพราะ CI/release pipeline พึ่งมัน) pnpm ยังเป็นตัวเลือกที่ risk ต่ำกว่า

---

## สรุป Final Stack (ตารางเดียวจบ)

| Layer | เลือก | เหตุผลสั้น |
|---|---|---|
| Desktop shell | **Tauri** | เบา เร็ว เหมาะ system utility |
| Core logic | **Rust** | บังคับจาก Tauri + เหมาะงาน system-level |
| Frontend | **React + TypeScript** | contributor pool ใหญ่สุดสำหรับ OSS |
| Local state | **SQLite (rusqlite)** | ACID, resumability, query ได้ในอนาคต |
| Manifest registry | **Git repo + YAML** | ฟรี, PR review = audit trail, versioning ในตัว |
| Build package manager | **pnpm** | เร็ว, เสถียร, deterministic lockfile |
| Provider (MVP) | **Winget only** | scope เล็กสุดที่ ship ได้จริง, abstraction เปิดทางขยาย |
| Install strategy (MVP) | **package_manager only** (ห้าม script) | ตัด attack surface ใหญ่สุดออกก่อน |

## Consequences

**สิ่งที่ง่ายขึ้น:**
- Core language เดียว (Rust) ลด context-switching ให้ solo maintainer
- ไม่มี server ต้องดูแล (git registry + local SQLite)
- Contributor เข้าร่วมง่าย 2 ระดับ: แก้ manifest YAML (ไม่ต้องรู้ Rust) หรือแก้ core (React/Rust devs)

**สิ่งที่ยากขึ้น / ต้องรับความเสี่ยง:**
- ต้องเรียน Rust ให้แข็งจริงถ้ายังไม่ถนัด (learning curve ช่วงแรก)
- SQLite schema migration ต้องวางแผนตั้งแต่ v1 (เพิ่ม field ทีหลังไม่ยาก แต่เปลี่ยน type ยาก)
- Git-based registry ต้องคิดเรื่อง client sync/cache invalidation ตั้งแต่ต้น (เมื่อไหร่ pull ใหม่)

**สิ่งที่ต้อง revisit เมื่อโตขึ้น:**
- เมื่อเพิ่ม provider ที่ 2 (brew/apt) → ทดสอบว่า version string ตรงกันข้าม OS จริงไหม
- เมื่อ registry มี package หลายร้อย → พิจารณาทำ CDN cache ของ manifest แทน raw GitHub fetch ตรง ๆ
- เมื่อ community เริ่มส่ง PR จำนวนมาก → ยกระดับจาก "human review" เป็น automated policy check ตามที่วางไว้ใน security design

## Action Items
1. [ ] Scaffold Tauri + Rust + React project (cargo workspace แยก `core` crate ออกจาก `app` crate เพื่อ testability)
2. [ ] Define SQLite schema เวอร์ชันแรก (plans, steps, detected_packages tables)
3. [ ] สร้าง JSON Schema สำหรับ validate manifest YAML ใน CI
4. [ ] เขียน Provider trait + WingetProvider implementation ตัวแรก
5. [ ] ตั้ง pnpm workspace + Tauri dev script
