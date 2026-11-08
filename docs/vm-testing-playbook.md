# EstzKit — Clean VM Testing Playbook

This playbook documents the procedures for validating EstzKit's installation behaviors, exit codes, and SQLite resumability on an isolated Windows environment (Windows Sandbox or Hyper-V VM).

---

## 1. Automated Test with Windows Sandbox

Windows Sandbox provides a pristine, disposable Windows 10/11 environment.

### Prerequisites
- Windows 10/11 Pro, Enterprise, or Education
- Windows Sandbox feature enabled (`Enable-WindowsOptionalFeature -Online -FeatureName "Containers-DisposableClientVM"`)

### Execution
1. Double-click `scripts/test-sandbox.wsb` (or run `wsb.exe scripts\test-sandbox.wsb`).
2. The sandbox mounts the repository as a shared folder at `C:\EstzKit`.
3. Open PowerShell inside the Sandbox and run:
   ```powershell
   cd C:\EstzKit
   # Run automated probes
   powershell -ExecutionPolicy Bypass -File scripts\run-sandbox-tests.ps1
   ```

---

## 2. Test Scenarios & Observed Exit Codes

Executor special-casing for 1223/3010 was removed in this phase pending observation (see Approach 1): `map_exit_code` maps only `0 → Completed`, all others → `Failed`. Retries: `scope: user` auto-retries 3×; `scope: machine` nonzero is single-attempt (no UAC spam) per ADR-011.

| # | Scenario | Test Action | Observed Exit Code | Observed State |
|---|---|---|---|---|
| **1** | Clean Install | `estzkit-cli install git` | `pending CI / Sandbox run` | `pending CI / Sandbox run` |
| **2** | Missing Package | `estzkit-cli --manifest-dir <test-dir> install probe-missing` | `pending Sandbox run` | `pending Sandbox run` |
| **3** | UAC Prompt Cancel | `estzkit-cli install wsl2` (click **No** on Windows UAC prompt) | `pending VM run — see §3` | `pending VM run — see §3` |
| **4** | Reboot Required | `estzkit-cli install wsl2` (subsystem requiring restart) | `pending VM run — see §3` | `pending VM run — see §3` |
| **5** | Plan Resume | `estzkit-cli resume <plan_id>` after reboot | `pending VM run — see §3` (resumes from pending steps) | `pending VM run — see §3` |

---

## 3. Hyper-V manual runbook (scenarios 3–5)

Human-operated. Follow in order. Take a checkpoint immediately, before any test.

1. Create the VM (creates a 64 GB VHDX):
   ```powershell
   New-VM -Name EstzKit-Test -MemoryStartupBytes 8GB -NewVHDPath C:\EstzKit-Test\EstzKit-Test.vhdx -NewVHDSizeBytes 64GB -Generation 2
   ```
2. Set nested virtualization:
   ```powershell
   Set-VMProcessor -VMName EstzKit-Test -ExposeVirtualizationExtensions $true
   ```
3. Install Windows, then take a checkpoint (before any test):
   ```powershell
   Checkpoint-VM -Name EstzKit-Test -SnapshotName pre-scenario3
   ```
4. Scenario 3 — UAC cancel, no Docker needed:
   - **Pre-check (do not record as the result):** on the clean checkpoint, inside the VM run
     ```powershell
     winget install --id Microsoft.WSL --silent --scope machine
     ```
     note whether UAC pops and the `$LASTEXITCODE` you get. If no UAC pops (MSIX/Store build), restore `Restore-VMSnapshot -Name pre-scenario3 -VMName EstzKit-Test` and switch to another elevating package (other `scope: machine` first, else any `allow_elevation: true` in `manifests/packages/`) until UAC pops; only then restore again and do the real run.
   - **Real run:** inside the VM run
     ```powershell
     & $Cli install wsl2
     ```
     (or the pre-check-winning package), click **No** on the UAC prompt, record `$LASTEXITCODE` in PowerShell, and note the printed `plan_id`; then export `estzkit.db` and run
     ```powershell
     sqlite3 estzkit.db "SELECT package_id, state, exit_code, message FROM steps;"
     ```
5. Scenario 4 — reboot:
   - **Pre-check (do not record as the result):** restore `Restore-VMSnapshot -Name pre-scenario3 -VMName EstzKit-Test`, inside the VM run
     ```powershell
     winget install --id Microsoft.WSL --silent
     ```
     note the `$LASTEXITCODE` and whether a reboot is actually required. Reboot after WSL does not always return `3010` — if the code is not `3010`, record the wrapped code you actually got and do not force the expected value.
   - **Real run:** inside the VM run
     ```powershell
     & $Cli install wsl2
     ```
     record `$LASTEXITCODE`, note the printed `plan_id`, and reboot the VM.
6. Scenario 5 — resume:
   - After reboot, inside the VM run
     ```powershell
     & $Cli resume <plan_id>
     ```
     record `$LASTEXITCODE`, check stdout for resumed steps, and query `estzkit.db` to verify completed transitions:
     ```powershell
     sqlite3 estzkit.db "SELECT package_id, state, exit_code, message FROM steps;"
     ```

Note: winget may wrap installer codes — record what the shell actually prints, not what documentation predicts.

---

## 4. Verification Protocol

After executing tests in the sandbox or VM:
1. Export the SQLite database from `%APPDATA%\estzkit\state.db` (or local `estzkit.db`).
2. Run `sqlite3 estzkit.db "SELECT package_id, state, exit_code, message FROM steps;"`.
3. Compare SQLite rows against the Observed column (§2) and confirm transitions are correctly logged.

Any mismatch triggers Approach step 6: set the executor mapping from reality (`map_exit_code` must equal the observed values; update unit tests to pin them).
