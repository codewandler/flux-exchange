//! Durable, tenant-scoped workflow activity with value-free structural events.

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use exchange_host::{EditorTraceEvent, EditorTraceObserver, Tenant};
use rusqlite::{params, Connection, OptionalExtension as _};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

static NEXT_RUN: AtomicU64 = AtomicU64::new(1);

/// One redacted node event with a host-assigned monotonic sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRunEvent {
    /// Sequence within this run.
    pub sequence: u64,
    /// Upstream's value-free editor trace event.
    pub event: EditorTraceEvent,
}

/// Durable status for one immutable workflow execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRun {
    /// Host-assigned run id.
    pub id: String,
    /// Tenant-local workflow id.
    pub workflow_id: String,
    /// Immutable version targeted.
    pub version: u64,
    /// `running`, `succeeded`, `failed`, or `cancelled`.
    pub status: String,
    /// Redacted ordinary result after success.
    pub result: Option<String>,
    /// Redacted terminal failure.
    pub error: Option<String>,
    /// Unix timestamp milliseconds at creation.
    pub created_at_ms: u64,
    /// Value-free node lifecycle events.
    pub events: Vec<WorkflowRunEvent>,
}

/// SQLite run records plus process-local cancellation handles for live futures.
pub struct WorkflowRunStore {
    database: Mutex<Connection>,
    cancellations: Mutex<HashMap<String, oneshot::Sender<()>>>,
}

impl std::fmt::Debug for WorkflowRunStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkflowRunStore")
            .finish_non_exhaustive()
    }
}

impl WorkflowRunStore {
    /// Open/create the SQLite store and its schema.
    pub fn bind(path: &Path) -> Result<Self, String> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|error| error.to_string())?;
        if file.metadata().map_err(|error| error.to_string())?.mode() & 0o077 != 0 {
            return Err(format!(
                "workflow run store `{}` permissions allow group or other access",
                path.display()
            ));
        }
        drop(file);
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA foreign_keys=ON;
                 CREATE TABLE IF NOT EXISTS workflow_runs (
                   id TEXT PRIMARY KEY,
                   tenant TEXT NOT NULL,
                   workflow_id TEXT NOT NULL,
                   version INTEGER NOT NULL,
                   status TEXT NOT NULL,
                   result TEXT,
                   error TEXT,
                   created_at_ms INTEGER NOT NULL
                 );
                 CREATE INDEX IF NOT EXISTS workflow_runs_tenant
                   ON workflow_runs(tenant, workflow_id, created_at_ms DESC);
                 CREATE TABLE IF NOT EXISTS workflow_run_events (
                   run_id TEXT NOT NULL,
                   sequence INTEGER NOT NULL,
                   event TEXT NOT NULL,
                   PRIMARY KEY(run_id, sequence),
                   FOREIGN KEY(run_id) REFERENCES workflow_runs(id) ON DELETE CASCADE
                 );",
            )
            .map_err(|error| error.to_string())?;
        Ok(Self {
            database: Mutex::new(connection),
            cancellations: Mutex::new(HashMap::new()),
        })
    }

    /// Create a running record and cancellation receiver.
    pub fn create(
        &self,
        tenant: &Tenant,
        workflow_id: &str,
        version: u64,
    ) -> Result<(WorkflowRun, oneshot::Receiver<()>), String> {
        let created_at_ms = now_ms();
        let id = format!(
            "run-{created_at_ms}-{}-{}",
            std::process::id(),
            NEXT_RUN.fetch_add(1, Ordering::Relaxed)
        );
        let version_sql = i64::try_from(version).map_err(|_| "workflow version is too large")?;
        let created_at_sql =
            i64::try_from(created_at_ms).map_err(|_| "workflow timestamp is too large")?;
        self.connection()?
            .execute(
                "INSERT INTO workflow_runs
             (id, tenant, workflow_id, version, status, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, 'running', ?5)",
                params![
                    id,
                    tenant.as_str(),
                    workflow_id,
                    version_sql,
                    created_at_sql
                ],
            )
            .map_err(|error| error.to_string())?;
        let (sender, receiver) = oneshot::channel();
        self.cancellations
            .lock()
            .map_err(|_| "workflow cancellation registry is unavailable".to_owned())?
            .insert(id.clone(), sender);
        Ok((
            WorkflowRun {
                id,
                workflow_id: workflow_id.into(),
                version,
                status: "running".into(),
                result: None,
                error: None,
                created_at_ms,
                events: Vec::new(),
            },
            receiver,
        ))
    }

    /// Complete a run and forget its cancellation handle.
    pub fn finish(
        &self,
        id: &str,
        status: &str,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), String> {
        self.connection()?
            .execute(
                "UPDATE workflow_runs SET status=?2, result=?3, error=?4 WHERE id=?1",
                params![id, status, result, error],
            )
            .map_err(|error| error.to_string())?;
        self.cancellations
            .lock()
            .map_err(|_| "workflow cancellation registry is unavailable".to_owned())?
            .remove(id);
        Ok(())
    }

    /// Ask a live run owned by `tenant` to cancel. Dropping the interpreter future is the normal
    /// upstream cancellation path; its trace activation guards emit terminal failed events.
    pub fn cancel(&self, tenant: &Tenant, id: &str) -> Result<bool, String> {
        if self.get(tenant, id)?.status != "running" {
            return Ok(false);
        }
        Ok(self
            .cancellations
            .lock()
            .map_err(|_| "workflow cancellation registry is unavailable".to_owned())?
            .remove(id)
            .is_some_and(|sender| sender.send(()).is_ok()))
    }

    /// Read one run only within the resolved tenant.
    pub fn get(&self, tenant: &Tenant, id: &str) -> Result<WorkflowRun, String> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT workflow_id, version, status, result, error, created_at_ms
                 FROM workflow_runs WHERE tenant=?1 AND id=?2",
                params![tenant.as_str(), id],
                |row| {
                    Ok(WorkflowRun {
                        id: id.into(),
                        workflow_id: row.get(0)?,
                        version: row.get::<_, i64>(1)? as u64,
                        status: row.get(2)?,
                        result: row.get(3)?,
                        error: row.get(4)?,
                        created_at_ms: row.get::<_, i64>(5)? as u64,
                        events: Vec::new(),
                    })
                },
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "no such workflow run".to_owned())?;
        let mut statement = connection
            .prepare(
                "SELECT sequence, event FROM workflow_run_events
                 WHERE run_id=?1 ORDER BY sequence",
            )
            .map_err(|error| error.to_string())?;
        let events = statement
            .query_map(params![id], |row| {
                let sequence = row.get::<_, i64>(0)? as u64;
                let encoded: String = row.get(1)?;
                Ok((sequence, encoded))
            })
            .map_err(|error| error.to_string())?
            .map(|row| {
                let (sequence, encoded) = row.map_err(|error| error.to_string())?;
                let event = serde_json::from_str(&encoded).map_err(|error| error.to_string())?;
                Ok(WorkflowRunEvent { sequence, event })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(WorkflowRun { events, ..row })
    }

    /// List tenant activity, optionally narrowed to one workflow.
    pub fn list(
        &self,
        tenant: &Tenant,
        workflow: Option<&str>,
    ) -> Result<Vec<WorkflowRun>, String> {
        let ids: Vec<String> = {
            let connection = self.connection()?;
            let mut statement = connection
                .prepare(
                    "SELECT id FROM workflow_runs
                     WHERE tenant=?1 AND (?2 IS NULL OR workflow_id=?2)
                     ORDER BY created_at_ms DESC, id DESC LIMIT 200",
                )
                .map_err(|error| error.to_string())?;
            let ids = statement
                .query_map(params![tenant.as_str(), workflow], |row| row.get(0))
                .map_err(|error| error.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?;
            ids
        };
        ids.iter().map(|id| self.get(tenant, id)).collect()
    }

    /// Observer that persists only upstream's value-free node identity/status event.
    pub(crate) fn observer(self: &Arc<Self>, run_id: &str) -> Arc<RunObserver> {
        Arc::new(RunObserver {
            store: self.clone(),
            run_id: run_id.into(),
            failure: Mutex::new(None),
        })
    }

    fn append(&self, run_id: &str, event: &EditorTraceEvent) -> Result<(), String> {
        let encoded = serde_json::to_string(event).map_err(|error| error.to_string())?;
        let mut connection = self.connection()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let sequence: i64 = transaction
            .query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM workflow_run_events WHERE run_id=?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO workflow_run_events(run_id, sequence, event) VALUES (?1, ?2, ?3)",
                params![run_id, sequence, encoded],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    }

    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
        self.database
            .lock()
            .map_err(|_| "workflow run database is unavailable".to_owned())
    }
}

pub(crate) struct RunObserver {
    store: Arc<WorkflowRunStore>,
    run_id: String,
    failure: Mutex<Option<String>>,
}

impl RunObserver {
    /// First durable trace refusal, if one occurred during synchronous observation.
    pub(crate) fn failure(&self) -> Option<String> {
        self.failure.lock().ok().and_then(|failure| failure.clone())
    }
}

impl EditorTraceObserver for RunObserver {
    fn event(&self, event: &EditorTraceEvent) {
        // SQLite append is synchronous, so the supervisor can refuse a nominally-successful run
        // when its explanation did not become durable. Keep only the storage reason, never values.
        if let Err(error) = self.store.append(&self.run_id, event) {
            if let Ok(mut failure) = self.failure.lock() {
                failure.get_or_insert(error);
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    use super::*;

    fn path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "flux-exchange-workflow-runs-{}-{}.sqlite",
            std::process::id(),
            NEXT_RUN.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn the_run_database_is_owner_only_and_widening_is_refused() {
        let path = path();
        drop(WorkflowRunStore::bind(&path).unwrap());
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o077, 0);

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let refusal = WorkflowRunStore::bind(&path).unwrap_err();

        assert!(refusal.contains("group or other access"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_trace_append_failure_is_visible_to_the_run_supervisor() {
        let path = path();
        let store = Arc::new(WorkflowRunStore::bind(&path).unwrap());
        let observer = store.observer("missing-run");
        let event: EditorTraceEvent = serde_json::from_value(serde_json::json!({
            "node_id": "node-1",
            "source_path": "flow.body[0]",
            "occurrence": 1,
            "phase": "entered"
        }))
        .unwrap();

        observer.event(&event);

        assert!(observer.failure().is_some());
        let _ = std::fs::remove_file(path);
    }
}
