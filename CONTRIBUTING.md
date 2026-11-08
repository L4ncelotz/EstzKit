# Contributing to EstzKit

Thank you for your interest in contributing to EstzKit!

---

## 1. Adding or Updating Package Manifests

EstzKit is designed so contributors can add new software packages by writing YAML manifests without knowing Rust:

1. Create a new YAML file in `manifests/packages/<name>.yaml`.
2. Follow the JSON Schema (`schemas/package.schema.json`):
   ```yaml
   schema_version: 1
   name: your-package-name
   category: runtime # e.g. runtime, devops, database, tools, editor
   windows:
     provider: winget
     package: Vendor.PackageName # exact Winget ID
     version: null
   depends_on: [] # list package names that must install first
   install:
     strategy: package_manager # MVP strictly requires package_manager (no scripts)
     scope: user # user or machine
   verify:
     command: binary-name
     args: ["--version"]
     expect_regex: "^v?\\d+\\.\\d+"
   security:
     allow_scripts: false # MVP requires false
     allow_elevation: true # true if installer requires Windows UAC
   ```
3. Run the schema validator:
   ```bash
   pnpm run validate:manifests
   ```
   All manifests must output `OK` before opening a pull request.

---

## 2. Pull Request Guidelines

- Ensure `pnpm run validate:manifests` and `pnpm run typecheck` pass.
- Write clear, focused commit messages.
- New packages must reference real, valid package IDs verifiable via `winget search <id>`.
- Any change to core logic, provider traits, or storage must include unit tests.
