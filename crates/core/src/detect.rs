//! Detection: determine whether a package is installed, in layers:
//!
//! 1. the manifest detect/verify command (with a fresh `PATH` on Windows),
//! 2. the winget id via [`WingetProvider::is_installed`],
//! 3. the Windows Registry Uninstall keys (best effort).

use crate::manifest::PackageManifest;
use crate::provider::WingetProvider;
use crate::verify::{output_matches, run_combined};
use crate::{DetectStatus, DetectedPackage};

/// A detection request for one package manifest.
#[derive(Debug, Clone)]
pub struct DetectJob {
    /// The manifest to detect.
    pub manifest: PackageManifest,
}

/// The result of detecting one package.
#[derive(Debug, Clone)]
pub struct DetectResult {
    /// Friendly detection summary (also persisted to the store).
    pub package: DetectedPackage,
    /// Machine-readable status.
    pub status: DetectStatus,
}

/// Detect one package via the layered strategy described above.
pub fn detect_package(manifest: &PackageManifest, winget: &WingetProvider) -> DetectResult {
    let status = detect_status(manifest, winget);
    let (found, version) = match &status {
        DetectStatus::Missing => (false, None),
        DetectStatus::InstalledOk { version } => (true, version.clone()),
        DetectStatus::VersionMismatch { actual, .. } => (true, Some(actual.clone())),
    };
    let source = detection_source(manifest);
    DetectResult {
        package: DetectedPackage {
            name: manifest.name.clone(),
            found,
            version,
            source: source.to_string(),
        },
        status,
    }
}

/// Name of the detection layer that produced the answer (for display).
fn detection_source(manifest: &PackageManifest) -> &'static str {
    if manifest.detect.is_some() || manifest.verify.command.as_str() != "" {
        "detect/verify"
    } else {
        "registry"
    }
}

fn detect_status(manifest: &PackageManifest, winget: &WingetProvider) -> DetectStatus {
    // Layer 1: detect (or verify) command.
    let (command, args) = match &manifest.detect {
        Some(d) if d.command.is_some() => (d.command.clone().unwrap(), d.args.clone()),
        _ => (manifest.verify.command.clone(), manifest.verify.args.clone()),
    };
    if !command.is_empty() {
        refresh_path();
        if let Ok(output) = run_combined(&command, &args) {
            if output_matches(manifest.verify.expect_regex.as_deref(), &output).unwrap_or(false) {
                return DetectStatus::InstalledOk { version: None };
            }
        }
        // Command ran but did not match, or could not spawn: fall through
        // to the winget/registry layers instead of failing outright.
    }

    // Layer 2: winget id lookup.
    let winget_id = manifest
        .detect
        .as_ref()
        .and_then(|d| d.winget_id.clone())
        .or_else(|| manifest.windows.as_ref().map(|w| w.package.clone()));
    if let Some(id) = winget_id {
        let status = winget.is_installed(&id);
        if status != DetectStatus::Missing {
            return status;
        }
    }

    // Layer 3: registry check (Windows, best effort).
    if registry_installed(&manifest.name) {
        return DetectStatus::InstalledOk { version: None };
    }

    DetectStatus::Missing
}

/// Reload `PATH` from the registry so freshly installed tools are visible
/// to new child processes (Windows only; no-op elsewhere).
#[cfg(windows)]
pub fn refresh_path() {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut parts: Vec<String> = Vec::new();
    if let Ok(hklm) = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment")
    {
        if let Ok(p) = hklm.get_value::<String, _>("Path") {
            parts.push(p);
        }
    }
    if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER).open_subkey("Environment") {
        if let Ok(p) = hkcu.get_value::<String, _>("Path") {
            parts.push(p);
        }
    }
    if !parts.is_empty() {
        std::env::set_var("PATH", parts.join(";"));
    }
}

/// No-op `PATH` refresh on non-Windows platforms.
#[cfg(not(windows))]
pub fn refresh_path() {}

/// Best-effort check of the Windows Registry Uninstall keys
/// (HKLM + HKCU + WOW6432Node) for a display name containing `name`.
#[cfg(windows)]
pub fn registry_installed(name: &str) -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    const UNINSTALL_PATHS: [&str; 3] = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    let roots: [(RegKey, &str); 3] = [
        (RegKey::predef(HKEY_LOCAL_MACHINE), UNINSTALL_PATHS[0]),
        (RegKey::predef(HKEY_LOCAL_MACHINE), UNINSTALL_PATHS[2]),
        (RegKey::predef(HKEY_CURRENT_USER), UNINSTALL_PATHS[1]),
    ];
    for (root, path) in roots {
        if let Ok(opened) = root.open_subkey(path) {
            for key in opened.enum_keys().flatten() {
                if let Ok(entry) = opened.open_subkey(&key) {
                    if let Ok(display) = entry.get_value::<String, _>("DisplayName") {
                        if display.to_lowercase().contains(&name.to_lowercase()) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Non-Windows platforms have no registry to consult.
#[cfg(not(windows))]
pub fn registry_installed(_name: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_status_layering_missing() {
        // A manifest whose command cannot spawn, with no winget id, is Missing.
        let manifest = serde_yaml::from_str(
            "schema_version: 1\nname: ghost\ncategory: test\n\
             install: {strategy: package_manager, scope: user}\n\
             verify: {command: definitely-not-a-real-binary-estzkit, args: [], expect_regex: null}\n\
             security: {allow_scripts: false, allow_elevation: false}\n",
        )
        .expect("test manifest");
        let result = detect_package(&manifest, &WingetProvider::new());
        assert_eq!(result.status, DetectStatus::Missing);
        assert!(!result.package.found);
    }
}
