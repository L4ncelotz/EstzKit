//! Plan execution: run plan steps in order, emitting and persisting each
//! transition, verifying after install, and mapping process exit codes.

use std::collections::{HashMap, HashSet};
use std::thread;
use std::time::Duration;

use crate::error::{CoreError, CoreResult};
use crate::events::EventSink;
use crate::manifest::{InstallStrategy, PackageManifest};
use crate::planner::remaining_plan_from_steps;
use crate::provider::{ExitCode, Provider};
use crate::store::Store;
use crate::verify::run_verify;
use crate::{InstallPlan, StepEvent, StepState};

/// Map a provider exit code to a step state:
///
/// - `0` → [`StepState::Completed`]
/// - anything else → [`StepState::Failed`]
pub fn map_exit_code(code: i32) -> StepState {
    match code {
        0 => StepState::Completed,
        _ => StepState::Failed,
    }
}

/// Backoff delays between install retries (seconds): 1s, 2s, 4s.
const RETRY_BACKOFF_SECS: [u64; 3] = [1, 2, 4];

/// Execute a plan: for each step, emit
/// `Pending → Resolving → (Downloading/Installing) → Verifying → Completed`,
/// persist every transition to `store`, and retry failed installs up to
/// three times with backoff. The plan halts after a `RebootRequired` step.
pub fn run_plan(
    plan: &InstallPlan,
    by_name: &HashMap<String, PackageManifest>,
    provider: &dyn Provider,
    store: &mut Store,
    sink: &mut dyn EventSink,
) -> CoreResult<()> {
    store.create_plan(plan)?;

    let emit = |sink: &mut dyn EventSink,
                store: &mut Store,
                package_id: &str,
                state: StepState,
                message: Option<String>,
                error: Option<String>,
                exit_code: Option<i32>| {
        sink.emit(StepEvent {
            plan_id: plan.id.clone(),
            package_id: package_id.to_string(),
            state,
            message: message.clone(),
            error: error.clone(),
        });
        let detail = message.as_deref().or(error.as_deref());
        let _ = store.record_step(&plan.id, package_id, state, exit_code, detail);
    };

    let mut halted = false;
    let mut failed_packages = HashSet::new();
    let mut any_failed = false;
    for step in &plan.steps {
        if halted {
            continue;
        }
        if step.state == StepState::Skipped {
            emit(
                sink,
                store,
                &step.package_id,
                StepState::Skipped,
                Some("already installed".to_string()),
                None,
                None,
            );
            continue;
        }

        let manifest = match by_name.get(&step.package_id) {
            Some(m) => m,
            None => {
                let msg = format!("unknown package {}", step.package_id);
                emit(sink, store, &step.package_id, StepState::Failed, None, Some(msg), None);
                failed_packages.insert(step.package_id.clone());
                any_failed = true;
                continue;
            }
        };

        if let Some(failed_dep) = manifest.depends_on.iter().find(|dep| failed_packages.contains(*dep)) {
            let msg = format!("dependency '{failed_dep}' failed");
            emit(sink, store, &step.package_id, StepState::Failed, None, Some(msg), None);
            failed_packages.insert(step.package_id.clone());
            any_failed = true;
            continue;
        }

        emit(
            sink,
            store,
            &step.package_id,
            StepState::Pending,
            None,
            None,
            None,
        );
        emit(
            sink,
            store,
            &step.package_id,
            StepState::Resolving,
            None,
            None,
            None,
        );

        if manifest.install.strategy != InstallStrategy::PackageManager {
            let msg = format!(
                "unsupported strategy {:?} for {} (MVP accepts package_manager only)",
                manifest.install.strategy, step.package_id
            );
            emit(sink, store, &step.package_id, StepState::Failed, None, Some(msg), None);
            failed_packages.insert(step.package_id.clone());
            any_failed = true;
            continue;
        }
        let spec = match manifest.os_provider_for_current() {
            Some(s) => s,
            None => {
                let msg = format!("no provider for {} on this OS", step.package_id);
                emit(sink, store, &step.package_id, StepState::Failed, None, Some(msg), None);
                failed_packages.insert(step.package_id.clone());
                any_failed = true;
                continue;
            }
        };

        match install_with_retry(spec, step.scope, sink, &step.package_id, provider) {
            Ok(code) => {
                let state = map_exit_code(code.0);
                match state {
                    StepState::Completed => match run_verify(&manifest.verify) {
                        Ok(true) => emit(
                            sink,
                            store,
                            &step.package_id,
                            StepState::Completed,
                            Some("installed and verified".to_string()),
                            None,
                            Some(code.0),
                        ),
                        Ok(false) => {
                            emit(
                                sink,
                                store,
                                &step.package_id,
                                StepState::Failed,
                                None,
                                Some("verify command did not match expected output".to_string()),
                                Some(code.0),
                            );
                            failed_packages.insert(step.package_id.clone());
                            any_failed = true;
                        }
                        Err(e) => {
                            emit(
                                sink,
                                store,
                                &step.package_id,
                                StepState::Failed,
                                None,
                                Some(e.to_string()),
                                Some(code.0),
                            );
                            failed_packages.insert(step.package_id.clone());
                            any_failed = true;
                        }
                    },
                    StepState::Paused => {
                        emit(
                            sink,
                            store,
                            &step.package_id,
                            StepState::Paused,
                            Some("UAC cancelled - retry when ready".to_string()),
                            None,
                            Some(code.0),
                        );
                        failed_packages.insert(step.package_id.clone());
                        any_failed = true;
                    }
                    StepState::RebootRequired => {
                        emit(
                            sink,
                            store,
                            &step.package_id,
                            StepState::RebootRequired,
                            Some("reboot required after install".to_string()),
                            None,
                            Some(code.0),
                        );
                        halted = true;
                        failed_packages.insert(step.package_id.clone());
                    }
                    _ => {
                        emit(
                            sink,
                            store,
                            &step.package_id,
                            StepState::Failed,
                            None,
                            Some(format!("install exited with code {}", code.0)),
                            Some(code.0),
                        );
                        failed_packages.insert(step.package_id.clone());
                        any_failed = true;
                    }
                }
            }
            Err(e) => {
                emit(
                    sink,
                    store,
                    &step.package_id,
                    StepState::Failed,
                    None,
                    Some(e.to_string()),
                    None,
                );
                failed_packages.insert(step.package_id.clone());
                any_failed = true;
            }
        }
    }

    if halted {
        store.set_plan_status(&plan.id, "reboot_required")?;
    } else if any_failed {
        store.set_plan_status(&plan.id, "failed")?;
    } else {
        store.set_plan_status(&plan.id, "completed")?;
    }
    Ok(())
}

/// Resume an existing plan: load persisted steps from `store`, build an
/// [`InstallPlan`] containing only the remaining non-completed steps, and
/// run them.
pub fn resume_plan(
    plan_id: &str,
    by_name: &HashMap<String, PackageManifest>,
    provider: &dyn Provider,
    store: &mut Store,
    sink: &mut dyn EventSink,
) -> CoreResult<()> {
    let steps = store.steps_for_plan(plan_id)?;
    let remaining = remaining_plan_from_steps(&steps, plan_id)
        .ok_or_else(|| CoreError::PlanNotFound(plan_id.to_string()))?;
    run_plan(&remaining, by_name, provider, store, sink)
}

/// Whether a nonzero provider exit should be retried automatically.
/// `scope: machine` may elevate (UAC/reboot/installer): a user "No" or a
/// reboot-required signal must surface after one attempt, not re-trigger
/// the installer. `scope: user` failures are retryable (transient fetch).
/// Interim until observed mapping lands (see ADR-011): `map_exit_code`
/// still maps every nonzero to `Failed`; this only suppresses auto-retry.
fn should_retry(scope: crate::manifest::InstallScope, code: i32) -> bool {
    code != 0 && matches!(scope, crate::manifest::InstallScope::User)
}

/// Run one install, retrying nonzero exits up to three times with
/// 1s/2s/4s backoff for `scope: user`. `scope: machine` exits are
/// single-attempt (no re-trigger of UAC/reboot installers). If
/// `provider.install` returns an error, that error is surfaced after the
/// final attempt.
fn install_with_retry(
    spec: &crate::manifest::OsProvider,
    scope: crate::manifest::InstallScope,
    sink: &mut dyn EventSink,
    package_id: &str,
    provider: &dyn Provider,
) -> CoreResult<ExitCode> {
    let mut last_err: Option<CoreError> = None;
    for attempt in 0..=RETRY_BACKOFF_SECS.len() {
        if attempt > 0 {
            thread::sleep(Duration::from_secs(RETRY_BACKOFF_SECS[attempt - 1]));
        }
        match provider.install(spec, scope, sink, package_id) {
            Ok(code) => {
                if code.0 == 0 {
                    return Ok(code);
                }
                if attempt == RETRY_BACKOFF_SECS.len() || !should_retry(scope, code.0) {
                    // Final attempt (or machine-scope single attempt):
                    // surface the failure code.
                    return Ok(code);
                }
            }
            Err(e) => {
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| CoreError::ProcessSpawn("install failed".to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_mapping() {
        assert_eq!(map_exit_code(0), StepState::Completed);
        assert_eq!(map_exit_code(1223), StepState::Failed);
        assert_eq!(map_exit_code(3010), StepState::Failed);
        assert_eq!(map_exit_code(1), StepState::Failed);
        assert_eq!(map_exit_code(-1), StepState::Failed);
        assert_eq!(map_exit_code(1603), StepState::Failed);
    }

    #[test]
    fn machine_scope_nonzero_is_single_attempt() {
        use crate::manifest::InstallScope;
        assert!(!should_retry(InstallScope::Machine, 1223));
        assert!(!should_retry(InstallScope::Machine, 3010));
        assert!(!should_retry(InstallScope::Machine, 1));
        assert!(!should_retry(InstallScope::Machine, 0));
        assert!(!should_retry(InstallScope::User, 0));
        assert!(should_retry(InstallScope::User, 1223));
        assert!(should_retry(InstallScope::User, 3010));
        assert!(should_retry(InstallScope::User, 1));
    }

    #[test]
    fn resume_plan_unknown_plan_returns_not_found() {
        let mut store = Store::open_in_memory().expect("in-memory store");
        let by_name = HashMap::new();
        let provider = crate::provider::WingetProvider::new();
        let mut sink = crate::events::NullSink;
        let err = resume_plan("unknown-plan-999", &by_name, &provider, &mut store, &mut sink)
            .expect_err("unknown plan should error");
        match err {
            CoreError::PlanNotFound(id) => assert_eq!(id, "unknown-plan-999"),
            other => panic!("expected PlanNotFound, got {other:?}"),
        }
    }

    struct AlwaysFailProvider;
    impl Provider for AlwaysFailProvider {
        fn kind(&self) -> crate::manifest::ProviderKind {
            crate::manifest::ProviderKind::Winget
        }
        fn is_available(&self) -> bool {
            true
        }
        fn install(
            &self,
            _spec: &crate::manifest::OsProvider,
            _scope: crate::manifest::InstallScope,
            _sink: &mut dyn EventSink,
            _package_id: &str,
        ) -> CoreResult<ExitCode> {
            Err(CoreError::ProcessSpawn("always fails".to_string()))
        }
        fn uninstall(&self, _spec: &crate::manifest::OsProvider) -> CoreResult<()> {
            Ok(())
        }
    }

    #[test]
    fn retry_exhaustion_does_not_panic() {
        let spec = crate::manifest::OsProvider {
            provider: crate::manifest::ProviderKind::Winget,
            package: "test.pkg".to_string(),
            version: None,
        };
        let mut sink = crate::events::NullSink;
        let provider = AlwaysFailProvider;
        let res = install_with_retry(
            &spec,
            crate::manifest::InstallScope::User,
            &mut sink,
            "test-pkg",
            &provider,
        );
        assert!(res.is_err());
    }

    #[test]
    fn dependent_step_fails_when_dependency_fails() {
        use crate::manifest::{
            InstallScope, InstallSpec, InstallStrategy, OsProvider, PackageManifest, SecuritySpec,
            VerifySpec,
        };
        let mut by_name = HashMap::new();
        by_name.insert(
            "dep".to_string(),
            PackageManifest {
                schema_version: 1,
                name: "dep".to_string(),
                category: "tool".to_string(),
                windows: Some(OsProvider {
                    provider: crate::manifest::ProviderKind::Winget,
                    package: "Dep".to_string(),
                    version: None,
                }),
                macos: None,
                linux: None,
                depends_on: vec![],
                install: InstallSpec {
                    strategy: InstallStrategy::PackageManager,
                    scope: InstallScope::Machine,
                },
                verify: VerifySpec {
                    command: "dep".to_string(),
                    args: vec![],
                    expect_regex: None,
                },
                detect: None,
                security: SecuritySpec {
                    allow_scripts: false,
                    allow_elevation: true,
                },
            },
        );
        by_name.insert(
            "child".to_string(),
            PackageManifest {
                schema_version: 1,
                name: "child".to_string(),
                category: "tool".to_string(),
                windows: Some(OsProvider {
                    provider: crate::manifest::ProviderKind::Winget,
                    package: "Child".to_string(),
                    version: None,
                }),
                macos: None,
                linux: None,
                depends_on: vec!["dep".to_string()],
                install: InstallSpec {
                    strategy: InstallStrategy::PackageManager,
                    scope: InstallScope::Machine,
                },
                verify: VerifySpec {
                    command: "child".to_string(),
                    args: vec![],
                    expect_regex: None,
                },
                detect: None,
                security: SecuritySpec {
                    allow_scripts: false,
                    allow_elevation: true,
                },
            },
        );
        let plan = InstallPlan {
            id: "test-cascade".to_string(),
            package_ids: vec!["dep".to_string(), "child".to_string()],
            steps: vec![
                crate::PlanStep {
                    package_id: "dep".to_string(),
                    depends_on: vec![],
                    scope: InstallScope::Machine,
                    state: StepState::Pending,
                    message: None,
                },
                crate::PlanStep {
                    package_id: "child".to_string(),
                    depends_on: vec!["dep".to_string()],
                    scope: InstallScope::Machine,
                    state: StepState::Pending,
                    message: None,
                },
            ],
            uac_warnings: 2,
        };
        let mut store = Store::open_in_memory().expect("in-memory store");
        let provider = AlwaysFailProvider;
        let mut sink = crate::events::NullSink;
        run_plan(&plan, &by_name, &provider, &mut store, &mut sink).expect("run plan");
        let steps = store.steps_for_plan("test-cascade").expect("steps");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].package_id, "dep");
        assert_eq!(steps[0].state, "Failed");
        assert_eq!(steps[1].package_id, "child");
        assert_eq!(steps[1].state, "Failed");
        assert!(steps[1]
            .message
            .as_deref()
            .unwrap()
            .contains("dependency 'dep' failed"));
    }
}
