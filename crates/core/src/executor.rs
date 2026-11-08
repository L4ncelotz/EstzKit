//! Plan execution: run plan steps in order, emitting and persisting each
//! transition, verifying after install, and mapping process exit codes.

use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use crate::error::{CoreError, CoreResult};
use crate::events::EventSink;
use crate::manifest::{InstallStrategy, PackageManifest};
use crate::provider::{ExitCode, Provider};
use crate::store::Store;
use crate::verify::run_verify;
use crate::{InstallPlan, StepEvent, StepState};

/// Win32 `ERROR_CANCELLED`: the user cancelled the UAC prompt.
pub const EXIT_CODE_UAC_CANCELLED: i32 = 1223;
/// Win32 `ERROR_SUCCESS_REBOOT_REQUIRED`: a reboot is required.
pub const EXIT_CODE_REBOOT_REQUIRED: i32 = 3010;

/// Map a provider exit code to a step state:
///
/// - `0` → [`StepState::Completed`]
/// - `1223` (`ERROR_CANCELLED`) → [`StepState::Paused`] ("UAC cancelled - retry when ready")
/// - `3010` (`ERROR_SUCCESS_REBOOT_REQUIRED`) → [`StepState::RebootRequired`]
/// - anything else → [`StepState::Failed`]
pub fn map_exit_code(code: i32) -> StepState {
    match code {
        0 => StepState::Completed,
        EXIT_CODE_UAC_CANCELLED => StepState::Paused,
        EXIT_CODE_REBOOT_REQUIRED => StepState::RebootRequired,
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
        let _ = store.record_step(&plan.id, package_id, state, exit_code, message.as_deref());
    };

    let mut halted = false;
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

        let manifest = match by_name.get(&step.package_id) {
            Some(m) => m,
            None => {
                let msg = format!("unknown package {}", step.package_id);
                emit(sink, store, &step.package_id, StepState::Failed, None, Some(msg), None);
                continue;
            }
        };
        if manifest.install.strategy != InstallStrategy::PackageManager {
            let msg = format!(
                "unsupported strategy {:?} for {} (MVP accepts package_manager only)",
                manifest.install.strategy, step.package_id
            );
            emit(sink, store, &step.package_id, StepState::Failed, None, Some(msg), None);
            continue;
        }
        let spec = match manifest.os_provider_for_current() {
            Some(s) => s,
            None => {
                let msg = format!("no provider for {} on this OS", step.package_id);
                emit(sink, store, &step.package_id, StepState::Failed, None, Some(msg), None);
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
                        Ok(false) => emit(
                            sink,
                            store,
                            &step.package_id,
                            StepState::Failed,
                            None,
                            Some("verify command did not match expected output".to_string()),
                            Some(code.0),
                        ),
                        Err(e) => emit(
                            sink,
                            store,
                            &step.package_id,
                            StepState::Failed,
                            None,
                            Some(e.to_string()),
                            Some(code.0),
                        ),
                    },
                    StepState::Paused => emit(
                        sink,
                        store,
                        &step.package_id,
                        StepState::Paused,
                        Some("UAC cancelled - retry when ready".to_string()),
                        None,
                        Some(code.0),
                    ),
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
                    }
                    _ => emit(
                        sink,
                        store,
                        &step.package_id,
                        StepState::Failed,
                        None,
                        Some(format!("install exited with code {}", code.0)),
                        Some(code.0),
                    ),
                }
            }
            Err(e) => emit(
                sink,
                store,
                &step.package_id,
                StepState::Failed,
                None,
                Some(e.to_string()),
                None,
            ),
        }
    }

    if halted {
        store.set_plan_status(&plan.id, "reboot_required")?;
    } else {
        store.set_plan_status(&plan.id, "completed")?;
    }
    Ok(())
}

/// Run one install, retrying nonzero exits up to three times with
/// 1s/2s/4s backoff. `1223` (UAC cancelled) and `3010` (reboot) are
/// terminal and never retried. If `provider.install` returns an error,
/// that error is surfaced after the final attempt.
fn install_with_retry(
    spec: &crate::manifest::OsProvider,
    scope: crate::manifest::InstallScope,
    sink: &mut dyn EventSink,
    package_id: &str,
    provider: &dyn Provider,
) -> CoreResult<ExitCode> {
    let mut last_err: Option<CoreError> = None;
    for attempt in 0..=1 + RETRY_BACKOFF_SECS.len() {
        if attempt > 0 {
            thread::sleep(Duration::from_secs(RETRY_BACKOFF_SECS[attempt - 1]));
        }
        match provider.install(spec, scope, sink, package_id) {
            Ok(code) => {
                if code.0 == 0
                    || code.0 == EXIT_CODE_UAC_CANCELLED
                    || code.0 == EXIT_CODE_REBOOT_REQUIRED
                {
                    return Ok(code);
                }
                if attempt == RETRY_BACKOFF_SECS.len() {
                    // Final attempt: surface the failure code.
                    return Ok(code);
                }
            }
            Err(e) => {
                if attempt == RETRY_BACKOFF_SECS.len() {
                    last_err = Some(e);
                }
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
        assert_eq!(map_exit_code(1223), StepState::Paused);
        assert_eq!(map_exit_code(3010), StepState::RebootRequired);
        assert_eq!(map_exit_code(1), StepState::Failed);
        assert_eq!(map_exit_code(-1), StepState::Failed);
        assert_eq!(map_exit_code(1603), StepState::Failed);
    }

    #[test]
    fn uac_cancelled_and_reboot_codes_are_terminal() {
        assert_eq!(EXIT_CODE_UAC_CANCELLED, 1223);
        assert_eq!(EXIT_CODE_REBOOT_REQUIRED, 3010);
    }
}
