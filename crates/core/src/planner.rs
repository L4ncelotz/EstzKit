//! Plan building: turn an ordered manifest list into an [`InstallPlan`].

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::manifest::PackageManifest;
use crate::store::StoredStep;
use crate::{InstallPlan, PlanStep, StepState};

/// Build an install plan from resolved (dependency-ordered) manifests.
///
/// Packages in `already` are emitted as [`StepState::Skipped`] steps so the
/// executor reports them without reinstalling. `uac_warnings` counts the
/// remaining steps whose manifest sets `allow_elevation: true`.
pub fn build_plan(ordered: &[PackageManifest], already: &HashSet<String>) -> InstallPlan {
    let mut steps = Vec::new();
    let mut uac_warnings = 0_usize;
    for manifest in ordered {
        let skipped = already.contains(&manifest.name);
        if !skipped && manifest.security.allow_elevation {
            uac_warnings += 1;
        }
        steps.push(PlanStep {
            package_id: manifest.name.clone(),
            depends_on: manifest.depends_on.clone(),
            scope: manifest.install.scope,
            state: if skipped {
                StepState::Skipped
            } else {
                StepState::Pending
            },
            message: if skipped {
                Some("already installed (installed_ok) — not reinstalled".to_string())
            } else {
                None
            },
        });
    }
    InstallPlan {
        id: new_plan_id(),
        package_ids: ordered
            .iter()
            .map(|m| m.name.clone())
            .collect(),
        steps,
        uac_warnings,
    }
}

/// Generate a short unique plan id from the current time in nanoseconds.
fn new_plan_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("plan-{nanos:x}")
}

/// Reconstruct an [`InstallPlan`] containing only the remaining non-completed
/// steps from a persisted plan's steps.
///
/// Steps in state `Pending`, `Failed`, `Paused`, or `RebootRequired` are kept
/// in their original order. `Completed` and `Skipped` steps are excluded.
/// Returns `None` if `steps` is empty or no steps need execution.
pub fn remaining_plan_from_steps(steps: &[StoredStep], plan_id: &str) -> Option<InstallPlan> {
    if steps.is_empty() {
        return None;
    }
    let mut remaining = Vec::new();
    let mut package_ids = Vec::new();
    for s in steps {
        let is_remaining = matches!(
            s.state.as_str(),
            "Pending" | "Failed" | "Paused" | "RebootRequired"
        );
        if is_remaining {
            package_ids.push(s.package_id.clone());
            remaining.push(PlanStep {
                package_id: s.package_id.clone(),
                depends_on: Vec::new(),
                scope: crate::manifest::InstallScope::User,
                state: match s.state.as_str() {
                    "Failed" => StepState::Failed,
                    "Paused" => StepState::Paused,
                    "RebootRequired" => StepState::RebootRequired,
                    _ => StepState::Pending,
                },
                message: s.message.clone(),
            });
        }
    }
    if remaining.is_empty() {
        None
    } else {
        Some(InstallPlan {
            id: plan_id.to_string(),
            package_ids,
            steps: remaining,
            uac_warnings: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{InstallSpec, InstallStrategy, InstallScope, SecuritySpec, VerifySpec};

    fn manifest(name: &str, allow_elevation: bool) -> PackageManifest {
        PackageManifest {
            schema_version: 1,
            name: name.to_string(),
            category: "runtime".to_string(),
            windows: None,
            macos: None,
            linux: None,
            depends_on: vec![],
            install: InstallSpec {
                strategy: InstallStrategy::PackageManager,
                scope: InstallScope::User,
            },
            verify: VerifySpec {
                command: "echo".to_string(),
                args: vec![],
                expect_regex: None,
            },
            detect: None,
            security: SecuritySpec {
                allow_scripts: false,
                allow_elevation,
            },
        }
    }

    #[test]
    fn counts_uac_warnings_and_skips_installed() {
        let ordered = vec![manifest("a", true), manifest("b", false), manifest("c", true)];
        let already: HashSet<String> = ["c".to_string()].into_iter().collect();
        let plan = build_plan(&ordered, &already);
        assert_eq!(plan.uac_warnings, 1);
        assert_eq!(plan.package_ids.len(), 3);
        let skipped: Vec<&str> = plan
            .steps
            .iter()
            .filter(|s| s.state == StepState::Skipped)
            .map(|s| s.package_id.as_str())
            .collect();
        assert_eq!(skipped, vec!["c"]);
        assert!(plan.steps.iter().all(|s| s.state != StepState::Skipped || s.package_id == "c"));
        let step_c = plan.steps.iter().find(|s| s.package_id == "c").unwrap();
        assert_eq!(
            step_c.message.as_deref(),
            Some("already installed (installed_ok) — not reinstalled")
        );
        let step_a = plan.steps.iter().find(|s| s.package_id == "a").unwrap();
        assert_eq!(step_a.message, None);
    }

    #[test]
    fn empty_plan_has_no_warnings() {
        let plan = build_plan(&[], &HashSet::new());
        assert_eq!(plan.uac_warnings, 0);
        assert!(plan.steps.is_empty());
    }

    #[test]
    fn remaining_plan_filters_completed_and_skipped() {
        let fixture = vec![
            StoredStep {
                package_id: "a".to_string(),
                state: "Completed".to_string(),
                exit_code: Some(0),
                message: Some("ok".to_string()),
            },
            StoredStep {
                package_id: "b".to_string(),
                state: "Failed".to_string(),
                exit_code: Some(1),
                message: Some("err".to_string()),
            },
            StoredStep {
                package_id: "c".to_string(),
                state: "RebootRequired".to_string(),
                exit_code: Some(3010),
                message: Some("reboot".to_string()),
            },
            StoredStep {
                package_id: "d".to_string(),
                state: "Skipped".to_string(),
                exit_code: None,
                message: Some("skip".to_string()),
            },
        ];
        let plan = remaining_plan_from_steps(&fixture, "p-123").expect("remaining plan");
        assert_eq!(plan.id, "p-123");
        assert_eq!(plan.package_ids, vec!["b", "c"]);
        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].package_id, "b");
        assert_eq!(plan.steps[0].state, StepState::Failed);
        assert_eq!(plan.steps[1].package_id, "c");
        assert_eq!(plan.steps[1].state, StepState::RebootRequired);
        assert_eq!(plan.uac_warnings, 0);

        assert!(remaining_plan_from_steps(&[], "p-123").is_none());
        let all_done = vec![StoredStep {
            package_id: "a".to_string(),
            state: "Completed".to_string(),
            exit_code: Some(0),
            message: None,
        }];
        assert!(remaining_plan_from_steps(&all_done, "p-123").is_none());
    }
}
