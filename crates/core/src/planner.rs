//! Plan building: turn an ordered manifest list into an [`InstallPlan`].

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::manifest::PackageManifest;
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
    }

    #[test]
    fn empty_plan_has_no_warnings() {
        let plan = build_plan(&[], &HashSet::new());
        assert_eq!(plan.uac_warnings, 0);
        assert!(plan.steps.is_empty());
    }
}
