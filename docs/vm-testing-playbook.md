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

## 2. Test Scenarios & Expected Exit Codes

| # | Scenario | Test Action | Expected Exit Code | Expected State |
|---|---|---|---|---|
| **1** | Clean Install | `winget install --id Git.Git --exact --silent` | `0` (`SUCCESS`) | `Completed` |
| **2** | Missing Package | `winget list --id NonExistent.Pkg --exact` | `20` (`NOT_FOUND`) | `Missing` |
| **3** | UAC Prompt Cancel | Install an elevated tool (e.g. `Docker.DockerDesktop`) and click **No** on the Windows UAC dialog | `1223` (`ERROR_CANCELLED`) | `Paused` |
| **4** | Reboot Required | Install a subsystem component (e.g. `Microsoft.WSL`) | `3010` (`ERROR_SUCCESS_REBOOT_REQUIRED`) | `RebootRequired` |
| **5** | Plan Resume | Re-run EstzKit / `resume_plan` after reboot | Resumes from pending steps | `Completed` |

---

## 3. Verification Protocol

After executing tests in the sandbox or VM:
1. Export the SQLite database from `%APPDATA%\estzkit\state.db` (or local `estzkit.db`).
2. Run `sqlite3 estzkit.db "SELECT package_id, state, exit_code, message FROM steps;"`.
3. Verify that exit codes match the table above and transitions are correctly logged.
