//! EstzKit Tauri v2 thin IPC host.
//!
//! All commands delegate to `estzkit-core`; this crate only owns the Tauri
//! event bridge ([`TauriEventSink`]) and command plumbing. Progress events
//! are emitted under the `"install:state_changed"` event name. The app is a
//! standard-user app: no `requireAdministrator` manifest anywhere.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use estzkit_core::manifest::{BundledSource, DirSource, ManifestSource, PackageManifest};
use estzkit_core::provider::{provider_for, WingetProvider};
use estzkit_core::{
    build_plan, detect_package, index_by_name, installed_names, resolve, resume_plan as core_resume_plan, run_plan,
    run_plan_cancellable, DetectedPackage, EventSink, InstallPlan, PackageSummary, Profile, ProviderKind, StepEvent,
    StepState, Store, StoredStep,
};
use tauri::{AppHandle, Emitter, Manager, State};

/// Shared application state managed by Tauri.
pub struct AppState {
    /// Manifest source (bundled, or `$ESTZKIT_MANIFESTS` on disk).
    pub source: Box<dyn ManifestSource>,
    /// SQLite store, behind a mutex (Tauri commands take `&self`).
    pub store: Mutex<Store>,
    /// Plan ids for which cancellation was requested.
    pub cancelled: Mutex<HashSet<String>>,
}

impl AppState {
    /// Build state from the environment: `$ESTZKIT_MANIFESTS` selects a
    /// directory source, otherwise the bundled manifests are used.
    pub fn new() -> Self {
        let source: Box<dyn ManifestSource> = match DirSource::from_env() {
            Some(dir) => Box::new(dir),
            None => Box::new(BundledSource),
        };
        let store = Store::open_default().expect("open estzkit store");
        AppState {
            source,
            store: Mutex::new(store),
            cancelled: Mutex::new(HashSet::new()),
        }
    }

    fn packages(&self) -> Result<Vec<PackageManifest>, String> {
        self.source.packages().map_err(|e| e.to_string())
    }
}

/// Forwards [`StepEvent`]s to the frontend as the
/// `"install:state_changed"` event.
pub struct TauriEventSink {
    /// The Tauri app handle used to emit events.
    pub app: AppHandle,
}

impl EventSink for TauriEventSink {
    fn emit(&mut self, event: StepEvent) {
        let _ = self.app.emit("install:state_changed", event);
    }
}

/// Run detection for all package manifests.
#[tauri::command]
fn detect_installed(state: State<'_, AppState>) -> Result<Vec<DetectedPackage>, String> {
    let packages = state.packages()?;
    let winget = WingetProvider::new();
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for manifest in &packages {
        let result = detect_package(manifest, &winget);
        store
            .record_detected(&result.package)
            .map_err(|e| e.to_string())?;
        out.push(result.package);
    }
    Ok(out)
}

/// List profiles from the manifest source.
#[tauri::command]
fn list_profiles(state: State<'_, AppState>) -> Result<Vec<Profile>, String> {
    state
        .source
        .profiles()
        .map(|profiles| {
            profiles
                .into_iter()
                .map(|p| Profile {
                    name: p.name,
                    display_name: p.display_name,
                    packages: p.packages,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

/// List packages from the manifest source.
#[tauri::command]
fn list_packages(state: State<'_, AppState>) -> Result<Vec<PackageSummary>, String> {
    state
        .packages()
        .map(|packages| {
            packages
                .into_iter()
                .map(|p| PackageSummary {
                    name: p.name,
                    category: p.category,
                    winget_id: p
                        .detect
                        .as_ref()
                        .and_then(|d| d.winget_id.clone())
                        .or_else(|| p.windows.as_ref().map(|w| w.package.clone())),
                })
                .collect()
        })
}

/// Build (but do not execute) an install plan for `packages`.
#[tauri::command]
fn plan_install(
    state: State<'_, AppState>,
    packages: Vec<String>,
) -> Result<InstallPlan, String> {
    let plan = build_plan_for(&state, &packages)?;
    Ok(plan)
}

/// Execute an install plan, streaming `StepEvent`s to the frontend.
#[tauri::command]
fn execute_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    plan: InstallPlan,
    packages: Vec<String>,
) -> Result<(), String> {
    let manifests = state.packages()?;
    let by_name = index_by_name(&manifests);
    let provider = provider_for(ProviderKind::Winget);
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let mut sink = TauriEventSink { app };
    // The plan's package list is authoritative; `packages` is accepted for
    // symmetry with plan_install and validated for existence.
    for name in &packages {
        if !by_name.contains_key(name) {
            return Err(format!("unknown package: {name}"));
        }
    }
    let plan_id = plan.id.clone();
    run_plan_cancellable(&plan, &by_name, provider.as_ref(), &mut store, &mut sink, &|| {
        state
            .cancelled
            .lock()
            .map(|c| c.contains(&plan_id))
            .unwrap_or(false)
    })
    .map_err(|e| e.to_string())
}

/// Resume a previously halted plan (e.g. after a reboot).
#[tauri::command]
fn resume_plan(
    app: AppHandle,
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<(), String> {
    let manifests = state.packages()?;
    let by_name = index_by_name(&manifests);
    let provider = provider_for(ProviderKind::Winget);
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let mut sink = TauriEventSink { app };
    core_resume_plan(&plan_id, &by_name, provider.as_ref(), &mut store, &mut sink)
        .map_err(|e| e.to_string())
}

/// Request cancellation of a plan. The executor records the request; a
/// running plan checks it between steps in the future v2 (MVP: recorded).
#[tauri::command]
fn cancel_plan(state: State<'_, AppState>, plan_id: String) -> Result<(), String> {
    state
        .cancelled
        .lock()
        .map_err(|e| e.to_string())?
        .insert(plan_id);
    Ok(())
}

/// Query stored steps for a plan (ordered by execution sequence).
#[tauri::command]
fn get_plan_steps(
    state: State<'_, AppState>,
    plan_id: String,
) -> Result<Vec<StoredStep>, String> {
    let store = state.store.lock().map_err(|e| e.to_string())?;
    store.steps_for_plan(&plan_id).map_err(|e| e.to_string())
}

/// Retry a single failed/paused step: reinstall just that package.
#[tauri::command]
fn retry_step(
    app: AppHandle,
    state: State<'_, AppState>,
    plan_id: String,
    package_id: String,
) -> Result<StepState, String> {
    let manifests = state.packages()?;
    let by_name = index_by_name(&manifests);
    let plan = build_plan_for(&state, &[package_id.clone()])?;
    let provider = provider_for(ProviderKind::Winget);
    let mut store = state.store.lock().map_err(|e| e.to_string())?;
    let mut sink = TauriEventSink { app };
    // Reuse the executor for a one-step plan; the resulting state is read
    // back from the store.
    let single = InstallPlan {
        id: plan_id.clone(),
        package_ids: plan.package_ids,
        steps: plan
            .steps
            .into_iter()
            .filter(|s| s.package_id == package_id)
            .collect(),
        uac_warnings: 0,
    };
    run_plan(&single, &by_name, provider.as_ref(), &mut store, &mut sink).map_err(|e| e.to_string())?;
    let persisted = store
        .step_state(&plan_id, &package_id)
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    Ok(match persisted.as_str() {
        "Completed" => StepState::Completed,
        "Failed" => StepState::Failed,
        "Paused" => StepState::Paused,
        "RebootRequired" => StepState::RebootRequired,
        _ => StepState::Pending,
    })
}

/// Shared helper: resolve + detect + build an install plan.
fn build_plan_for(state: &AppState, packages: &[String]) -> Result<InstallPlan, String> {
    let manifests = state.packages()?;
    let by_name: HashMap<String, PackageManifest> = index_by_name(&manifests);
    let ordered = resolve(packages, &by_name).map_err(map_err)?;
    let winget = WingetProvider::new();
    let detected: Vec<DetectedPackage> = ordered
        .iter()
        .map(|m| detect_package(m, &winget).package)
        .collect();
    Ok(build_plan(&ordered, &installed_names(&detected)))
}

/// Translate a core error into the string error Tauri commands return.
fn map_err(e: estzkit_core::CoreError) -> String {
    e.to_string()
}

/// Entry point used by `main.rs`.
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(AppState::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            detect_installed,
            list_profiles,
            list_packages,
            plan_install,
            execute_plan,
            cancel_plan,
            retry_step,
            get_plan_steps,
            resume_plan
        ])
        .run(tauri::generate_context!())
        .expect("error while running EstzKit");
}
