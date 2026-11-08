//! Manifest types (`packages/*.yaml`, `profiles/*.yaml`) and sources.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use regex::Regex;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};

/// The only manifest schema version accepted by this build.
pub const SCHEMA_VERSION: u32 = 1;

/// Environment variable that points at an on-disk manifest directory.
pub const MANIFESTS_ENV_VAR: &str = "ESTZKIT_MANIFESTS";

/// A package manifest: one `manifests/packages/<name>.yaml` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageManifest {
    /// Manifest schema version; must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Package identifier, `^[a-z0-9-]+$`.
    pub name: String,
    /// Human-facing category (e.g. `"runtime"`, `"editor"`).
    pub category: String,
    /// Windows install specification, when supported.
    #[serde(default)]
    pub windows: Option<OsProvider>,
    /// macOS install specification, when supported.
    #[serde(default)]
    pub macos: Option<OsProvider>,
    /// Linux install specification, when supported.
    #[serde(default)]
    pub linux: Option<OsProvider>,
    /// Names of packages that must be installed before this one.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// How this package is installed and at which scope.
    pub install: InstallSpec,
    /// Post-install verification command.
    pub verify: VerifySpec,
    /// Optional detection override; when absent the verify command is used.
    #[serde(default)]
    pub detect: Option<DetectSpec>,
    /// Security flags. `allow_scripts` must be `false` in the MVP.
    pub security: SecuritySpec,
}

/// Per-OS provider binding: which package manager and which package id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsProvider {
    /// Which package manager provides this package on this OS.
    #[serde(rename = "provider")]
    pub provider: ProviderKind,
    /// Native package id (e.g. a winget id).
    pub package: String,
    /// Pinned version, if any.
    #[serde(default)]
    pub version: Option<String>,
}

/// The package manager that owns a package on an OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// Windows Package Manager (`winget`).
    Winget,
    /// Homebrew (`brew`). Not implemented in the MVP.
    Brew,
    /// APT (`apt`). Not implemented in the MVP.
    Apt,
}

/// How a package is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStrategy {
    /// Install via the OS package manager (the only MVP strategy).
    PackageManager,
    /// Install a direct download.
    Direct,
    /// Run an install script.
    Script,
}

/// User-scope vs machine-scope installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum InstallScope {
    /// Per-user install (no elevation required).
    User,
    /// Machine-wide install (may prompt UAC).
    Machine,
}

/// Install strategy + scope pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallSpec {
    /// Install strategy.
    pub strategy: InstallStrategy,
    /// Install scope.
    pub scope: InstallScope,
}

/// Verification command run after a successful install.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifySpec {
    /// Executable to run.
    pub command: String,
    /// Arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Regex the combined output must match; `None` means success status is enough.
    #[serde(default)]
    pub expect_regex: Option<String>,
}

/// Detection override. At least one of `command` / `winget_id` must be set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectSpec {
    /// Command to probe, if any.
    #[serde(default)]
    pub command: Option<String>,
    /// Arguments for the probe command.
    #[serde(default)]
    pub args: Vec<String>,
    /// winget id to look up via `winget list`, if any.
    #[serde(default)]
    pub winget_id: Option<String>,
}

/// Security flags declared by a manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecuritySpec {
    /// Whether the manifest may run install scripts. Must be `false` in the MVP.
    pub allow_scripts: bool,
    /// Whether installation is expected to prompt UAC (machine scope / installer).
    pub allow_elevation: bool,
}

/// A profile manifest: one `manifests/profiles/<name>.yaml` file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileManifest {
    /// Manifest schema version; must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Profile identifier.
    pub name: String,
    /// Human-facing display name.
    pub display_name: String,
    /// Package names included in this profile (at least one).
    pub packages: Vec<String>,
}

impl PackageManifest {
    /// Validate the manifest. Rejects wrong schema versions, malformed
    /// names, non-`package_manager` strategies, `allow_scripts: true`,
    /// and empty detect specs.
    pub fn validate(&self) -> CoreResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(CoreError::InvalidManifest(format!(
                "package '{}': unsupported schema_version {} (expected {})",
                self.name, self.schema_version, SCHEMA_VERSION
            )));
        }
        if !is_valid_name(&self.name) {
            return Err(CoreError::InvalidManifest(format!(
                "package name '{}' must match ^[a-z0-9-]+$",
                self.name
            )));
        }
        if self.install.strategy != InstallStrategy::PackageManager {
            return Err(CoreError::UnsupportedStrategy(format!(
                "package '{}': strategy {:?} (MVP accepts package_manager only)",
                self.name, self.install.strategy
            )));
        }
        if self.security.allow_scripts {
            return Err(CoreError::InvalidManifest(format!(
                "package '{}': allow_scripts must be false in the MVP",
                self.name
            )));
        }
        if let Some(detect) = &self.detect {
            if detect.command.is_none() && detect.winget_id.is_none() {
                return Err(CoreError::InvalidManifest(format!(
                    "package '{}': detect must set at least one of command/winget_id",
                    self.name
                )));
            }
        }
        Ok(())
    }

    /// The provider binding for the current OS, if the manifest declares one.
    pub fn os_provider_for_current(&self) -> Option<&OsProvider> {
        if cfg!(target_os = "windows") {
            self.windows.as_ref()
        } else if cfg!(target_os = "macos") {
            self.macos.as_ref()
        } else {
            self.linux.as_ref()
        }
    }
}

impl ProfileManifest {
    /// Validate the profile manifest.
    pub fn validate(&self) -> CoreResult<()> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(CoreError::InvalidManifest(format!(
                "profile '{}': unsupported schema_version {} (expected {})",
                self.name, self.schema_version, SCHEMA_VERSION
            )));
        }
        if self.packages.is_empty() {
            return Err(CoreError::InvalidManifest(format!(
                "profile '{}' must list at least one package",
                self.name
            )));
        }
        Ok(())
    }
}

/// Check the package-name grammar `^[a-z0-9-]+$`.
pub fn is_valid_name(name: &str) -> bool {
    static NAME_RE: std::sync::LazyLock<Regex> =
        std::sync::LazyLock::new(|| Regex::new(r"^[a-z0-9-]+$").expect("static regex"));
    NAME_RE.is_match(name)
}

/// Parse a package manifest from YAML text.
pub fn parse_package_manifest(yaml: &str) -> CoreResult<PackageManifest> {
    let manifest: PackageManifest = serde_yaml::from_str(yaml)
        .map_err(|e| CoreError::ManifestParse(e.to_string()))?;
    manifest.validate()?;
    Ok(manifest)
}

/// Parse a profile manifest from YAML text.
pub fn parse_profile_manifest(yaml: &str) -> CoreResult<ProfileManifest> {
    let manifest: ProfileManifest = serde_yaml::from_str(yaml)
        .map_err(|e| CoreError::ManifestParse(e.to_string()))?;
    manifest.validate()?;
    Ok(manifest)
}

/// Source of package and profile manifests.
pub trait ManifestSource: Send + Sync {
    /// All package manifests, sorted by name.
    fn packages(&self) -> CoreResult<Vec<PackageManifest>>;
    /// All profile manifests, sorted by name.
    fn profiles(&self) -> CoreResult<Vec<ProfileManifest>>;
}

/// Manifests embedded in the binary from the workspace `manifests/` directory.
#[derive(Debug, Default, Clone, Copy)]
pub struct BundledSource;

#[derive(RustEmbed)]
#[folder = "../../manifests"]
struct Assets;

impl BundledSource {
    fn load(&self, prefix: &str) -> CoreResult<Vec<String>> {
        let mut texts = Vec::new();
        for path in Assets::iter() {
            let path = path.as_ref();
            if path.starts_with(prefix) && path.ends_with(".yaml") {
                if let Some(file) = Assets::get(path) {
                    texts.push(
                        String::from_utf8(file.data.into_owned())
                            .map_err(|e| CoreError::ManifestParse(e.to_string()))?,
                    );
                }
            }
        }
        Ok(texts)
    }
}

impl ManifestSource for BundledSource {
    fn packages(&self) -> CoreResult<Vec<PackageManifest>> {
        let mut out = Vec::new();
        for text in self.load("packages/")? {
            out.push(parse_package_manifest(&text)?);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn profiles(&self) -> CoreResult<Vec<ProfileManifest>> {
        let mut out = Vec::new();
        for text in self.load("profiles/")? {
            out.push(parse_profile_manifest(&text)?);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

/// Manifests read from an on-disk directory with a
/// `packages/*.yaml` + `profiles/*.yaml` layout.
#[derive(Debug, Clone)]
pub struct DirSource {
    /// Root directory containing `packages/` and `profiles/`.
    pub dir: PathBuf,
}

impl DirSource {
    /// Create a source for an explicit directory.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        DirSource { dir: dir.into() }
    }

    /// Create a source from `$ESTZKIT_MANIFESTS`, if set.
    pub fn from_env() -> Option<Self> {
        std::env::var(MANIFESTS_ENV_VAR)
            .ok()
            .map(|v| DirSource { dir: PathBuf::from(v) })
    }

    /// Resolve the effective manifest directory: explicit path first, then
    /// `$ESTZKIT_MANIFESTS`, then `./manifests`.
    pub fn resolve_dir(explicit: Option<&Path>) -> PathBuf {
        if let Some(dir) = explicit {
            return dir.to_path_buf();
        }
        if let Some(source) = DirSource::from_env() {
            return source.dir;
        }
        PathBuf::from("manifests")
    }

    fn load_dir(&self, subdir: &str) -> CoreResult<Vec<String>> {
        let dir = self.dir.join(subdir);
        let mut names: Vec<PathBuf> = Vec::new();
        let entries = std::fs::read_dir(&dir).map_err(|e| {
            CoreError::Io(format!("reading manifest dir {}: {}", dir.display(), e))
        })?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                names.push(path);
            }
        }
        names.sort();
        let mut texts = Vec::new();
        for path in names {
            texts.push(std::fs::read_to_string(&path).map_err(|e| {
                CoreError::Io(format!("reading {}: {}", path.display(), e))
            })?);
        }
        Ok(texts)
    }
}

impl ManifestSource for DirSource {
    fn packages(&self) -> CoreResult<Vec<PackageManifest>> {
        let mut out = Vec::new();
        for text in self.load_dir("packages")? {
            out.push(parse_package_manifest(&text)?);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    fn profiles(&self) -> CoreResult<Vec<ProfileManifest>> {
        let mut out = Vec::new();
        for text in self.load_dir("profiles")? {
            out.push(parse_profile_manifest(&text)?);
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

/// Index manifests by name.
pub fn index_by_name(manifests: &[PackageManifest]) -> std::collections::HashMap<String, PackageManifest> {
    manifests
        .iter()
        .map(|m| (m.name.clone(), m.clone()))
        .collect()
}

/// Names of packages present in `installed` (used to skip already-installed
/// packages when planning).
pub fn installed_names(installed: &[crate::DetectedPackage]) -> HashSet<String> {
    installed
        .iter()
        .filter(|p| p.found)
        .map(|p| p.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_PACKAGE: &str = r#"
schema_version: 1
name: node
category: runtime
windows:
  provider: winget
  package: OpenJS.NodeJS.LTS
  version: null
depends_on: []
install:
  strategy: package_manager
  scope: user
verify:
  command: node
  args: ["--version"]
  expect_regex: null
security:
  allow_scripts: false
  allow_elevation: false
"#;

    #[test]
    fn parse_minimal_package() {
        let m = parse_package_manifest(MINIMAL_PACKAGE).expect("valid manifest");
        assert_eq!(m.name, "node");
        assert_eq!(m.install.scope, InstallScope::User);
        assert_eq!(m.depends_on.len(), 0);
    }

    #[test]
    fn parse_rejects_script_strategy() {
        let yaml = MINIMAL_PACKAGE.replace("package_manager", "script");
        let err = parse_package_manifest(&yaml).expect_err("script must be rejected");
        assert!(matches!(err, CoreError::UnsupportedStrategy(_)));
    }

    #[test]
    fn parse_rejects_bad_name() {
        let yaml = MINIMAL_PACKAGE.replace("node", "Node!");
        let err = parse_package_manifest(&yaml).expect_err("bad name must be rejected");
        assert!(matches!(err, CoreError::InvalidManifest(_)));
    }

    #[test]
    fn parse_rejects_allow_scripts_true() {
        let yaml = MINIMAL_PACKAGE.replace("allow_scripts: false", "allow_scripts: true");
        let err = parse_package_manifest(&yaml).expect_err("allow_scripts must be false");
        assert!(matches!(err, CoreError::InvalidManifest(_)));
    }

    #[test]
    fn parse_rejects_empty_detect() {
        let yaml = format!(
            "{}\ndetect: {{}}\n",
            MINIMAL_PACKAGE.trim_end()
        );
        let err = parse_package_manifest(&yaml).expect_err("empty detect must be rejected");
        assert!(matches!(err, CoreError::InvalidManifest(_)));
    }

    #[test]
    fn name_grammar() {
        assert!(is_valid_name("node"));
        assert!(is_valid_name("my-pkg-1"));
        assert!(!is_valid_name("Node"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("a b"));
    }

    #[test]
    fn parse_profile() {
        let yaml = "schema_version: 1\nname: fullstack-developer\ndisplay_name: Fullstack Developer\npackages: [node, git]\n";
        let p = parse_profile_manifest(yaml).expect("valid profile");
        assert_eq!(p.packages.len(), 2);
    }

    #[test]
    fn parse_profile_requires_packages() {
        let yaml = "schema_version: 1\nname: empty\ndisplay_name: Empty\npackages: []\n";
        let err = parse_profile_manifest(yaml).expect_err("empty profile must be rejected");
        assert!(matches!(err, CoreError::InvalidManifest(_)));
    }
}
