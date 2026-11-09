import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  DetectedPackage,
  InstallPlan,
  PackageSummary,
  Profile,
  StepEvent,
  StepState,
  StoredStep,
} from "./types/ipc";

type StepLogs = Record<string, { state: StepState; message: string | null; error: string | null }>;

function badgeClass(state: StepState): string {
  return state.toLowerCase();
}

export default function App() {
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [packages, setPackages] = useState<PackageSummary[]>([]);
  const [selectedProfile, setSelectedProfile] = useState<string>("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [detected, setDetected] = useState<DetectedPackage[]>([]);
  const [plan, setPlan] = useState<InstallPlan | null>(null);
  const [stepLogs, setStepLogs] = useState<StepLogs>({});
  const [storedLogs, setStoredLogs] = useState<StoredStep[]>([]);
  const [showLogs, setShowLogs] = useState(false);
  const [busy, setBusy] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = useCallback(async () => {
    setError(null);
    try {
      const [profs, pkgs] = await Promise.all([
        invoke<Profile[]>("list_profiles"),
        invoke<PackageSummary[]>("list_packages"),
      ]);
      setProfiles(profs);
      setPackages(pkgs);
      if (profs.length > 0 && selectedProfile === "") {
        setSelectedProfile(profs[0].name);
        setSelected(new Set(profs[0].packages));
      }
    } catch (e) {
      setError(String(e));
    }
  }, [selectedProfile]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    void (async () => {
      try {
        const fn = await listen<StepEvent>("install:state_changed", (event) => {
          const e = event.payload;
          setStepLogs((prev) => ({
            ...prev,
            [e.packageId]: { state: e.state, message: e.message, error: e.error },
          }));
        });
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      } catch (e) {
        setError(String(e));
      }
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const onSelectProfile = (name: string) => {
    setSelectedProfile(name);
    const profile = profiles.find((p) => p.name === name);
    setSelected(new Set(profile?.packages ?? []));
    setPlan(null);
    setStepLogs({});
  };

  const togglePackage = (name: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(name)) {
        next.delete(name);
      } else {
        next.add(name);
      }
      return next;
    });
  };

  const runDetect = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await invoke<DetectedPackage[]>("detect_installed");
      setDetected(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const runPlan = async () => {
    setBusy(true);
    setError(null);
    try {
      const packages = Array.from(selected);
      const result = await invoke<InstallPlan>("plan_install", { packages });
      setPlan(result);
      setStepLogs({});
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const runExecute = async () => {
    if (!plan) return;
    setExecuting(true);
    setCancelling(false);
    setBusy(true);
    setError(null);
    try {
      await invoke("execute_plan", { plan, packages: plan.packageIds });
    } catch (e) {
      setError(String(e));
    } finally {
      setExecuting(false);
      setCancelling(false);
      setBusy(false);
    }
  };

  const runCancel = async () => {
    if (!plan) return;
    if (!executing) {
      setPlan(null);
      return;
    }
    setCancelling(true);
    setError(null);
    try {
      await invoke("cancel_plan", { planId: plan.id });
    } catch (e) {
      setError(String(e));
      setCancelling(false);
    }
  };

  const runResume = async () => {
    if (!plan) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("resume_plan", { planId: plan.id });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const runRetry = async (packageId: string) => {
    if (!plan) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("retry_step", { planId: plan.id, packageId });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const toggleViewLogs = async () => {
    if (!plan) return;
    if (showLogs) {
      setShowLogs(false);
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const logs = await invoke<StoredStep[]>("get_plan_steps", { planId: plan.id });
      setStoredLogs(logs);
      setShowLogs(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const hasReboot = Object.values(stepLogs).some((s) => s.state === "RebootRequired");
  const pausedIds = Object.entries(stepLogs)
    .filter(([, s]) => s.state === "Paused")
    .map(([id]) => id);

  return (
    <div className="app">
      <header>
        <h1>EstzKit</h1>
        <p className="muted">Minimal software setup for developers</p>
      </header>

      {error && <div className="banner paused">{error}</div>}
      {hasReboot && (
        <div className="banner reboot" style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <span>Reboot required — please restart your computer and re-run EstzKit.</span>
          {plan && (
            <button type="button" disabled={busy} onClick={runResume}>
              Resume from stored state
            </button>
          )}
        </div>
      )}

      <section className="card">
        <h2>Profile &amp; packages</h2>
        <div className="row">
          <label htmlFor="profile">Profile</label>
          <select
            id="profile"
            value={selectedProfile}
            onChange={(e) => onSelectProfile(e.target.value)}
          >
            {profiles.map((p) => (
              <option key={p.name} value={p.name}>
                {p.displayName} ({p.name})
              </option>
            ))}
          </select>
        </div>
        <div className="checklist" style={{ marginTop: "0.75rem" }}>
          {packages.map((pkg) => (
            <label key={pkg.name}>
              <input
                type="checkbox"
                checked={selected.has(pkg.name)}
                onChange={() => togglePackage(pkg.name)}
              />
              <span>{pkg.name}</span>
              <span className="muted">{pkg.category}</span>
            </label>
          ))}
        </div>
        {packages.length === 0 && <p className="muted">No packages loaded.</p>}
      </section>

      <section className="card">
        <h2>Detect</h2>
        <div className="row">
          <button type="button" className="primary" disabled={busy || selected.size === 0} onClick={runDetect}>
            Detect installed
          </button>
        </div>
        {detected.length > 0 && (
          <table style={{ marginTop: "0.75rem" }}>
            <thead>
              <tr>
                <th>Package</th>
                <th>Found</th>
                <th>Version</th>
                <th>Source</th>
              </tr>
            </thead>
            <tbody>
              {detected.map((d) => (
                <tr key={d.name}>
                  <td>{d.name}</td>
                  <td>{d.found ? "yes" : "no"}</td>
                  <td>{d.version ?? "—"}</td>
                  <td>{d.source}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </section>

      <section className="card">
        <h2>Plan</h2>
        <div className="row">
          <button type="button" disabled={busy || selected.size === 0} onClick={runPlan}>
            Preview plan
          </button>
          <button type="button" className="primary" disabled={busy || !plan || executing} onClick={runExecute}>
            Execute plan
          </button>
          <button type="button" disabled={!plan || cancelling} onClick={runCancel}>
            {cancelling ? "Cancelling..." : executing ? "Stop execution" : "Cancel plan"}
          </button>
        </div>
        {plan && (
          <>
            <p className="muted" style={{ marginTop: "0.5rem" }}>
              Plan {plan.id}
              {plan.uacWarnings > 0 && ` — ${plan.uacWarnings} UAC prompts expected (expected to prompt UAC)`}
              {plan.uacWarnings === 0 && " — no UAC prompts expected"}
            </p>
            <table>
              <thead>
                <tr>
                  <th>Package</th>
                  <th>Scope</th>
                  <th>Depends on</th>
                </tr>
              </thead>
              <tbody>
                {plan.steps.map((s) => (
                  <tr key={s.packageId}>
                    <td>{s.packageId}</td>
                    <td>{s.scope}</td>
                    <td>{s.dependsOn.join(", ") || "—"}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )}
      </section>

      <section className="card">
        <div className="row" style={{ justifyContent: "space-between", alignItems: "center" }}>
          <h2>Install progress</h2>
          {plan && (
            <button type="button" disabled={busy} onClick={toggleViewLogs}>
              {showLogs ? "Hide Logs" : "View Logs"}
            </button>
          )}
        </div>
        {plan && plan.steps.length === 0 && <p className="muted">No steps.</p>}
        {plan && (
          <table>
            <thead>
              <tr>
                <th>Package</th>
                <th>State</th>
                <th>Message</th>
              </tr>
            </thead>
            <tbody>
              {plan.steps.map((s) => {
                const log = stepLogs[s.packageId];
                const retryable = log?.state === "Paused";
                return (
                  <tr key={s.packageId}>
                    <td>{s.packageId}</td>
                    <td>
                      {log ? (
                        <span className={`badge ${badgeClass(log.state)}`}>{log.state}</span>
                      ) : (
                        <span className="badge">Pending</span>
                      )}
                    </td>
                    <td>
                      <div>{log?.message ?? "—"}</div>
                      {log?.error && <div className="error">{log.error}</div>}
                      {retryable && (
                        <button type="button" disabled={busy} onClick={() => runRetry(s.packageId)}>
                          Retry
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        {pausedIds.length > 0 && (
          <div className="banner paused" style={{ marginTop: "0.75rem" }}>
            Paused: {pausedIds.join(", ")} — UAC cancelled - retry when ready
          </div>
        )}
        {showLogs && (
          <div className="logs-panel" style={{ marginTop: "1rem" }}>
            <h3 style={{ fontSize: "1rem", marginBottom: "0.5rem" }}>
              Persisted Store Logs (Plan {plan?.id})
            </h3>
            {storedLogs.length === 0 ? (
              <p className="muted">No persisted steps found in store.</p>
            ) : (
              <table>
                <thead>
                  <tr>
                    <th>Package</th>
                    <th>State</th>
                    <th>Exit code</th>
                    <th>Message</th>
                  </tr>
                </thead>
                <tbody>
                  {storedLogs.map((entry, idx) => (
                    <tr key={`${entry.packageId}-${idx}`}>
                      <td>{entry.packageId}</td>
                      <td>
                        <span className={`badge ${badgeClass(entry.state as StepState)}`}>
                          {entry.state}
                        </span>
                      </td>
                      <td>{entry.exitCode !== null ? entry.exitCode : "—"}</td>
                      <td>{entry.message ?? "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        )}
        {!plan && <p className="muted">Run Preview plan, then Execute plan.</p>}
      </section>
    </div>
  );
}
