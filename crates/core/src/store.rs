//! SQLite persistence for plans, steps, and detection results.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{CoreError, CoreResult};
use crate::{DetectedPackage, InstallPlan, StepState};

/// Schema version stored in the `version` table.
pub const STORE_SCHEMA_VERSION: i64 = 1;

/// File-backed SQLite store for plans/steps/detected packages.
#[derive(Debug)]
pub struct Store {
    /// The underlying SQLite connection.
    pub conn: Connection,
}

impl Store {
    /// Return the resolved default path for the SQLite store:
    /// 1. `$ESTZKIT_STORE` environment variable if set and non-empty.
    /// 2. On Windows: `%LOCALAPPDATA%\EstzKit\state.db` (fallback `%APPDATA%\EstzKit\state.db`).
    /// 3. On other platforms: `$HOME/.local/share/estzkit/state.db`.
    /// 4. Fallback: `estzkit.db` in current working directory.
    pub fn default_path() -> PathBuf {
        if let Ok(store) = std::env::var("ESTZKIT_STORE") {
            if !store.trim().is_empty() {
                return PathBuf::from(store);
            }
        }
        #[cfg(target_os = "windows")]
        {
            if let Ok(local) = std::env::var("LOCALAPPDATA") {
                if !local.trim().is_empty() {
                    return PathBuf::from(local).join("EstzKit").join("state.db");
                }
            }
            if let Ok(roaming) = std::env::var("APPDATA") {
                if !roaming.trim().is_empty() {
                    return PathBuf::from(roaming).join("EstzKit").join("state.db");
                }
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(home) = std::env::var("HOME") {
                if !home.trim().is_empty() {
                    return PathBuf::from(home)
                        .join(".local")
                        .join("share")
                        .join("estzkit")
                        .join("state.db");
                }
            }
        }
        PathBuf::from("estzkit.db")
    }

    /// Open (creating if needed) the store at `path`, ensuring parent directories
    /// exist and creating schema tables.
    pub fn open(path: impl AsRef<Path>) -> CoreResult<Self> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(p)?;
        Self::init(conn)
    }

    /// Open the store using [`Store::default_path`].
    pub fn open_default() -> CoreResult<Self> {
        Self::open(Self::default_path())
    }

    /// Open an in-memory store (used by tests).
    pub fn open_in_memory() -> CoreResult<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> CoreResult<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS version (id INTEGER PRIMARY KEY, version INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS plans (id TEXT PRIMARY KEY, created_at TEXT NOT NULL, status TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS steps (plan_id TEXT NOT NULL, package_id TEXT NOT NULL, state TEXT NOT NULL, exit_code INTEGER, message TEXT, PRIMARY KEY (plan_id, package_id));
             CREATE TABLE IF NOT EXISTS detected_packages (name TEXT PRIMARY KEY, found INTEGER NOT NULL, version TEXT, checked_at TEXT NOT NULL);",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO version (id, version) VALUES (1, ?1)",
            params![STORE_SCHEMA_VERSION],
        )?;
        Ok(Store { conn })
    }

    /// Persist a new plan row on construction.
    pub fn create_plan(&mut self, plan: &InstallPlan) -> CoreResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO plans (id, created_at, status) VALUES (?1, datetime('now'), 'running')",
            params![plan.id],
        )?;
        for step in &plan.steps {
            self.record_step(&plan.id, &step.package_id, step.state, None, None)?;
        }
        Ok(())
    }

    /// Upsert one step transition.
    pub fn record_step(
        &mut self,
        plan_id: &str,
        package_id: &str,
        state: StepState,
        exit_code: Option<i32>,
        message: Option<&str>,
    ) -> CoreResult<()> {
        self.conn.execute(
            "INSERT INTO steps (plan_id, package_id, state, exit_code, message) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(plan_id, package_id) DO UPDATE SET state=excluded.state, exit_code=excluded.exit_code, message=excluded.message",
            params![plan_id, package_id, state.as_str(), exit_code, message],
        )?;
        Ok(())
    }

    /// Upsert one detection result.
    pub fn record_detected(&mut self, detected: &DetectedPackage) -> CoreResult<()> {
        self.conn.execute(
            "INSERT INTO detected_packages (name, found, version, checked_at) VALUES (?1, ?2, ?3, datetime('now'))
             ON CONFLICT(name) DO UPDATE SET found=excluded.found, version=excluded.version, checked_at=excluded.checked_at",
            params![detected.name, detected.found as i64, detected.version],
        )?;
        Ok(())
    }

    /// Update a plan's status string.
    pub fn set_plan_status(&mut self, plan_id: &str, status: &str) -> CoreResult<()> {
        self.conn.execute(
            "UPDATE plans SET status = ?2 WHERE id = ?1",
            params![plan_id, status],
        )?;
        Ok(())
    }

    /// Read back the persisted state of one step (used by tests/diagnostics).
    pub fn step_state(&mut self, plan_id: &str, package_id: &str) -> CoreResult<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT state FROM steps WHERE plan_id = ?1 AND package_id = ?2")?;
        let mut rows = stmt.query(params![plan_id, package_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }
    /// Read back all recorded steps for a plan in insertion order.
    pub fn steps_for_plan(&self, plan_id: &str) -> CoreResult<Vec<StoredStep>> {
        let mut stmt = self.conn.prepare(
            "SELECT package_id, state, exit_code, message FROM steps WHERE plan_id = ?1 ORDER BY rowid",
        )?;
        let rows = stmt.query_map(params![plan_id], |row| {
            Ok(StoredStep {
                package_id: row.get(0)?,
                state: row.get(1)?,
                exit_code: row.get(2)?,
                message: row.get(3)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

/// A step's persisted state retrieved from the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct StoredStep {
    /// Package identifier.
    pub package_id: String,
    /// Step lifecycle state name (e.g. "Pending", "Completed", "Failed").
    pub state: String,
    /// Process exit code if completed/failed.
    pub exit_code: Option<i32>,
    /// Progress or error message.
    pub message: Option<String>,
}
/// Wrap a rusqlite error into a store error (kept for callers that bind it).
pub fn store_err(e: rusqlite::Error) -> CoreError {
    CoreError::Store(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PlanStep;
    use crate::manifest::InstallScope;

    #[test]
    fn persists_plan_and_steps() {
        let mut store = Store::open_in_memory().expect("in-memory store");
        let plan = InstallPlan {
            id: "plan-test".to_string(),
            package_ids: vec!["node".to_string()],
            steps: vec![PlanStep {
                package_id: "node".to_string(),
                depends_on: vec![],
                scope: InstallScope::User,
                state: StepState::Pending,
                message: None,
            }],
            uac_warnings: 0,
        };
        store.create_plan(&plan).expect("create");
        store
            .record_step("plan-test", "node", StepState::Completed, Some(0), Some("ok"))
            .expect("record");
        let state = store.step_state("plan-test", "node").expect("read").expect("present");
        assert_eq!(state, "Completed");
    }

    #[test]
    fn persists_detected() {
        let mut store = Store::open_in_memory().expect("in-memory store");
        let detected = DetectedPackage {
            name: "node".to_string(),
            found: true,
            version: Some("v20".to_string()),
            source: "verify".to_string(),
        };
        store.record_detected(&detected).expect("record detected");
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM detected_packages", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 1);
    }

    #[test]
    fn steps_for_plan_preserves_order_and_content() {
        let mut store = Store::open_in_memory().expect("in-memory store");
        store
            .record_step("p1", "first", StepState::Pending, None, None)
            .expect("record first");
        store
            .record_step("p1", "second", StepState::Completed, Some(0), Some("installed ok"))
            .expect("record second");
        let steps = store.steps_for_plan("p1").expect("read steps");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].package_id, "first");
        assert_eq!(steps[0].state, "Pending");
        assert_eq!(steps[0].exit_code, None);
        assert_eq!(steps[1].package_id, "second");
        assert_eq!(steps[1].state, "Completed");
        assert_eq!(steps[1].exit_code, Some(0));
        assert_eq!(steps[1].message.as_deref(), Some("installed ok"));
    }

    #[test]
    fn open_creates_parent_directories() {
        let temp_dir = std::env::temp_dir().join(format!("estzkit-store-test-{}", std::process::id()));
        let db_path = temp_dir.join("sub").join("state.db");
        let store = Store::open(&db_path).expect("open with nested path");
        assert!(db_path.exists());
        drop(store);
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
