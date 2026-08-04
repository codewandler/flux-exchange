//! Tenant-scoped operator labels for connector connection UUIDs.
//!
//! Credentials remain authoritative for whether a connection exists. This registry answers only
//! what an operator called a host-minted UUID; callers of this port must intersect its answers with
//! [`SecretStore::references`](crate::SecretStore::references) before presenting or selecting a
//! connection. A stale label can therefore name nothing, and deleting every label hides nothing.

use std::collections::BTreeMap;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use connector_address::InstanceId;

use crate::Tenant;

/// An operator-chosen connection name.
///
/// This is not an address component. The address carries the immutable UUID the host minted; the
/// label may be renamed without moving a credential.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionLabel(String);

impl ConnectionLabel {
    /// Validate one label: 1–64 ASCII alphanumeric, `-`, or `_` bytes.
    ///
    /// # Errors
    ///
    /// [`RegistryRefusal::InvalidLabel`] when the spelling is empty, too long, non-ASCII, or
    /// contains a byte outside the closed grammar.
    pub fn new(label: impl Into<String>) -> Result<Self, RegistryRefusal> {
        let label = label.into();
        let valid = !label.is_empty()
            && label.len() <= 64
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if !valid {
            return Err(RegistryRefusal::InvalidLabel { label });
        }
        Ok(Self(label))
    }

    /// The validated spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ConnectionLabel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// One naming-overlay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedConnection {
    /// The operator's mutable name.
    pub label: ConnectionLabel,
    /// The immutable address component minted by the host.
    pub instance: InstanceId,
}

/// The label overlay a composition binds.
pub trait ConnectionRegistry: Send + Sync {
    /// Every name recorded for this tenant and connector.
    ///
    /// These rows do not prove existence. The caller must derive held instances from the secret
    /// store and discard rows whose UUID is not held.
    fn entries(
        &self,
        tenant: &Tenant,
        connector: &str,
    ) -> Result<Vec<NamedConnection>, RegistryRefusal>;

    /// Record `label -> instance`, refusing either a duplicate label or a second label for the UUID.
    fn assign(
        &self,
        tenant: &Tenant,
        connector: &str,
        label: &ConnectionLabel,
        instance: &InstanceId,
    ) -> Result<(), RegistryRefusal>;

    /// Rename one row without changing its UUID.
    fn rename(
        &self,
        tenant: &Tenant,
        connector: &str,
        from: &ConnectionLabel,
        to: &ConnectionLabel,
    ) -> Result<InstanceId, RegistryRefusal>;

    /// Remove one name and return the UUID it named.
    fn remove(
        &self,
        tenant: &Tenant,
        connector: &str,
        label: &ConnectionLabel,
    ) -> Result<InstanceId, RegistryRefusal>;

    /// Resolve within exactly one tenant and connector.
    fn resolve(
        &self,
        tenant: &Tenant,
        connector: &str,
        label: &ConnectionLabel,
    ) -> Result<Option<InstanceId>, RegistryRefusal> {
        Ok(self
            .entries(tenant, connector)?
            .into_iter()
            .find(|entry| entry.label == *label)
            .map(|entry| entry.instance))
    }
}

type Values = BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>;

/// An in-memory label overlay for tests and embedding compositions that deliberately need no
/// restart durability.
#[derive(Debug, Default)]
pub struct MemoryConnectionRegistry {
    values: RwLock<Values>,
}

impl MemoryConnectionRegistry {
    fn read(&self) -> Result<RwLockReadGuard<'_, Values>, RegistryRefusal> {
        self.values
            .read()
            .map_err(|_| RegistryRefusal::Unavailable {
                reason: "the connection-registry lock is poisoned".to_owned(),
            })
    }

    fn write(&self) -> Result<RwLockWriteGuard<'_, Values>, RegistryRefusal> {
        self.values
            .write()
            .map_err(|_| RegistryRefusal::Unavailable {
                reason: "the connection-registry lock is poisoned".to_owned(),
            })
    }
}

fn entries(
    values: &Values,
    tenant: &Tenant,
    connector: &str,
) -> Result<Vec<NamedConnection>, RegistryRefusal> {
    values
        .get(tenant.as_str())
        .and_then(|connectors| connectors.get(connector))
        .into_iter()
        .flat_map(|labels| labels.iter())
        .map(|(label, instance)| {
            Ok(NamedConnection {
                label: ConnectionLabel::new(label.clone())?,
                instance: InstanceId::parse(instance).map_err(|reason| {
                    RegistryRefusal::Unavailable {
                        reason: format!("stored connection UUID `{instance}` is invalid: {reason}"),
                    }
                })?,
            })
        })
        .collect()
}

fn assign(
    values: &mut Values,
    tenant: &Tenant,
    connector: &str,
    label: &ConnectionLabel,
    instance: &InstanceId,
) -> Result<(), RegistryRefusal> {
    let labels = values
        .entry(tenant.as_str().to_owned())
        .or_default()
        .entry(connector.to_owned())
        .or_default();
    if labels.contains_key(label.as_str()) {
        return Err(RegistryRefusal::LabelAlreadyExists {
            connector: connector.to_owned(),
            label: label.to_string(),
        });
    }
    if labels.values().any(|held| held == instance.as_str()) {
        return Err(RegistryRefusal::InstanceAlreadyNamed {
            connector: connector.to_owned(),
            instance: instance.to_string(),
        });
    }
    labels.insert(label.to_string(), instance.to_string());
    Ok(())
}

fn rename(
    values: &mut Values,
    tenant: &Tenant,
    connector: &str,
    from: &ConnectionLabel,
    to: &ConnectionLabel,
) -> Result<InstanceId, RegistryRefusal> {
    let labels = values
        .get_mut(tenant.as_str())
        .and_then(|connectors| connectors.get_mut(connector))
        .ok_or_else(|| RegistryRefusal::UnknownLabel {
            connector: connector.to_owned(),
            label: from.to_string(),
        })?;
    if labels.contains_key(to.as_str()) {
        return Err(RegistryRefusal::LabelAlreadyExists {
            connector: connector.to_owned(),
            label: to.to_string(),
        });
    }
    let instance = labels
        .remove(from.as_str())
        .ok_or_else(|| RegistryRefusal::UnknownLabel {
            connector: connector.to_owned(),
            label: from.to_string(),
        })?;
    labels.insert(to.to_string(), instance.clone());
    InstanceId::parse(&instance).map_err(|reason| RegistryRefusal::Unavailable {
        reason: format!("stored connection UUID `{instance}` is invalid: {reason}"),
    })
}

fn remove(
    values: &mut Values,
    tenant: &Tenant,
    connector: &str,
    label: &ConnectionLabel,
) -> Result<InstanceId, RegistryRefusal> {
    let instance = values
        .get_mut(tenant.as_str())
        .and_then(|connectors| connectors.get_mut(connector))
        .and_then(|labels| labels.remove(label.as_str()))
        .ok_or_else(|| RegistryRefusal::UnknownLabel {
            connector: connector.to_owned(),
            label: label.to_string(),
        })?;
    InstanceId::parse(&instance).map_err(|reason| RegistryRefusal::Unavailable {
        reason: format!("stored connection UUID `{instance}` is invalid: {reason}"),
    })
}

impl ConnectionRegistry for MemoryConnectionRegistry {
    fn entries(
        &self,
        tenant: &Tenant,
        connector: &str,
    ) -> Result<Vec<NamedConnection>, RegistryRefusal> {
        let values = self.read()?;
        entries(&values, tenant, connector)
    }

    fn assign(
        &self,
        tenant: &Tenant,
        connector: &str,
        label: &ConnectionLabel,
        instance: &InstanceId,
    ) -> Result<(), RegistryRefusal> {
        let mut values = self.write()?;
        assign(&mut values, tenant, connector, label, instance)
    }

    fn rename(
        &self,
        tenant: &Tenant,
        connector: &str,
        from: &ConnectionLabel,
        to: &ConnectionLabel,
    ) -> Result<InstanceId, RegistryRefusal> {
        let mut values = self.write()?;
        rename(&mut values, tenant, connector, from, to)
    }

    fn remove(
        &self,
        tenant: &Tenant,
        connector: &str,
        label: &ConnectionLabel,
    ) -> Result<InstanceId, RegistryRefusal> {
        let mut values = self.write()?;
        remove(&mut values, tenant, connector, label)
    }
}

/// Why a label-registry operation was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryRefusal {
    /// The label is outside the closed grammar.
    #[error(
        "connection label `{label}` is invalid; use 1–64 ASCII alphanumeric, `-`, or `_` bytes"
    )]
    InvalidLabel {
        /// The rejected spelling. A label is public operator metadata, not a credential value.
        label: String,
    },
    /// The label is already occupied within the tenant and connector.
    #[error("connector `{connector}` already has a connection named `{label}` for this tenant")]
    LabelAlreadyExists {
        /// The connector scope.
        connector: String,
        /// The colliding label.
        label: String,
    },
    /// The immutable instance already has another name.
    #[error("connector `{connector}` instance `{instance}` already has a label for this tenant")]
    InstanceAlreadyNamed {
        /// The connector scope.
        connector: String,
        /// The host-minted UUID, which is an address and not a credential value.
        instance: String,
    },
    /// No row has this label within the tenant and connector.
    #[error("connector `{connector}` has no connection named `{label}` for this tenant")]
    UnknownLabel {
        /// The connector scope.
        connector: String,
        /// The missing label.
        label: String,
    },
    /// The registry could not safely answer or persist.
    #[error("the connection registry is unavailable: {reason}")]
    Unavailable {
        /// The actionable, value-free reason.
        reason: String,
    },
}

mod file {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::paths::{enclosing_working_tree, resolve};
    use crate::{private_fs, CONNECTION_REGISTRY_SETTING};

    /// A durable label overlay. The file contains labels and host-minted UUIDs, never credentials.
    #[derive(Debug)]
    pub struct ConnectionRegistryStore {
        path: PathBuf,
        values: RwLock<Values>,
    }

    impl ConnectionRegistryStore {
        /// Bind the configured path, refusing an absent or empty setting.
        pub fn bind_configured(
            configured: Option<&str>,
        ) -> Result<Self, ConnectionRegistryStoreError> {
            match configured.map(str::trim).filter(|value| !value.is_empty()) {
                Some(path) => Self::bind(path),
                None => Err(ConnectionRegistryStoreError::Unconfigured {
                    setting: CONNECTION_REGISTRY_SETTING,
                }),
            }
        }

        /// Open or create the registry outside every working tree.
        pub fn bind(path: impl AsRef<Path>) -> Result<Self, ConnectionRegistryStoreError> {
            let requested = path.as_ref();
            if requested.as_os_str().is_empty() {
                return Err(ConnectionRegistryStoreError::Unconfigured {
                    setting: CONNECTION_REGISTRY_SETTING,
                });
            }
            let resolved =
                resolve(requested).map_err(|error| ConnectionRegistryStoreError::Unresolvable {
                    path: requested.display().to_string(),
                    reason: error.to_string(),
                })?;
            if let Some(root) = enclosing_working_tree(&resolved) {
                return Err(ConnectionRegistryStoreError::InsideWorkingTree {
                    path: resolved.display().to_string(),
                    root: root.display().to_string(),
                });
            }
            let directory =
                resolved
                    .parent()
                    .ok_or_else(|| ConnectionRegistryStoreError::Unusable {
                        path: resolved.display().to_string(),
                        reason: "the store path has no parent directory".to_owned(),
                    })?;
            private_fs::ensure_directory(directory).map_err(|error| {
                ConnectionRegistryStoreError::Unusable {
                    path: resolved.display().to_string(),
                    reason: error.to_string(),
                }
            })?;
            let values = read(&resolved)?;
            validate(&resolved, &values)?;
            Ok(Self {
                path: resolved,
                values: RwLock::new(values),
            })
        }

        /// The bound path.
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// The startup line naming what the file contains.
        pub fn banner(&self) -> String {
            format!(
                "connection registry: {} (file store, labels and host-minted UUIDs only)",
                self.path.display()
            )
        }

        fn read_values(&self) -> Result<RwLockReadGuard<'_, Values>, RegistryRefusal> {
            self.values
                .read()
                .map_err(|_| RegistryRefusal::Unavailable {
                    reason: "the connection-registry lock is poisoned".to_owned(),
                })
        }

        fn write_values(&self) -> Result<RwLockWriteGuard<'_, Values>, RegistryRefusal> {
            self.values
                .write()
                .map_err(|_| RegistryRefusal::Unavailable {
                    reason: "the connection-registry lock is poisoned".to_owned(),
                })
        }

        fn persist(&self, values: &Values) -> Result<(), RegistryRefusal> {
            let unavailable = |reason: String| RegistryRefusal::Unavailable {
                reason: format!("{}: {reason}", self.path.display()),
            };
            let encoded = serde_json::to_vec_pretty(values)
                .map_err(|error| unavailable(error.to_string()))?;
            private_fs::write_atomic(&self.path, &encoded)
                .map_err(|error| unavailable(error.to_string()))
        }

        fn change<T>(
            &self,
            mutate: impl FnOnce(&mut Values) -> Result<T, RegistryRefusal>,
        ) -> Result<T, RegistryRefusal> {
            let mut values = self.write_values()?;
            let previous = values.clone();
            let answer = mutate(&mut values)?;
            if let Err(error) = self.persist(&values) {
                *values = previous;
                return Err(error);
            }
            Ok(answer)
        }
    }

    fn read(path: &Path) -> Result<Values, ConnectionRegistryStoreError> {
        let Some(raw) = private_fs::read(path, 1024 * 1024).map_err(|error| {
            ConnectionRegistryStoreError::Unusable {
                path: path.display().to_string(),
                reason: error.to_string(),
            }
        })?
        else {
            return Ok(Values::new());
        };
        if raw.is_empty() {
            return Ok(Values::new());
        }
        serde_json::from_slice(&raw).map_err(|error| ConnectionRegistryStoreError::Unusable {
            path: path.display().to_string(),
            reason: error.to_string(),
        })
    }

    fn validate(path: &Path, values: &Values) -> Result<(), ConnectionRegistryStoreError> {
        for (tenant, connectors) in values {
            Tenant::new(tenant.clone()).map_err(|error| {
                ConnectionRegistryStoreError::Unusable {
                    path: path.display().to_string(),
                    reason: error.to_string(),
                }
            })?;
            for (connector, labels) in connectors {
                let mut instances = BTreeSet::new();
                for (label, instance) in labels {
                    ConnectionLabel::new(label.clone()).map_err(|error| {
                        ConnectionRegistryStoreError::Unusable {
                            path: path.display().to_string(),
                            reason: error.to_string(),
                        }
                    })?;
                    InstanceId::parse(instance).map_err(|reason| {
                        ConnectionRegistryStoreError::Unusable {
                            path: path.display().to_string(),
                            reason: format!(
                                "tenant `{tenant}` connector `{connector}` has invalid UUID `{instance}`: {reason}"
                            ),
                        }
                    })?;
                    if !instances.insert(instance) {
                        return Err(ConnectionRegistryStoreError::Unusable {
                            path: path.display().to_string(),
                            reason: format!(
                                "tenant `{tenant}` connector `{connector}` gives UUID `{instance}` more than one label"
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    impl ConnectionRegistry for ConnectionRegistryStore {
        fn entries(
            &self,
            tenant: &Tenant,
            connector: &str,
        ) -> Result<Vec<NamedConnection>, RegistryRefusal> {
            let values = self.read_values()?;
            super::entries(&values, tenant, connector)
        }

        fn assign(
            &self,
            tenant: &Tenant,
            connector: &str,
            label: &ConnectionLabel,
            instance: &InstanceId,
        ) -> Result<(), RegistryRefusal> {
            self.change(|values| super::assign(values, tenant, connector, label, instance))
        }

        fn rename(
            &self,
            tenant: &Tenant,
            connector: &str,
            from: &ConnectionLabel,
            to: &ConnectionLabel,
        ) -> Result<InstanceId, RegistryRefusal> {
            self.change(|values| super::rename(values, tenant, connector, from, to))
        }

        fn remove(
            &self,
            tenant: &Tenant,
            connector: &str,
            label: &ConnectionLabel,
        ) -> Result<InstanceId, RegistryRefusal> {
            self.change(|values| super::remove(values, tenant, connector, label))
        }
    }

    /// Why the configured durable registry could not be bound.
    #[derive(Debug, thiserror::Error)]
    pub enum ConnectionRegistryStoreError {
        /// No path was selected.
        #[error("no connection-registry store is configured: set `{setting}` to a path outside every working tree")]
        Unconfigured {
            /// The setting to configure.
            setting: &'static str,
        },
        /// The path resolves inside a checkout.
        #[error("refusing a connection-registry store at `{path}` inside working tree `{root}`")]
        InsideWorkingTree {
            /// The resolved path.
            path: String,
            /// The containing checkout.
            root: String,
        },
        /// The path could not be resolved.
        #[error("connection-registry store `{path}` cannot be resolved: {reason}")]
        Unresolvable {
            /// The configured spelling.
            path: String,
            /// The IO reason.
            reason: String,
        },
        /// The file could not be read, created, or parsed.
        #[error("connection-registry store `{path}` is unusable: {reason}")]
        Unusable {
            /// The resolved path.
            path: String,
            /// The IO or format reason.
            reason: String,
        },
    }
}

pub use file::{ConnectionRegistryStore, ConnectionRegistryStoreError};

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    #[cfg(unix)]
    use std::path::PathBuf;
    #[cfg(unix)]
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tenant(name: &str) -> Tenant {
        Tenant::new(name).expect("plain tenant")
    }

    fn instance(text: &str) -> InstanceId {
        InstanceId::parse(text).expect("canonical instance")
    }

    #[cfg(unix)]
    struct Scratch(PathBuf);

    #[cfg(unix)]
    impl Scratch {
        fn new(prefix: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "{prefix}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            crate::ensure_private_state_directory(&path).expect("a scratch directory");
            Self(path)
        }
    }

    #[cfg(unix)]
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn labels_are_closed_and_rename_moves_no_uuid() {
        for accepted in ["prod", "sandbox_2", "EU-west"] {
            assert_eq!(ConnectionLabel::new(accepted).unwrap().as_str(), accepted);
        }
        for refused in ["", "with space", "a/b", "é"] {
            assert!(ConnectionLabel::new(refused).is_err(), "{refused:?}");
        }

        let registry = MemoryConnectionRegistry::default();
        let tenant = tenant("acme");
        let id = instance("0d3f79ae-b6df-4f77-8f77-438436c3b2ef");
        let old = ConnectionLabel::new("old").unwrap();
        let new = ConnectionLabel::new("new").unwrap();
        registry.assign(&tenant, "zendesk", &old, &id).unwrap();

        assert_eq!(registry.rename(&tenant, "zendesk", &old, &new).unwrap(), id);
        assert_eq!(registry.resolve(&tenant, "zendesk", &old).unwrap(), None);
        assert_eq!(
            registry.resolve(&tenant, "zendesk", &new).unwrap(),
            Some(id)
        );
    }

    /// X-14's tenant-isolation acceptance at the store boundary. The same spelling is resolved
    /// under the principal's tenant and cannot reach the other tenant's row.
    #[test]
    fn a_label_never_resolves_across_tenants() {
        let registry = MemoryConnectionRegistry::default();
        let acme = tenant("acme");
        let globex = tenant("globex");
        let label = ConnectionLabel::new("prod").unwrap();
        let id = instance("0d3f79ae-b6df-4f77-8f77-438436c3b2ef");
        registry.assign(&globex, "zendesk", &label, &id).unwrap();

        assert_eq!(registry.resolve(&acme, "zendesk", &label).unwrap(), None);
        assert_eq!(
            registry.resolve(&globex, "zendesk", &label).unwrap(),
            Some(id)
        );
    }

    /// The production registry is not merely a process-local implementation of the port. A label
    /// written before a restart resolves to the same immutable UUID afterwards.
    #[cfg(unix)]
    #[test]
    fn the_file_registry_survives_restart() {
        let scratch = Scratch::new("flux-exchange-instance-registry");
        let path = scratch.0.join("connections.json");
        let tenant = tenant("acme");
        let label = ConnectionLabel::new("prod").unwrap();
        let id = instance("0d3f79ae-b6df-4f77-8f77-438436c3b2ef");

        ConnectionRegistryStore::bind(&path)
            .expect("a registry outside the worktree")
            .assign(&tenant, "zendesk", &label, &id)
            .expect("the row is durable");

        let reopened = ConnectionRegistryStore::bind(&path).expect("the registry reopens");
        assert_eq!(
            reopened.resolve(&tenant, "zendesk", &label).unwrap(),
            Some(id),
        );
        assert_eq!(
            std::fs::metadata(path)
                .expect("the registry file")
                .permissions()
                .mode()
                & 0o777,
            0o600,
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_file_cannot_restore_two_labels_for_one_instance() {
        let scratch = Scratch::new("flux-exchange-duplicate-instance");
        let path = scratch.0.join("connections.json");
        let id = "0d3f79ae-b6df-4f77-8f77-438436c3b2ef";
        crate::write_private_state_file(
            &path,
            format!(r#"{{"acme":{{"zendesk":{{"prod":"{id}","alias":"{id}"}}}}}}"#).as_bytes(),
        )
        .expect("a planted registry");

        let refusal = ConnectionRegistryStore::bind(&path).expect_err("duplicate UUIDs refuse");
        assert!(refusal.to_string().contains("more than one label"));
    }
}
