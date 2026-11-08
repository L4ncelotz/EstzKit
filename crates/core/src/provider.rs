//! Package-manager providers. The MVP implements `winget`; brew/apt
//! providers report `is_available() == false` (v2).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use crate::error::{CoreError, CoreResult};
use crate::events::EventSink;
use crate::manifest::{InstallScope, OsProvider, ProviderKind};
use crate::{DetectStatus, StepEvent, StepState};

/// Exit code returned by a provider install run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitCode(pub i32);

/// A package-manager provider.
pub trait Provider {
    /// Which package manager this provider implements.
    fn kind(&self) -> ProviderKind;
    /// Whether the package manager is usable on this machine.
    fn is_available(&self) -> bool;
    /// Install `spec` (the manifest's OS provider binding) at `scope`,
    /// streaming progress to `sink`. Returns the process exit code.
    fn install(
        &self,
        spec: &OsProvider,
        scope: InstallScope,
        sink: &mut dyn EventSink,
        package_id: &str,
    ) -> CoreResult<ExitCode>;
    /// Uninstall `spec` from this machine.
    fn uninstall(&self, spec: &OsProvider) -> CoreResult<()>;
}

/// winget-backed provider.
///
/// Availability is `winget --version` succeeding; installation runs
/// `winget install` with silent/accept flags and streams its output to the
/// event sink. `is_installed` queries `winget list --id <id> --exact` and
/// caches the answer for the session.
#[derive(Debug, Default)]
pub struct WingetProvider {
    cache: RefCell<HashMap<String, DetectStatus>>,
}

impl WingetProvider {
    /// Create a new winget provider with an empty installed-cache.
    pub fn new() -> Self {
        WingetProvider { cache: RefCell::new(HashMap::new()) }
    }

    /// Check whether `winget_id` is installed via `winget list`.
    pub fn is_installed(&self, winget_id: &str) -> DetectStatus {
        if let Some(cached) = self.cache.borrow().get(winget_id) {
            return cached.clone();
        }
        let status = Self::query_installed(winget_id);
        self.cache.borrow_mut().insert(winget_id.to_string(), status.clone());
        status
    }

    fn query_installed(winget_id: &str) -> DetectStatus {
        let output = Command::new("winget")
            .args(["list", "--id", winget_id, "--exact", "--accept-source-agreements"])
            .output();
        let output = match output {
            Ok(o) => o,
            Err(_) => return DetectStatus::Missing,
        };
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        parse_winget_list(&stdout, output.status.success(), winget_id)
    }
}

/// Parse `winget list --id <id> --exact` output into a [`DetectStatus`].
///
/// Pure and unit-testable against golden files in `tests/fixtures/`.
/// Rules, in order (grounded by probed outputs — see `tests/fixtures/README.md`):
/// 1. Non-zero exit → [`DetectStatus::Missing`] (probed: not-found exits 20).
/// 2. Output contains "No installed package found" → `Missing` (defense in
///    depth for winget builds that exit 0 on empty results).
/// 3. A row whose tokens contain `winget_id` (case-insensitive) →
///    [`DetectStatus::InstalledOk`] with the token after the id as version.
/// 4. Anything else → `Missing`.
pub fn parse_winget_list(stdout: &str, status_success: bool, winget_id: &str) -> DetectStatus {
    if !status_success {
        return DetectStatus::Missing;
    }
    let lower = stdout.to_lowercase();
    if lower.contains("no installed package found") {
        return DetectStatus::Missing;
    }
    let id_lower = winget_id.to_lowercase();
    for line in stdout.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let Some(pos) = tokens.iter().position(|t| t.to_lowercase() == id_lower) else {
            continue;
        };
        // Row layout: Name Id Version [Available] [Source]; Name may contain
        // spaces but never the id token itself, so the token after the id is
        // the installed version.
        let version = tokens.get(pos + 1).map(|v| (*v).to_string());
        return DetectStatus::InstalledOk { version };
    }
    DetectStatus::Missing
}

impl Provider for WingetProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Winget
    }

    fn is_available(&self) -> bool {
        Command::new("winget")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn install(
        &self,
        spec: &OsProvider,
        scope: InstallScope,
        sink: &mut dyn EventSink,
        package_id: &str,
    ) -> CoreResult<ExitCode> {
        let mut cmd = Command::new("winget");
        cmd.args(["install", "--id", &spec.package, "--silent", "--accept-source-agreements", "--accept-package-agreements"]);
        if let Some(version) = &spec.version {
            cmd.args(["--version", version]);
        }
        cmd.args(["--scope", scope.as_str()]);
        cmd.stdout(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| CoreError::ProcessSpawn(format!("winget: {e}")))?;

        sink.emit(StepEvent {
            plan_id: String::new(),
            package_id: package_id.to_string(),
            state: StepState::Downloading,
            message: Some(format!("winget install --id {}", spec.package)),
            error: None,
        });
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => sink.emit(StepEvent {
                        plan_id: String::new(),
                        package_id: package_id.to_string(),
                        state: StepState::Installing,
                        message: Some(line),
                        error: None,
                    }),
                    _ => {}
                }
            }
        }
        let status = child.wait()?;
        // Refresh the session cache so later detect calls see the install.
        self.cache.borrow_mut().remove(&spec.package);
        Ok(ExitCode(status.code().unwrap_or(-1)))
    }

    fn uninstall(&self, spec: &OsProvider) -> CoreResult<()> {
        let status = Command::new("winget")
            .args(["uninstall", "--id", &spec.package, "--silent", "--accept-source-agreements", "--accept-package-agreements"])
            .status()
            .map_err(|e| CoreError::ProcessSpawn(format!("winget uninstall: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(CoreError::ProcessSpawn(format!(
                "winget uninstall exited with {}",
                status.code().unwrap_or(-1)
            )))
        }
    }
}

/// Homebrew provider. Not implemented in the MVP (v2): always unavailable.
#[derive(Debug, Default)]
pub struct BrewProvider;

impl Provider for BrewProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Brew
    }

    fn is_available(&self) -> bool {
        false
    }

    fn install(
        &self,
        _spec: &OsProvider,
        _scope: InstallScope,
        _sink: &mut dyn EventSink,
        package_id: &str,
    ) -> CoreResult<ExitCode> {
        Err(CoreError::UnsupportedStrategy(format!(
            "brew provider is not implemented in the MVP (package {package_id})"
        )))
    }

    fn uninstall(&self, _spec: &OsProvider) -> CoreResult<()> {
        Err(CoreError::UnsupportedStrategy(
            "brew provider is not implemented in the MVP".to_string(),
        ))
    }
}

/// APT provider. Not implemented in the MVP (v2): always unavailable.
#[derive(Debug, Default)]
pub struct AptProvider;

impl Provider for AptProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Apt
    }

    fn is_available(&self) -> bool {
        false
    }

    fn install(
        &self,
        _spec: &OsProvider,
        _scope: InstallScope,
        _sink: &mut dyn EventSink,
        package_id: &str,
    ) -> CoreResult<ExitCode> {
        Err(CoreError::UnsupportedStrategy(format!(
            "apt provider is not implemented in the MVP (package {package_id})"
        )))
    }

    fn uninstall(&self, _spec: &OsProvider) -> CoreResult<()> {
        Err(CoreError::UnsupportedStrategy(
            "apt provider is not implemented in the MVP".to_string(),
        ))
    }
}

/// Return a provider for the given package-manager kind.
pub fn provider_for(kind: ProviderKind) -> Box<dyn Provider> {
    match kind {
        ProviderKind::Winget => Box::new(WingetProvider::new()),
        ProviderKind::Brew => Box::new(BrewProvider),
        ProviderKind::Apt => Box::new(AptProvider),
    }
}

impl InstallScope {
    /// The `--scope` spelling winget expects.
    pub fn as_str(&self) -> &'static str {
        match self {
            InstallScope::User => "user",
            InstallScope::Machine => "machine",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brew_and_apt_are_unavailable_in_mvp() {
        assert!(!BrewProvider.is_available());
        assert!(!AptProvider.is_available());
        assert_eq!(BrewProvider.kind(), ProviderKind::Brew);
        assert_eq!(AptProvider.kind(), ProviderKind::Apt);
    }

    #[test]
    fn provider_for_returns_matching_kind() {
        assert_eq!(provider_for(ProviderKind::Winget).kind(), ProviderKind::Winget);
        assert_eq!(provider_for(ProviderKind::Brew).kind(), ProviderKind::Brew);
        assert_eq!(provider_for(ProviderKind::Apt).kind(), ProviderKind::Apt);
    }

    #[test]
    fn scope_spellings() {
        assert_eq!(InstallScope::User.as_str(), "user");
        assert_eq!(InstallScope::Machine.as_str(), "machine");
    }

    // Golden outputs captured read-only from the dev machine (winget
    // v1.29.380, en-US, 2026-11-08) — see tests/fixtures/README.md.
    const LIST_GIT_PRESENT: &str = include_str!("../tests/fixtures/winget_list_git_present.txt");
    const LIST_MISSING: &str = include_str!("../tests/fixtures/winget_list_missing.txt");

    #[test]
    fn parses_present_row_from_golden_fixture() {
        let status = parse_winget_list(LIST_GIT_PRESENT, true, "Git.Git");
        assert_eq!(status, DetectStatus::InstalledOk { version: Some("2.56.0.3".to_string()) });
    }

    #[test]
    fn parses_missing_from_golden_fixture() {
        // Probed: not-found exits 20 on winget v1.29.380.
        let status = parse_winget_list(LIST_MISSING, false, "__EstzKit.Nonexistent.Package__");
        assert_eq!(status, DetectStatus::Missing);
    }

    #[test]
    fn not_found_message_is_missing_even_with_success_status() {
        let status = parse_winget_list(LIST_MISSING, true, "__EstzKit.Nonexistent.Package__");
        assert_eq!(status, DetectStatus::Missing);
    }

    #[test]
    fn id_match_is_case_insensitive_and_version_optional() {
        let status = parse_winget_list(LIST_GIT_PRESENT, true, "git.git");
        assert_eq!(status, DetectStatus::InstalledOk { version: Some("2.56.0.3".to_string()) });
        assert_eq!(parse_winget_list("", true, "Git.Git"), DetectStatus::Missing);
    }
}
