//! Core error types.

use thiserror::Error;

/// Errors returned by the EstzKit core library.
#[derive(Debug, Error)]
pub enum CoreError {
    /// An install strategy other than `package_manager` was requested.
    /// The MVP supports `package_manager` only.
    #[error("unsupported install strategy: {0}")]
    UnsupportedStrategy(String),

    /// A dependency cycle was detected during resolution.
    #[error("dependency cycle detected: {0}")]
    Cycle(String),

    /// A package depends on a package that is not in the manifest set.
    #[error("missing dependency: {0}")]
    MissingDependency(String),

    /// A manifest file could not be parsed or failed validation.
    #[error("manifest parse error: {0}")]
    ManifestParse(String),

    /// A manifest failed semantic validation (schema version, name, etc.).
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    /// A SQLite store operation failed.
    #[error("store error: {0}")]
    Store(String),

    /// A child process could not be spawned.
    #[error("process spawn error: {0}")]
    ProcessSpawn(String),

    /// An I/O error occurred while reading the filesystem.
    #[error("io error: {0}")]
    Io(String),

    /// A plan was not found in the store.
    #[error("plan not found: {0}")]
    PlanNotFound(String),
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::Store(e.to_string())
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

/// Convenience alias for core results.
pub type CoreResult<T> = Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_mentions_payload() {
        let err = CoreError::MissingDependency("docker".to_string());
        assert!(err.to_string().contains("docker"));
        let err = CoreError::UnsupportedStrategy("script".to_string());
        assert!(err.to_string().contains("script"));
    }
}
