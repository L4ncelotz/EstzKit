//! SQLite persistence for plans, steps, and detection results.

use rusqlite::{params, Connection};

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
    /// Open (creating if needed) the store at `path`, creating tables.
    pub fn open(path: &str) -> CoreResult<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
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
}
