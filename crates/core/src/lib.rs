//! EstzKit core: manifests, detection, planning, and execution.
//!
//! This crate is pure Rust and knows nothing about Tauri. Hosts (the Tauri
//! IPC layer, the CLI) implement [`events::EventSink`] to receive
//! [`StepEvent`] transitions; the Tauri host forwards them as the
//! `"install:state_changed"` event.

pub mod detect;
pub mod error;
pub mod events;
pub mod executor;
pub mod manifest;
pub mod planner;
pub mod provider;
pub mod resolver;
pub mod store;
pub mod verify;

pub use detect::{detect_package, DetectJob, DetectResult};
pub use error::{CoreError, CoreResult};
pub use events::{CollectSink, EventSink, NullSink, INSTALL_STATE_CHANGED_EVENT};
pub use executor::{map_exit_code, resume_plan, run_plan, EXIT_CODE_REBOOT_REQUIRED, EXIT_CODE_UAC_CANCELLED};
pub use manifest::{
    index_by_name, installed_names, is_valid_name, parse_package_manifest,
    parse_profile_manifest, DetectSpec, DirSource, InstallSpec, InstallStrategy, ManifestSource,
    OsProvider, PackageManifest, ProfileManifest, ProviderKind, SecuritySpec, VerifySpec,
    BundledSource, MANIFESTS_ENV_VAR, SCHEMA_VERSION,
};
pub use planner::{build_plan, remaining_plan_from_steps};
pub use provider::{provider_for, AptProvider, BrewProvider, ExitCode, Provider, WingetProvider};
pub use resolver::resolve;
pub use store::{Store, StoredStep, STORE_SCHEMA_VERSION};
pub use verify::{output_matches, run_combined, run_verify};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Detection summary for one package (shown in the Detect table).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct DetectedPackage {
    /// Package identifier.
    pub name: String,
    /// Whether the package was found on this machine.
    pub found: bool,
    /// Detected version, when known.
    pub version: Option<String>,
    /// Which detection layer answered (e.g. `"detect/verify"`, `"winget"`).
    pub source: String,
}

/// A profile offered to the user (from `manifests/profiles/*.yaml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// Profile identifier.
    pub name: String,
    /// Human-facing display name.
    pub display_name: String,
    /// Package names in this profile.
    pub packages: Vec<String>,
}

/// One package shown in the package picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PackageSummary {
    /// Package identifier.
    pub name: String,
    /// Human-facing category.
    pub category: String,
    /// winget id, when the manifest declares one.
    pub winget_id: Option<String>,
}

/// An install plan ready for execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    /// Unique plan id.
    pub id: String,
    /// All package ids in the plan, in install order.
    pub package_ids: Vec<String>,
    /// Steps, in install order.
    pub steps: Vec<PlanStep>,
    /// How many steps are expected to prompt UAC.
    pub uac_warnings: usize,
}

/// One step of an install plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    /// Package to install.
    pub package_id: String,
    /// Names of packages that must be installed first.
    pub depends_on: Vec<String>,
    /// Install scope for this step.
    pub scope: manifest::InstallScope,
    /// Initial step state (`Pending`, or `Skipped` when already installed).
    pub state: StepState,
    /// Optional status or skip reason message.
    #[serde(default)]
    pub message: Option<String>,
}

/// Lifecycle state of one plan step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "PascalCase")]
pub enum StepState {
    /// Waiting to run.
    Pending,
    /// Resolving the install specification.
    Resolving,
    /// Downloading the package.
    Downloading,
    /// Installing the package.
    Installing,
    /// Verifying the installation.
    Verifying,
    /// Installed and verified.
    Completed,
    /// Installation failed.
    Failed,
    /// Paused (e.g. UAC cancelled); safe to retry.
    Paused,
    /// Installed but a reboot is required; the plan is halted.
    RebootRequired,
    /// Not installed because the package is already present.
    Skipped,
}

impl StepState {
    /// Stable string spelling used in persistence and logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            StepState::Pending => "Pending",
            StepState::Resolving => "Resolving",
            StepState::Downloading => "Downloading",
            StepState::Installing => "Installing",
            StepState::Verifying => "Verifying",
            StepState::Completed => "Completed",
            StepState::Failed => "Failed",
            StepState::Paused => "Paused",
            StepState::RebootRequired => "RebootRequired",
            StepState::Skipped => "Skipped",
        }
    }
}

/// One progress event for a plan step. Delivered to the host via
/// [`EventSink`]; the Tauri host re-emits this as the
/// `"install:state_changed"` event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StepEvent {
    /// Plan the step belongs to.
    pub plan_id: String,
    /// Package this event is about.
    pub package_id: String,
    /// New step state.
    pub state: StepState,
    /// Optional progress message (e.g. a winget output line).
    #[serde(default)]
    pub message: Option<String>,
    /// Optional error message for failed steps.
    #[serde(default)]
    pub error: Option<String>,
}

/// Detection outcome for one package (externally tagged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum DetectStatus {
    /// The package is not installed.
    Missing,
    /// The package is installed (optionally with its version).
    InstalledOk {
        /// Detected version, when known.
        version: Option<String>,
    },
    /// The package is installed, but at a different version than expected.
    VersionMismatch {
        /// Expected (pinned) version.
        expected: String,
        /// Actual detected version.
        actual: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_state_spellings() {
        assert_eq!(StepState::Pending.as_str(), "Pending");
        assert_eq!(StepState::RebootRequired.as_str(), "RebootRequired");
        assert_eq!(StepState::Completed.as_str(), "Completed");
    }

    #[test]
    fn detect_status_serializes_externally_tagged() {
        let json = serde_json::to_value(DetectStatus::Missing).unwrap();
        assert_eq!(json, serde_json::json!("Missing"));
        let json = serde_json::to_value(DetectStatus::InstalledOk { version: None }).unwrap();
        assert_eq!(json, serde_json::json!({"InstalledOk": {"version": null}}));
        let json = serde_json::to_value(DetectStatus::VersionMismatch {
            expected: "1.0".to_string(),
            actual: "2.0".to_string(),
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"VersionMismatch": {"expected": "1.0", "actual": "2.0"}})
        );
    }

    #[test]
    fn step_event_round_trips_camel_case() {
        let event = StepEvent {
            plan_id: "p".to_string(),
            package_id: "node".to_string(),
            state: StepState::Installing,
            message: Some("m".to_string()),
            error: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["planId"], "p");
        assert_eq!(json["packageId"], "node");
        assert_eq!(json["state"], "Installing");
        let back: StepEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }
}
