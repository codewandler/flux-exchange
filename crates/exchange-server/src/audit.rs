//! Durable, typed evidence of authority exercised or refused.
//!
//! The journal deliberately has no generic metadata field. Tokens, request bodies, OIDC material,
//! credential values and setting values cannot be represented by this module's record vocabulary;
//! route adapters can supply only a resolved actor and one of the closed target variants below.

use std::fs::{self, DirBuilder, OpenOptions};
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use exchange_host::Principal;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension as _};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use tracing::{info, warn};

use crate::entropy;

/// The environment setting that names the durable SQLite journal.
pub const AUDIT_SETTING: &str = "FLUX_EXCHANGE_AUDIT";

const SCHEMA_VERSION: u8 = 1;
const RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;
const RETENTION_INTERVAL_SECONDS: i64 = 24 * 60 * 60;
const AUTHENTICATION_WINDOW_SECONDS: i64 = 5 * 60;
const AUTHENTICATION_THRESHOLD: u64 = 20;
const AUTHORIZATION_WINDOW_SECONDS: i64 = 5 * 60;
const AUTHORIZATION_THRESHOLD: u64 = 10;

/// One server-generated request correlation identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestId(String);

impl RequestId {
    /// Draw a fresh identifier from the process's one entropy source.
    pub fn generate() -> Result<Self, AuditError> {
        entropy::hex::<16>().map(Self).map_err(AuditError::Entropy)
    }

    /// A stable identifier supplied by a test, never by an HTTP caller.
    #[cfg(test)]
    pub fn for_test(value: &str) -> Self {
        Self(value.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The stable action vocabulary written to durable evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Authentication,
    Authorization,
    SessionOpened,
    SessionClosed,
    ServiceAccountMinted,
    ServiceAccountRevoked,
    ConnectionLabeled,
    ConnectionCreated,
    CredentialAcquired,
    CredentialRotated,
    CredentialRefreshed,
    CredentialRefreshCommitLost,
    ConnectionRemoved,
    SettingSet,
    SettingCleared,
    GrantsReplaced,
    Invocation,
    AlertRaised,
}

impl Action {
    fn spelling(self) -> &'static str {
        match self {
            Self::Authentication => "authentication",
            Self::Authorization => "authorization",
            Self::SessionOpened => "session_opened",
            Self::SessionClosed => "session_closed",
            Self::ServiceAccountMinted => "service_account_minted",
            Self::ServiceAccountRevoked => "service_account_revoked",
            Self::ConnectionLabeled => "connection_labeled",
            Self::ConnectionCreated => "connection_created",
            Self::CredentialAcquired => "credential_acquired",
            Self::CredentialRotated => "credential_rotated",
            Self::CredentialRefreshed => "credential_refreshed",
            Self::CredentialRefreshCommitLost => "credential_refresh_commit_lost",
            Self::ConnectionRemoved => "connection_removed",
            Self::SettingSet => "setting_set",
            Self::SettingCleared => "setting_cleared",
            Self::GrantsReplaced => "grants_replaced",
            Self::Invocation => "invocation",
            Self::AlertRaised => "alert_raised",
        }
    }
}

/// Whether an action was about to happen, happened, or was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Attempted,
    Succeeded,
    Refused,
}

impl Outcome {
    fn spelling(self) -> &'static str {
        match self {
            Self::Attempted => "attempted",
            Self::Succeeded => "succeeded",
            Self::Refused => "refused",
        }
    }
}

/// The resolved caller, split into fields so evidence can be queried without parsing a display.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub tenant: String,
    pub kind: String,
    pub id: String,
}

impl From<&Principal> for Actor {
    fn from(principal: &Principal) -> Self {
        Self {
            tenant: principal.tenant().as_str().to_owned(),
            kind: principal.kind().to_string(),
            id: principal.id().to_owned(),
        }
    }
}

/// The minimum non-secret object affected by an action.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Identity,
    Session,
    Route {
        path: String,
    },
    ServiceAccounts,
    ServiceAccount {
        id: String,
    },
    Connection {
        connector: String,
    },
    ConnectionInstance {
        connector: String,
        label: String,
    },
    Credential {
        connector: String,
        credential: String,
    },
    InstanceCredential {
        connector: String,
        label: String,
        credential: String,
    },
    Setting {
        connector: String,
        service: String,
        field: String,
    },
    InstanceSetting {
        connector: String,
        label: String,
        service: String,
        field: String,
    },
    Grants {
        tenant: String,
    },
    Invocation {
        operation: String,
    },
    Alert {
        policy: AlertPolicy,
        triggering_event_id: String,
    },
}

impl Target {
    fn query_parts(&self) -> (&'static str, String) {
        match self {
            Self::Identity => ("identity", "identity".to_owned()),
            Self::Session => ("session", "session".to_owned()),
            Self::Route { path } => ("route", path.clone()),
            Self::ServiceAccounts => ("service_accounts", "service_accounts".to_owned()),
            Self::ServiceAccount { id } => ("service_account", id.clone()),
            Self::Connection { connector } => ("connection", connector.clone()),
            Self::ConnectionInstance { connector, label } => {
                ("connection_instance", format!("{connector}/{label}"))
            }
            Self::Credential {
                connector,
                credential,
            } => ("credential", format!("{connector}/{credential}")),
            Self::InstanceCredential {
                connector,
                label,
                credential,
            } => (
                "instance_credential",
                format!("{connector}/{label}/{credential}"),
            ),
            Self::Setting {
                connector,
                service,
                field,
            } => ("setting", format!("{connector}/{service}/{field}")),
            Self::InstanceSetting {
                connector,
                label,
                service,
                field,
            } => (
                "instance_setting",
                format!("{connector}/{label}/{service}/{field}"),
            ),
            Self::Grants { tenant } => ("grants", tenant.clone()),
            Self::Invocation { operation } => ("invocation", operation.clone()),
            Self::Alert {
                policy,
                triggering_event_id,
            } => (
                "alert",
                format!("{}/{}", policy.spelling(), triggering_event_id),
            ),
        }
    }
}

/// A fixed operator-notification rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertPolicy {
    AuthenticationFlood,
    RepeatedAuthorizationFailure,
    CredentialChanged,
    GrantsChanged,
}

impl AlertPolicy {
    fn spelling(self) -> &'static str {
        match self {
            Self::AuthenticationFlood => "authentication_flood",
            Self::RepeatedAuthorizationFailure => "repeated_authorization_failure",
            Self::CredentialChanged => "credential_changed",
            Self::GrantsChanged => "grants_changed",
        }
    }
}

/// One versioned JSON record, both retained and emitted through tracing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub schema_version: u8,
    pub event_id: String,
    pub request_id: String,
    pub timestamp: String,
    pub action: Action,
    pub outcome: Outcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<Actor>,
    pub target: Target,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_seconds: Option<u64>,
}

#[derive(Clone)]
pub struct AuditJournal {
    inner: Arc<Mutex<JournalState>>,
    path: PathBuf,
    fixed_now: Option<i64>,
}

struct JournalState {
    connection: Connection,
    last_retention: i64,
}

/// An action durably marked attempted before its underlying authority is exercised.
pub struct Attempt {
    journal: AuditJournal,
    event_id: String,
}

impl Attempt {
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    /// Transition the attempted row atomically to its final outcome.
    pub fn finish(self, outcome: Outcome) -> Result<Record, AuditError> {
        debug_assert_ne!(outcome, Outcome::Attempted);
        self.journal.transition(&self.event_id, outcome)
    }
}

impl AuditJournal {
    /// Bind a durable owner-only SQLite journal, refusing wider existing modes.
    pub fn bind(path: impl AsRef<Path>) -> Result<Self, AuditError> {
        Self::bind_at(path.as_ref(), None)
    }

    /// Open an existing journal without write authority, for the operator query command.
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, AuditError> {
        let path = path.as_ref();
        let parent = path.parent().ok_or_else(|| AuditError::Path {
            path: path.to_path_buf(),
            reason: "the journal path has no parent directory".to_owned(),
        })?;
        verify_directory(parent)?;
        verify_database_file(path)?;
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|source| AuditError::Database {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(Self {
            inner: Arc::new(Mutex::new(JournalState {
                connection,
                last_retention: i64::MAX,
            })),
            path: path.to_path_buf(),
            fixed_now: None,
        })
    }

    #[cfg(test)]
    fn bind_for_test(path: &Path, now: i64) -> Result<Self, AuditError> {
        Self::bind_at(path, Some(now))
    }

    fn bind_at(path: &Path, fixed_now: Option<i64>) -> Result<Self, AuditError> {
        let parent = path.parent().ok_or_else(|| AuditError::Path {
            path: path.to_path_buf(),
            reason: "the journal path has no parent directory".to_owned(),
        })?;
        ensure_directory(parent)?;
        ensure_database_file(path)?;

        let connection = Connection::open(path).map_err(|source| AuditError::Database {
            path: path.to_path_buf(),
            source,
        })?;
        initialise(&connection, path)?;
        let now = fixed_now.unwrap_or_else(now_unix);
        connection
            .execute(
                "DELETE FROM audit_records WHERE timestamp_unix < ?1",
                params![now - RETENTION_SECONDS],
            )
            .map_err(|source| AuditError::Database {
                path: path.to_path_buf(),
                source,
            })?;

        Ok(Self {
            inner: Arc::new(Mutex::new(JournalState {
                connection,
                last_retention: now,
            })),
            path: path.to_path_buf(),
            fixed_now,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one final event that does not surround a state-changing action.
    pub fn record(
        &self,
        request_id: &RequestId,
        action: Action,
        outcome: Outcome,
        actor: Option<&Principal>,
        target: Target,
    ) -> Result<Record, AuditError> {
        let record = self.new_record(request_id, action, outcome, actor, target, None)?;
        self.insert(&record)?;
        self.evaluate_alerts(&record)?;
        Ok(record)
    }

    /// Insert `attempted` before a state-changing store or runtime is touched.
    pub fn begin(
        &self,
        request_id: &RequestId,
        action: Action,
        actor: &Principal,
        target: Target,
    ) -> Result<Attempt, AuditError> {
        let record = self.new_record(
            request_id,
            action,
            Outcome::Attempted,
            Some(actor),
            target,
            None,
        )?;
        self.insert(&record)?;
        Ok(Attempt {
            journal: self.clone(),
            event_id: record.event_id,
        })
    }

    /// Query by the exact event id.
    pub fn by_event_id(&self, event_id: &str) -> Result<Vec<Record>, AuditError> {
        self.query(
            "SELECT record_json FROM audit_records WHERE event_id = ?1 ORDER BY timestamp_unix DESC LIMIT ?2",
            params![event_id, 1_i64],
        )
    }

    /// Query by the resolved actor triple.
    pub fn by_actor(
        &self,
        tenant: &str,
        kind: &str,
        id: &str,
        limit: u16,
    ) -> Result<Vec<Record>, AuditError> {
        self.query(
            "SELECT record_json FROM audit_records WHERE actor_tenant = ?1 AND actor_kind = ?2 AND actor_id = ?3 ORDER BY timestamp_unix DESC, event_id DESC LIMIT ?4",
            params![tenant, kind, id, i64::from(limit)],
        )
    }

    /// Query by a target's stable kind and canonical value.
    pub fn by_target(
        &self,
        kind: &str,
        value: &str,
        limit: u16,
    ) -> Result<Vec<Record>, AuditError> {
        self.query(
            "SELECT record_json FROM audit_records WHERE target_kind = ?1 AND target_value = ?2 ORDER BY timestamp_unix DESC, event_id DESC LIMIT ?3",
            params![kind, value, i64::from(limit)],
        )
    }

    /// The latest retained successful write that established one credential for a connection.
    ///
    /// The tenant predicate is part of the query rather than a filter over its result: target
    /// values deliberately omit tenant identifiers, so taking a global latest row first could
    /// disclose another tenant's actor or hide the caller's own evidence behind it. A connection
    /// static creation supplies every credential the request wrote; acquisition and refresh
    /// instead record who initiated vendor minting, and a later write of either kind wins.
    /// Absence is `Ok(None)` because the credential store, not this journal, decides whether the
    /// connection exists.
    pub fn latest_credential_supplier(
        &self,
        tenant: &str,
        connector: &str,
        credential: &str,
        instance_label: Option<&str>,
    ) -> Result<Option<Record>, AuditError> {
        let (created_kind, created_value, rotated_kind, rotated_value) = match instance_label {
            Some(label) => (
                "connection_instance",
                format!("{connector}/{label}"),
                "instance_credential",
                format!("{connector}/{label}/{credential}"),
            ),
            None => (
                "connection",
                connector.to_owned(),
                "credential",
                format!("{connector}/{credential}"),
            ),
        };
        let records = self.query(
            "SELECT record_json FROM audit_records
             WHERE actor_tenant = ?1 AND outcome = 'succeeded' AND (
                 (action IN ('connection_created', 'credential_acquired') AND target_kind = ?2 AND target_value = ?3) OR
                 (action IN ('credential_rotated', 'credential_refreshed') AND target_kind = ?4 AND target_value = ?5)
             )
             ORDER BY timestamp_unix DESC, rowid DESC LIMIT 1",
            params![
                tenant,
                created_kind,
                created_value,
                rotated_kind,
                rotated_value
            ],
        )?;
        Ok(records.into_iter().next())
    }

    fn query<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<Record>, AuditError> {
        let state = self.inner.lock().map_err(|_| AuditError::Poisoned)?;
        let mut statement = state
            .connection
            .prepare(sql)
            .map_err(|source| self.database_error(source))?;
        let rows = statement
            .query_map(params, |row| row.get::<_, String>(0))
            .map_err(|source| self.database_error(source))?;
        let mut records = Vec::new();
        for row in rows {
            let json = row.map_err(|source| self.database_error(source))?;
            records.push(serde_json::from_str(&json).map_err(AuditError::Json)?);
        }
        Ok(records)
    }

    fn transition(&self, event_id: &str, outcome: Outcome) -> Result<Record, AuditError> {
        let mut state = self.inner.lock().map_err(|_| AuditError::Poisoned)?;
        self.retain_if_due(&mut state)?;
        let json: Option<String> = state
            .connection
            .query_row(
                "SELECT record_json FROM audit_records WHERE event_id = ?1",
                params![event_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| self.database_error(source))?;
        let mut record: Record = serde_json::from_str(
            &json.ok_or_else(|| AuditError::MissingAttempt(event_id.to_owned()))?,
        )
        .map_err(AuditError::Json)?;
        record.outcome = outcome;
        let rendered = serde_json::to_string(&record).map_err(AuditError::Json)?;
        let changed = state
            .connection
            .execute(
                "UPDATE audit_records SET outcome = ?1, record_json = ?2 WHERE event_id = ?3 AND outcome = 'attempted'",
                params![outcome.spelling(), rendered, event_id],
            )
            .map_err(|source| self.database_error(source))?;
        if changed != 1 {
            return Err(AuditError::AlreadyFinal(event_id.to_owned()));
        }
        drop(state);
        emit(&record);
        self.evaluate_alerts(&record)?;
        Ok(record)
    }

    fn new_record(
        &self,
        request_id: &RequestId,
        action: Action,
        outcome: Outcome,
        actor: Option<&Principal>,
        target: Target,
        alert_measure: Option<(u64, u64)>,
    ) -> Result<Record, AuditError> {
        new_record_at(
            self.now(),
            request_id,
            action,
            outcome,
            actor,
            target,
            alert_measure,
        )
    }

    fn insert(&self, record: &Record) -> Result<(), AuditError> {
        let rendered = serde_json::to_string(record).map_err(AuditError::Json)?;
        let (target_kind, target_value) = record.target.query_parts();
        let mut state = self.inner.lock().map_err(|_| AuditError::Poisoned)?;
        self.retain_if_due(&mut state)?;
        state
            .connection
            .execute(
                "INSERT INTO audit_records (event_id, request_id, timestamp, timestamp_unix, action, outcome, actor_tenant, actor_kind, actor_id, target_kind, target_value, record_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    record.event_id,
                    record.request_id,
                    record.timestamp,
                    parse_timestamp(&record.timestamp)?,
                    record.action.spelling(),
                    record.outcome.spelling(),
                    record.actor.as_ref().map(|actor| actor.tenant.as_str()),
                    record.actor.as_ref().map(|actor| actor.kind.as_str()),
                    record.actor.as_ref().map(|actor| actor.id.as_str()),
                    target_kind,
                    target_value,
                    rendered,
                ],
            )
            .map_err(|source| self.database_error(source))?;
        drop(state);
        emit(record);
        Ok(())
    }

    fn evaluate_alerts(&self, record: &Record) -> Result<(), AuditError> {
        if record.action == Action::AlertRaised || record.outcome == Outcome::Attempted {
            return Ok(());
        }
        let candidate = match (record.action, record.outcome) {
            (Action::Authentication, Outcome::Refused) => Some((
                AlertPolicy::AuthenticationFlood,
                AUTHENTICATION_THRESHOLD,
                AUTHENTICATION_WINDOW_SECONDS,
                String::new(),
            )),
            (Action::Authorization, Outcome::Refused) => record.actor.as_ref().map(|actor| {
                (
                    AlertPolicy::RepeatedAuthorizationFailure,
                    AUTHORIZATION_THRESHOLD,
                    AUTHORIZATION_WINDOW_SECONDS,
                    format!("{}/{}/{}", actor.tenant, actor.kind, actor.id),
                )
            }),
            (
                Action::ConnectionCreated
                | Action::CredentialAcquired
                | Action::CredentialRotated
                | Action::CredentialRefreshed
                | Action::ConnectionRemoved,
                Outcome::Succeeded,
            ) => Some((
                AlertPolicy::CredentialChanged,
                1,
                0,
                record.event_id.clone(),
            )),
            (Action::GrantsReplaced, Outcome::Succeeded) => {
                Some((AlertPolicy::GrantsChanged, 1, 0, record.event_id.clone()))
            }
            _ => None,
        };
        let Some((policy, threshold, window, key)) = candidate else {
            return Ok(());
        };

        let now = self.now();
        let mut state = self.inner.lock().map_err(|_| AuditError::Poisoned)?;
        let transaction = state
            .connection
            .transaction()
            .map_err(|source| self.database_error(source))?;
        let count = if window == 0 {
            1
        } else {
            let mut sql = String::from(
                "SELECT COUNT(*) FROM audit_records WHERE action = ?1 AND outcome = 'refused' AND timestamp_unix >= ?2",
            );
            let count = if policy == AlertPolicy::RepeatedAuthorizationFailure {
                sql.push_str(" AND actor_tenant || '/' || actor_kind || '/' || actor_id = ?3");
                transaction.query_row(
                    &sql,
                    params![record.action.spelling(), now - window, key],
                    |row| row.get::<_, i64>(0),
                )
            } else {
                transaction.query_row(
                    &sql,
                    params![record.action.spelling(), now - window],
                    |row| row.get::<_, i64>(0),
                )
            }
            .map_err(|source| self.database_error(source))?;
            u64::try_from(count).unwrap_or(u64::MAX)
        };
        if count < threshold {
            return Ok(());
        }

        if window > 0 {
            let last: Option<i64> = transaction
                .query_row(
                    "SELECT last_raised FROM alert_state WHERE policy = ?1 AND actor_key = ?2",
                    params![policy.spelling(), key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|source| self.database_error(source))?;
            if last.is_some_and(|last| now - last < window) {
                return Ok(());
            }
        }

        let alert = self.new_record(
            &RequestId(record.request_id.clone()),
            Action::AlertRaised,
            Outcome::Succeeded,
            None,
            Target::Alert {
                policy,
                triggering_event_id: record.event_id.clone(),
            },
            Some((count, u64::try_from(window).unwrap_or(0))),
        )?;
        let rendered = serde_json::to_string(&alert).map_err(AuditError::Json)?;
        let (target_kind, target_value) = alert.target.query_parts();
        transaction
            .execute(
                "INSERT INTO audit_records (event_id, request_id, timestamp, timestamp_unix, action, outcome, actor_tenant, actor_kind, actor_id, target_kind, target_value, record_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, NULL, ?7, ?8, ?9)",
                params![
                    alert.event_id,
                    alert.request_id,
                    alert.timestamp,
                    parse_timestamp(&alert.timestamp)?,
                    alert.action.spelling(),
                    alert.outcome.spelling(),
                    target_kind,
                    target_value,
                    rendered,
                ],
            )
            .map_err(|source| self.database_error(source))?;
        if window > 0 {
            transaction
                .execute(
                    "INSERT INTO alert_state (policy, actor_key, last_raised) VALUES (?1, ?2, ?3) ON CONFLICT(policy, actor_key) DO UPDATE SET last_raised = excluded.last_raised",
                    params![policy.spelling(), key, now],
                )
                .map_err(|source| self.database_error(source))?;
        }
        transaction
            .commit()
            .map_err(|source| self.database_error(source))?;
        drop(state);
        emit(&alert);
        warn!(
            audit = true,
            event_id = alert.event_id,
            request_id = alert.request_id,
            policy = policy.spelling(),
            count,
            window_seconds = window,
            "audit alert raised"
        );
        Ok(())
    }

    fn retain_if_due(&self, state: &mut JournalState) -> Result<(), AuditError> {
        let now = self.now();
        if now - state.last_retention < RETENTION_INTERVAL_SECONDS {
            return Ok(());
        }
        state
            .connection
            .execute(
                "DELETE FROM audit_records WHERE timestamp_unix < ?1",
                params![now - RETENTION_SECONDS],
            )
            .map_err(|source| self.database_error(source))?;
        state.last_retention = now;
        Ok(())
    }

    fn now(&self) -> i64 {
        self.fixed_now.unwrap_or_else(now_unix)
    }

    fn database_error(&self, source: rusqlite::Error) -> AuditError {
        AuditError::Database {
            path: self.path.clone(),
            source,
        }
    }
}

/// Emit the same closed JSON record when a loopback composition chose no durable journal.
pub fn emit_ephemeral(
    request_id: &RequestId,
    action: Action,
    outcome: Outcome,
    actor: Option<&Principal>,
    target: Target,
) -> Result<Record, AuditError> {
    let record = new_record_at(now_unix(), request_id, action, outcome, actor, target, None)?;
    emit(&record);
    Ok(record)
}

fn new_record_at(
    now: i64,
    request_id: &RequestId,
    action: Action,
    outcome: Outcome,
    actor: Option<&Principal>,
    target: Target,
    alert_measure: Option<(u64, u64)>,
) -> Result<Record, AuditError> {
    Ok(Record {
        schema_version: SCHEMA_VERSION,
        event_id: entropy::hex::<16>().map_err(AuditError::Entropy)?,
        request_id: request_id.as_str().to_owned(),
        timestamp: timestamp(now)?,
        action,
        outcome,
        actor: actor.map(Actor::from),
        target,
        count: alert_measure.map(|(count, _)| count),
        window_seconds: alert_measure.map(|(_, window)| window),
    })
}

fn ensure_directory(path: &Path) -> Result<(), AuditError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_dir() {
                return Err(AuditError::Path {
                    path: path.to_path_buf(),
                    reason: "it is not a directory".to_owned(),
                });
            }
            let mode = metadata.permissions().mode() & 0o777;
            if mode != 0o700 {
                return Err(AuditError::Mode {
                    path: path.to_path_buf(),
                    actual: mode,
                    required: 0o700,
                });
            }
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| AuditError::Path {
                path: path.to_path_buf(),
                reason: "its parent does not exist".to_owned(),
            })?;
            if !parent.is_dir() {
                return Err(AuditError::Path {
                    path: path.to_path_buf(),
                    reason: "its parent does not exist".to_owned(),
                });
            }
            DirBuilder::new()
                .mode(0o700)
                .create(path)
                .map_err(|source| AuditError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
        }
        Err(source) => {
            return Err(AuditError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    }
    Ok(())
}

fn verify_directory(path: &Path) -> Result<(), AuditError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| AuditError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(AuditError::Path {
            path: path.to_path_buf(),
            reason: "it is not a directory".to_owned(),
        });
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o700 {
        return Err(AuditError::Mode {
            path: path.to_path_buf(),
            actual: mode,
            required: 0o700,
        });
    }
    Ok(())
}

fn ensure_database_file(path: &Path) -> Result<(), AuditError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() {
                return Err(AuditError::Path {
                    path: path.to_path_buf(),
                    reason: "it is not a regular file".to_owned(),
                });
            }
            let mode = metadata.permissions().mode() & 0o777;
            if mode != 0o600 {
                return Err(AuditError::Mode {
                    path: path.to_path_buf(),
                    actual: mode,
                    required: 0o600,
                });
            }
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|source| AuditError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
        }
        Err(source) => {
            return Err(AuditError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
    }
    Ok(())
}

fn verify_database_file(path: &Path) -> Result<(), AuditError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| AuditError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(AuditError::Path {
            path: path.to_path_buf(),
            reason: "it is not a regular file".to_owned(),
        });
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        return Err(AuditError::Mode {
            path: path.to_path_buf(),
            actual: mode,
            required: 0o600,
        });
    }
    Ok(())
}

fn initialise(connection: &Connection, path: &Path) -> Result<(), AuditError> {
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             CREATE TABLE IF NOT EXISTS audit_records (
                 event_id TEXT PRIMARY KEY,
                 request_id TEXT NOT NULL,
                 timestamp TEXT NOT NULL,
                 timestamp_unix INTEGER NOT NULL,
                 action TEXT NOT NULL,
                 outcome TEXT NOT NULL,
                 actor_tenant TEXT,
                 actor_kind TEXT,
                 actor_id TEXT,
                 target_kind TEXT NOT NULL,
                 target_value TEXT NOT NULL,
                 record_json TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS audit_actor ON audit_records(actor_tenant, actor_kind, actor_id, timestamp_unix);
             CREATE INDEX IF NOT EXISTS audit_target ON audit_records(target_kind, target_value, timestamp_unix);
             CREATE INDEX IF NOT EXISTS audit_request ON audit_records(request_id, timestamp_unix);
             CREATE TABLE IF NOT EXISTS alert_state (
                 policy TEXT NOT NULL,
                 actor_key TEXT NOT NULL,
                 last_raised INTEGER NOT NULL,
                 PRIMARY KEY(policy, actor_key)
             );",
        )
        .map_err(|source| AuditError::Database {
            path: path.to_path_buf(),
            source,
        })
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

fn timestamp(unix: i64) -> Result<String, AuditError> {
    OffsetDateTime::from_unix_timestamp(unix)
        .map_err(|error| AuditError::Timestamp(error.to_string()))?
        .format(&Rfc3339)
        .map_err(|error| AuditError::Timestamp(error.to_string()))
}

fn parse_timestamp(timestamp: &str) -> Result<i64, AuditError> {
    OffsetDateTime::parse(timestamp, &Rfc3339)
        .map(OffsetDateTime::unix_timestamp)
        .map_err(|error| AuditError::Timestamp(error.to_string()))
}

fn emit(record: &Record) {
    match serde_json::to_string(record) {
        Ok(json) => {
            info!(audit = true, event_id = record.event_id, request_id = record.request_id, record = %json, "audit record")
        }
        Err(error) => warn!(%error, "a retained audit record could not be emitted"),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("the audit journal at `{path}` cannot be used: {reason}")]
    Path { path: PathBuf, reason: String },
    #[error(
        "refusing the audit journal at `{path}`: its mode is {actual:04o}, required {required:04o}"
    )]
    Mode {
        path: PathBuf,
        actual: u32,
        required: u32,
    },
    #[error("the audit journal at `{path}` cannot be opened: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the audit journal at `{path}` is unavailable: {source}")]
    Database {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("the audit journal could not generate an identifier: {0}")]
    Entropy(std::io::Error),
    #[error("the audit journal produced invalid JSON: {0}")]
    Json(serde_json::Error),
    #[error("the audit journal could not represent a timestamp: {0}")]
    Timestamp(String),
    #[error("audit event `{0}` has no attempted row to finish")]
    MissingAttempt(String),
    #[error("audit event `{0}` was already final")]
    AlreadyFinal(String),
    #[error("the audit journal lock is poisoned")]
    Poisoned,
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};
    use axum::Router;
    use exchange_host::{PrincipalKind, Tenant};
    use tower::Service as _;

    use crate::dev_identity::DevIdentity;
    use crate::routes;
    use crate::state::AppState;

    fn scratch(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-audit-{name}-{}",
            entropy::hex::<8>().expect("test entropy")
        ));
        fs::create_dir(&root).expect("a scratch root");
        root
    }

    fn alice() -> Principal {
        Principal::new(
            PrincipalKind::User,
            "alice",
            Tenant::new("acme").expect("a tenant"),
        )
    }

    async fn call(app: Router, request: Request<Body>) -> (StatusCode, String) {
        let mut service = app.into_service::<Body>();
        std::future::poll_fn(|cx| service.poll_ready(cx))
            .await
            .expect("router ready");
        let response = service.call(request).await.expect("router response");
        let request_id = response
            .headers()
            .get("x-request-id")
            .expect("correlation header")
            .to_str()
            .expect("ASCII request id")
            .to_owned();
        (response.status(), request_id)
    }

    fn request_records(journal: &AuditJournal, request_id: &str) -> Vec<Record> {
        let state = journal.inner.lock().expect("journal lock");
        let mut statement = state
            .connection
            .prepare("SELECT record_json FROM audit_records WHERE request_id = ?1 ORDER BY action")
            .expect("query");
        statement
            .query_map(params![request_id], |row| row.get::<_, String>(0))
            .expect("rows")
            .map(|row| serde_json::from_str(&row.expect("row")).expect("record JSON"))
            .collect()
    }

    #[test]
    fn records_are_json_and_survive_restart_with_three_query_shapes() {
        let root = scratch("restart");
        let path = root.join("audit").join("events.sqlite3");
        let request = RequestId::for_test("request-1");
        let event = {
            let journal = AuditJournal::bind_for_test(&path, 2_000_000_000).expect("a journal");
            let attempt = journal
                .begin(
                    &request,
                    Action::CredentialRotated,
                    &alice(),
                    Target::Credential {
                        connector: "github".to_owned(),
                        credential: "token".to_owned(),
                    },
                )
                .expect("attempted evidence");
            attempt
                .finish(Outcome::Succeeded)
                .expect("final evidence")
                .event_id
        };

        let journal = AuditJournal::bind_for_test(&path, 2_000_000_001).expect("reopened");
        let by_event = journal.by_event_id(&event).expect("event query");
        let by_actor = journal
            .by_actor("acme", "user", "alice", 20)
            .expect("actor query");
        let by_target = journal
            .by_target("credential", "github/token", 20)
            .expect("target query");
        assert_eq!(by_event, by_actor);
        assert_eq!(by_event, by_target);
        assert_eq!(by_event[0].outcome, Outcome::Succeeded);
        let parsed: serde_json::Value = serde_json::to_value(&by_event[0]).expect("JSON");
        for field in [
            "schema_version",
            "event_id",
            "request_id",
            "timestamp",
            "action",
            "outcome",
            "actor",
            "target",
        ] {
            assert!(parsed.get(field).is_some(), "missing {field}: {parsed}");
        }
    }

    #[test]
    fn retention_keeps_the_boundary_and_removes_only_older_rows() {
        let root = scratch("retention");
        let path = root.join("audit").join("events.sqlite3");
        let now = 2_000_000_000;
        let journal = AuditJournal::bind_for_test(&path, now).expect("a journal");
        let connection = Connection::open(&path).expect("operator read");
        for (id, age) in [
            ("boundary", RETENTION_SECONDS),
            ("older", RETENTION_SECONDS + 1),
        ] {
            let record = Record {
                schema_version: 1,
                event_id: id.to_owned(),
                request_id: "request".to_owned(),
                timestamp: timestamp(now - age).expect("timestamp"),
                action: Action::Authentication,
                outcome: Outcome::Refused,
                actor: None,
                target: Target::Identity,
                count: None,
                window_seconds: None,
            };
            connection
                .execute(
                    "INSERT INTO audit_records VALUES (?1, ?2, ?3, ?4, 'authentication', 'refused', NULL, NULL, NULL, 'identity', 'identity', ?5)",
                    params![id, "request", record.timestamp, now - age, serde_json::to_string(&record).expect("JSON")],
                )
                .expect("fixture row");
        }
        drop(connection);
        drop(journal);

        let reopened = AuditJournal::bind_for_test(&path, now).expect("reopened");
        assert_eq!(reopened.by_event_id("boundary").expect("query").len(), 1);
        assert!(reopened.by_event_id("older").expect("query").is_empty());
    }

    #[test]
    fn widened_directory_and_database_modes_are_refused_not_repaired() {
        let root = scratch("modes");
        let directory = root.join("audit");
        fs::create_dir(&directory).expect("directory");
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).expect("widen");
        let path = directory.join("events.sqlite3");
        let refusal = AuditJournal::bind(&path)
            .err()
            .expect("wide directory must refuse");
        assert!(matches!(refusal, AuditError::Mode { actual: 0o755, .. }));
        assert_eq!(
            fs::metadata(&directory)
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o755
        );

        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).expect("test setup");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o644)
            .open(&path)
            .expect("wide file");
        let refusal = AuditJournal::bind(&path)
            .err()
            .expect("wide file must refuse");
        assert!(matches!(refusal, AuditError::Mode { actual: 0o644, .. }));
        assert_eq!(
            fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
            0o644
        );
    }

    #[test]
    fn authentication_and_authorization_thresholds_raise_rearmed_alerts() {
        let root = scratch("alerts");
        let path = root.join("audit").join("events.sqlite3");
        let journal = AuditJournal::bind_for_test(&path, 2_000_000_000).expect("a journal");
        for index in 0..AUTHENTICATION_THRESHOLD {
            journal
                .record(
                    &RequestId::for_test(&format!("auth-{index}")),
                    Action::Authentication,
                    Outcome::Refused,
                    None,
                    Target::Identity,
                )
                .expect("authentication evidence");
        }
        for index in 0..AUTHORIZATION_THRESHOLD {
            journal
                .record(
                    &RequestId::for_test(&format!("authorization-{index}")),
                    Action::Authorization,
                    Outcome::Refused,
                    Some(&alice()),
                    Target::Route {
                        path: "/api/grants".to_owned(),
                    },
                )
                .expect("authorization evidence");
        }
        journal
            .record(
                &RequestId::for_test("credential-change"),
                Action::CredentialRotated,
                Outcome::Succeeded,
                Some(&alice()),
                Target::Credential {
                    connector: "github".to_owned(),
                    credential: "token".to_owned(),
                },
            )
            .expect("credential change evidence");
        journal
            .record(
                &RequestId::for_test("grant-change"),
                Action::GrantsReplaced,
                Outcome::Succeeded,
                Some(&alice()),
                Target::Grants {
                    tenant: "acme".to_owned(),
                },
            )
            .expect("grant change evidence");
        let auth = journal
            .by_target("alert", "authentication_flood/auth-irrelevant", 100)
            .expect("query shape");
        assert!(auth.is_empty(), "alert targets include the triggering id");
        {
            let state = journal.inner.lock().expect("journal lock");
            let count: i64 = state
                .connection
                .query_row(
                    "SELECT COUNT(*) FROM audit_records WHERE action = 'alert_raised'",
                    [],
                    |row| row.get(0),
                )
                .expect("count");
            assert_eq!(count, 4);
        }
        drop(journal);

        let journal = AuditJournal::bind_for_test(&path, 2_000_000_301).expect("reopened later");
        for index in 0..AUTHENTICATION_THRESHOLD {
            journal
                .record(
                    &RequestId::for_test(&format!("rearmed-{index}")),
                    Action::Authentication,
                    Outcome::Refused,
                    None,
                    Target::Identity,
                )
                .expect("authentication evidence");
        }
        let state = journal.inner.lock().expect("journal lock");
        let count: i64 = state
            .connection
            .query_row(
                "SELECT COUNT(*) FROM audit_records WHERE action = 'alert_raised'",
                [],
                |row| row.get(0),
            )
            .expect("count");
        assert_eq!(count, 5, "the flood policy re-arms after its window");
    }

    #[test]
    fn a_failed_final_transition_leaves_the_attempted_row() {
        let root = scratch("write-failure");
        let journal =
            AuditJournal::bind_for_test(&root.join("audit").join("events.sqlite3"), 2_000_000_000)
                .expect("journal");
        let attempt = journal
            .begin(
                &RequestId::for_test("request"),
                Action::GrantsReplaced,
                &alice(),
                Target::Grants {
                    tenant: "acme".to_owned(),
                },
            )
            .expect("attempt");
        let event_id = attempt.event_id().to_owned();
        journal
            .inner
            .lock()
            .expect("journal lock")
            .connection
            .execute_batch("PRAGMA query_only = ON")
            .expect("inject write refusal");

        attempt
            .finish(Outcome::Succeeded)
            .expect_err("the injected write refusal must be visible");
        assert_eq!(
            journal.by_event_id(&event_id).expect("query")[0].outcome,
            Outcome::Attempted,
            "an operator can see that the action may have happened"
        );
    }

    #[tokio::test]
    async fn requests_correlate_success_and_refusal_without_retaining_sentinel_material() {
        const SENTINEL: &str = "AUDIT_MUST_NEVER_RETAIN_7f4d2a";
        let root = scratch("sentinel");
        let journal = Arc::new(
            AuditJournal::bind_for_test(&root.join("audit").join("events.sqlite3"), 2_000_000_000)
                .expect("journal"),
        );
        let user = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster("user:alice@acme").expect("user roster"),
        ))
        .with_audit(journal.clone());

        let (status, success_request) = call(
            routes::app(user.clone()),
            Request::builder()
                .method(Method::POST)
                .uri("/api/session")
                .header(header::AUTHORIZATION, "Bearer alice")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"body":"{SENTINEL}"}}"#)))
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let success = request_records(&journal, &success_request);
        assert!(success.iter().any(|record| {
            record.action == Action::Authentication && record.outcome == Outcome::Succeeded
        }));
        assert!(success.iter().any(|record| {
            record.action == Action::SessionOpened && record.outcome == Outcome::Succeeded
        }));

        let service_account = AppState::with_development_identity(Arc::new(
            DevIdentity::from_roster("service_account:bot@acme").expect("account roster"),
        ))
        .with_audit(journal.clone());
        let (status, refused_request) = call(
            routes::app(service_account),
            Request::builder()
                .method(Method::POST)
                .uri("/api/service-accounts")
                .header(header::AUTHORIZATION, "Bearer bot")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"id":"{SENTINEL}"}}"#)))
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let refused = request_records(&journal, &refused_request);
        assert!(refused.iter().any(|record| {
            record.action == Action::Authentication && record.outcome == Outcome::Succeeded
        }));
        assert!(refused.iter().any(|record| {
            record.action == Action::Authorization && record.outcome == Outcome::Refused
        }));

        for (method, uri, authorization, body) in [
            (
                Method::GET,
                "/api/session",
                format!("Bearer {SENTINEL}"),
                String::new(),
            ),
            (
                Method::POST,
                "/api/connections/github",
                "Bearer alice".to_owned(),
                format!(r#"{{"credentials":{{"token":"{SENTINEL}"}}}}"#),
            ),
            (
                Method::PUT,
                "/api/connections/github/credentials/token",
                "Bearer alice".to_owned(),
                format!(r#"{{"value":"{SENTINEL}"}}"#),
            ),
            (
                Method::PUT,
                "/api/connections/zendesk/settings/default/endpoint.subdomain",
                "Bearer alice".to_owned(),
                format!(r#"{{"value":"{SENTINEL}"}}"#),
            ),
        ] {
            let _ = call(
                routes::app(user.clone()),
                Request::builder()
                    .method(method)
                    .uri(uri)
                    .header(header::AUTHORIZATION, authorization)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await;
        }
        let _ = call(
            routes::app(user),
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/signin/callback?state=x&code={SENTINEL}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;

        let state = journal.inner.lock().expect("journal lock");
        let mut statement = state
            .connection
            .prepare("SELECT record_json FROM audit_records")
            .expect("scan");
        let rendered: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .expect("rows")
            .map(|row| row.expect("row"))
            .collect();
        for json in rendered {
            let parsed: serde_json::Value = serde_json::from_str(&json).expect("record JSON");
            assert!(
                !parsed.to_string().contains(SENTINEL),
                "prohibited material reached an audit field or value: {parsed}"
            );
        }
    }
}
