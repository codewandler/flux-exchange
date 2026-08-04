//! One complete durable local composition, selected before any individual store opens.
//!
//! Store modules still own their formats and refusal semantics. This module owns the composition
//! question they cannot answer separately: either every authority-bearing path is available, or
//! none of a requested persistent composition is opened.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use exchange_host::{
    CHANNEL_STORE_SETTING, CONNECTION_REGISTRY_SETTING, CONNECTION_SETTINGS_SETTING,
    CREDENTIAL_STORE_SETTING, GRANT_STORE_SETTING, WORKFLOW_STORE_SETTING,
};

use crate::audit::AUDIT_SETTING;
use crate::service_account::SERVICE_ACCOUNT_STORE_SETTING;

/// Optional root override for the complete local composition.
pub const LOCAL_STATE_SETTING: &str = "FLUX_EXCHANGE_STATE";

const SETTINGS: [&str; 8] = [
    CREDENTIAL_STORE_SETTING,
    CONNECTION_SETTINGS_SETTING,
    GRANT_STORE_SETTING,
    CONNECTION_REGISTRY_SETTING,
    CHANNEL_STORE_SETTING,
    WORKFLOW_STORE_SETTING,
    AUDIT_SETTING,
    SERVICE_ACCOUNT_STORE_SETTING,
];

/// Explicit paths for every durable port the local server composition requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalStatePaths {
    pub credential: PathBuf,
    pub settings: PathBuf,
    pub grants: PathBuf,
    pub connections: PathBuf,
    pub channels: PathBuf,
    pub workflows: PathBuf,
    pub audit: PathBuf,
    pub service_accounts: PathBuf,
    /// Value-free prepared-transaction recovery state below the authenticated owner root.
    pub coordinator: Option<PathBuf>,
    /// Authenticated owner root containing value-free per-label credential heads.
    pub credential_heads_root: Option<PathBuf>,
    pub apps: Option<PathBuf>,
}

impl LocalStatePaths {
    /// Fixed store layout below one private root. Logical store formats remain unchanged.
    pub fn development(root: &Path) -> Self {
        Self {
            credential: root.join("credentials/store.txt"),
            settings: root.join("settings/store.json"),
            grants: root.join("grants/store.json"),
            connections: root.join("connections/store.json"),
            channels: root.join("channels/store.json"),
            workflows: root.join("workflows"),
            audit: root.join("audit/events.sqlite3"),
            service_accounts: root.join("service-accounts/store.json"),
            coordinator: Some(root.join("coordinator/transactions.sqlite3")),
            credential_heads_root: Some(root.to_path_buf()),
            apps: Some(root.join("apps")),
        }
    }

    fn with_explicit(mut self, configured: &BTreeMap<&'static str, String>) -> Self {
        if let Some(path) = configured.get(CREDENTIAL_STORE_SETTING) {
            self.credential = PathBuf::from(path);
        }
        if let Some(path) = configured.get(CONNECTION_SETTINGS_SETTING) {
            self.settings = PathBuf::from(path);
        }
        if let Some(path) = configured.get(GRANT_STORE_SETTING) {
            self.grants = PathBuf::from(path);
        }
        if let Some(path) = configured.get(CONNECTION_REGISTRY_SETTING) {
            self.connections = PathBuf::from(path);
        }
        if let Some(path) = configured.get(CHANNEL_STORE_SETTING) {
            self.channels = PathBuf::from(path);
        }
        if let Some(path) = configured.get(WORKFLOW_STORE_SETTING) {
            self.workflows = PathBuf::from(path);
        }
        if let Some(path) = configured.get(AUDIT_SETTING) {
            self.audit = PathBuf::from(path);
        }
        if let Some(path) = configured.get(SERVICE_ACCOUNT_STORE_SETTING) {
            self.service_accounts = PathBuf::from(path);
        }
        self
    }

    fn explicit(configured: &BTreeMap<&'static str, String>) -> Result<Self, LocalStateRefusal> {
        let missing: Vec<&str> = SETTINGS
            .into_iter()
            .filter(|setting| !configured.contains_key(setting))
            .collect();
        if !missing.is_empty() {
            return Err(LocalStateRefusal::Incomplete {
                configured: configured.keys().copied().collect(),
                missing,
            });
        }
        Ok(Self {
            credential: PathBuf::from(&configured[CREDENTIAL_STORE_SETTING]),
            settings: PathBuf::from(&configured[CONNECTION_SETTINGS_SETTING]),
            grants: PathBuf::from(&configured[GRANT_STORE_SETTING]),
            connections: PathBuf::from(&configured[CONNECTION_REGISTRY_SETTING]),
            channels: PathBuf::from(&configured[CHANNEL_STORE_SETTING]),
            workflows: PathBuf::from(&configured[WORKFLOW_STORE_SETTING]),
            audit: PathBuf::from(&configured[AUDIT_SETTING]),
            service_accounts: PathBuf::from(&configured[SERVICE_ACCOUNT_STORE_SETTING]),
            coordinator: None,
            credential_heads_root: None,
            apps: None,
        })
    }
}

/// Resolve the process configuration once, before opening any store.
pub fn configured(development: bool) -> Result<Option<LocalStatePaths>, LocalStateRefusal> {
    let root = std::env::var_os(LOCAL_STATE_SETTING)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    select(development, read_explicit(), root)
}

fn select(
    development: bool,
    configured: Result<BTreeMap<&'static str, String>, LocalStateRefusal>,
    root: Option<PathBuf>,
) -> Result<Option<LocalStatePaths>, LocalStateRefusal> {
    // An explicit setting that cannot be represented must refuse before a root is created. Moving
    // this `?` below development selection would silently replace operator authority with defaults.
    let configured = configured?;

    if development || root.is_some() {
        let root = owner_root(root)?;
        return Ok(Some(
            LocalStatePaths::development(&root).with_explicit(&configured),
        ));
    }

    if configured.is_empty() {
        Ok(None)
    } else {
        let owner = owner_root(None)?;
        let mut paths = LocalStatePaths::explicit(&configured)?;
        paths.coordinator = Some(owner.join("coordinator/transactions.sqlite3"));
        paths.credential_heads_root = Some(owner);
        Ok(Some(paths))
    }
}

fn owner_root(requested: Option<PathBuf>) -> Result<PathBuf, LocalStateRefusal> {
    #[cfg(windows)]
    let account_default = requested.is_none();
    let root = requested.map_or_else(conventional_root, Ok)?;
    #[cfg(windows)]
    let root = ensure_owner_only_root(&root, account_default)?;
    #[cfg(not(windows))]
    let root = ensure_owner_only_root(&root)?;
    Ok(root)
}

fn read_explicit() -> Result<BTreeMap<&'static str, String>, LocalStateRefusal> {
    read_explicit_with(std::env::var)
}

fn read_explicit_with(
    mut read: impl FnMut(&'static str) -> Result<String, std::env::VarError>,
) -> Result<BTreeMap<&'static str, String>, LocalStateRefusal> {
    SETTINGS
        .into_iter()
        .filter_map(|setting| match read(setting) {
            Ok(value) => Some(Ok((setting, value))),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                Some(Err(LocalStateRefusal::NotUnicode { setting }))
            }
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|(setting, value)| {
            let value = value.trim();
            if value.is_empty() {
                Err(LocalStateRefusal::Empty { setting })
            } else {
                Ok((setting, value.to_owned()))
            }
        })
        .collect()
}

fn conventional_root() -> Result<PathBuf, LocalStateRefusal> {
    crate::native_root::authenticated_account_state_root()
        .map_err(|reason| LocalStateRefusal::NoPerUserRoot { reason })
}

#[cfg(unix)]
fn ensure_owner_only_root(root: &Path) -> Result<PathBuf, LocalStateRefusal> {
    use std::ffi::CString;
    use std::os::fd::{AsRawFd as _, FromRawFd as _};
    use std::os::unix::ffi::OsStrExt as _;
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    use std::path::Component;

    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| LocalStateRefusal::UnusableRoot {
                path: root.to_path_buf(),
                source,
            })?
            .join(root)
    };
    if absolute
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(LocalStateRefusal::UnsafeRoot {
            path: absolute,
            reason: "it contains a parent-directory traversal; name the owner root directly"
                .to_owned(),
        });
    }

    let mut directory =
        std::fs::File::open("/").map_err(|source| LocalStateRefusal::UnusableRoot {
            path: PathBuf::from("/"),
            source,
        })?;
    // SAFETY: `geteuid` has no pointer arguments and returns the identity enforced by openat.
    let process_owner = unsafe { libc::geteuid() };
    let names = absolute
        .components()
        .filter_map(|component| match component {
            Component::Normal(name) => Some(name),
            Component::RootDir | Component::CurDir => None,
            Component::Prefix(_) | Component::ParentDir => unreachable!("checked above"),
        })
        .collect::<Vec<_>>();
    let mut inspected = PathBuf::from("/");
    let mut owner_boundary = false;

    for (index, name) in names.iter().enumerate() {
        inspected.push(name);
        let final_component = index + 1 == names.len();
        let native_name =
            CString::new(name.as_bytes()).map_err(|_| LocalStateRefusal::UnsafeRoot {
                path: inspected.clone(),
                reason: "a path component contains a NUL byte".to_owned(),
            })?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: the parent descriptor and NUL-terminated component remain live for the call.
        let mut descriptor =
            unsafe { libc::openat(directory.as_raw_fd(), native_name.as_ptr(), flags) };
        if descriptor < 0 {
            let refusal = io::Error::last_os_error();
            if refusal.kind() != io::ErrorKind::NotFound {
                let reason = if matches!(refusal.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
                {
                    "it is a symlink or not a directory; Exchange did not follow or replace it"
                        .to_owned()
                } else {
                    format!("native no-follow traversal refused it: {refusal}")
                };
                return Err(LocalStateRefusal::UnsafeRoot {
                    path: inspected,
                    reason,
                });
            }
            if !owner_boundary {
                return Err(LocalStateRefusal::UnsafeRoot {
                    path: inspected,
                    reason: "its missing parent chain has no existing authenticated-owner boundary; create an owner-only directory first. Exchange did not create or repair a shared ancestor".to_owned(),
                });
            }
            // SAFETY: same live parent/component as openat; the kernel applies the requested mode
            // atomically at creation and the descriptor is reopened and inspected immediately.
            let created =
                unsafe { libc::mkdirat(directory.as_raw_fd(), native_name.as_ptr(), 0o700) };
            if created != 0 {
                return Err(LocalStateRefusal::UnusableRoot {
                    path: inspected,
                    source: io::Error::last_os_error(),
                });
            }
            descriptor =
                unsafe { libc::openat(directory.as_raw_fd(), native_name.as_ptr(), flags) };
            if descriptor < 0 {
                return Err(LocalStateRefusal::UnusableRoot {
                    path: inspected,
                    source: io::Error::last_os_error(),
                });
            }
        }
        // SAFETY: a nonnegative openat return owns one new descriptor, transferred exactly once.
        let next = unsafe { std::fs::File::from_raw_fd(descriptor) };
        let metadata = next
            .metadata()
            .map_err(|source| LocalStateRefusal::UnusableRoot {
                path: inspected.clone(),
                source,
            })?;
        let mode = metadata.permissions().mode() & 0o7777;
        let writable_by_untrusted = mode & 0o022 != 0;
        let sticky_shared = !owner_boundary && mode & 0o1000 != 0 && !final_component;

        if owner_boundary {
            if metadata.uid() != process_owner || writable_by_untrusted {
                return Err(LocalStateRefusal::UnsafeRoot {
                    path: inspected,
                    reason: format!(
                        "an ancestor below the authenticated-owner boundary is owned by uid {} with mode {mode:04o}; it must remain owned by effective uid {process_owner} and not writable by untrusted accounts. Exchange did not chmod or chown it",
                        metadata.uid()
                    ),
                });
            }
        } else if metadata.uid() == process_owner && !writable_by_untrusted {
            owner_boundary = true;
        } else if writable_by_untrusted && !sticky_shared {
            return Err(LocalStateRefusal::UnsafeRoot {
                path: inspected,
                reason: format!(
                    "a shared ancestor is writable by untrusted accounts (mode {mode:04o}); create an owner-only child boundary first. Exchange did not chmod or chown it"
                ),
            });
        }

        if final_component
            && (metadata.uid() != process_owner || mode & 0o777 != 0o700 || !owner_boundary)
        {
            return Err(LocalStateRefusal::UnsafeRoot {
                path: inspected,
                reason: format!(
                    "the Exchange root is owned by uid {} with mode {mode:04o}; it must be owned by effective uid {process_owner} at mode 0700. Exchange did not chmod or chown it",
                    metadata.uid()
                ),
            });
        }
        directory = next;
    }

    if let Some(worktree) = absolute
        .ancestors()
        .find(|path| path.join(".git").exists())
        .map(Path::to_path_buf)
    {
        return Err(LocalStateRefusal::UnsafeRoot {
            path: absolute,
            reason: format!(
                "it is inside the working tree at {}; Exchange did not create a store there",
                worktree.display()
            ),
        });
    }
    Ok(absolute)
}

#[cfg(windows)]
fn ensure_owner_only_root(
    root: &Path,
    authenticated_account_default: bool,
) -> Result<PathBuf, LocalStateRefusal> {
    if authenticated_account_default {
        return crate::native_root::ensure_authenticated_account_state_root(root).map_err(
            |reason| LocalStateRefusal::UnsafeRoot {
                path: root.to_path_buf(),
                reason,
            },
        );
    }
    exchange_host::ensure_private_state_directory(root).map_err(|source| {
        LocalStateRefusal::UnsafeRoot {
            path: root.to_path_buf(),
            reason: format!(
                "native owner/protected-DACL inspection refused it: {source}; Exchange did not change the existing security descriptor"
            ),
        }
    })?;
    let resolved = root
        .canonicalize()
        .map_err(|source| LocalStateRefusal::UnusableRoot {
            path: root.to_path_buf(),
            source,
        })?;
    if let Some(worktree) = resolved
        .ancestors()
        .find(|path| path.join(".git").exists())
        .map(Path::to_path_buf)
    {
        return Err(LocalStateRefusal::UnsafeRoot {
            path: resolved,
            reason: format!(
                "it is inside the working tree at {}; Exchange did not create a store there",
                worktree.display()
            ),
        });
    }
    Ok(resolved)
}

/// A complete local composition could not be selected safely.
#[derive(Debug)]
pub enum LocalStateRefusal {
    Empty {
        setting: &'static str,
    },
    NotUnicode {
        setting: &'static str,
    },
    Incomplete {
        configured: Vec<&'static str>,
        missing: Vec<&'static str>,
    },
    NoPerUserRoot {
        reason: String,
    },
    UnsafeRoot {
        path: PathBuf,
        reason: String,
    },
    UnusableRoot {
        path: PathBuf,
        source: io::Error,
    },
}

impl std::fmt::Display for LocalStateRefusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty { setting } => write!(formatter, "{setting} is set but empty"),
            Self::NotUnicode { setting } => write!(
                formatter,
                "{setting} is not valid Unicode; refusing rather than treating an explicit store path as unset or replacing it with a development default"
            ),
            Self::Incomplete {
                configured,
                missing,
            } => write!(
                formatter,
                "persistent local state is all-or-nothing: configured {}, but missing {}",
                configured.join(", "),
                missing.join(", ")
            ),
            Self::NoPerUserRoot { reason } => write!(
                formatter,
                "cannot select the authenticated account's conventional Exchange state root: {reason}"
            ),
            Self::UnsafeRoot { path, reason } => write!(
                formatter,
                "refusing local state root `{}`: {reason}. Use {LOCAL_STATE_SETTING} to name a conventional owner-only root outside every working tree",
                path.display()
            ),
            Self::UnusableRoot { path, source } => write!(
                formatter,
                "cannot create or inspect local state root `{}`: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for LocalStateRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_defaults_bind_every_durable_store_below_one_root() {
        let root = Path::new("/outside/checkout/flux-exchange");
        let paths = LocalStatePaths::development(root);

        assert_eq!(paths.credential, root.join("credentials/store.txt"));
        assert_eq!(paths.settings, root.join("settings/store.json"));
        assert_eq!(paths.grants, root.join("grants/store.json"));
        assert_eq!(paths.connections, root.join("connections/store.json"));
        assert_eq!(paths.channels, root.join("channels/store.json"));
        assert_eq!(paths.workflows, root.join("workflows"));
        assert_eq!(paths.audit, root.join("audit/events.sqlite3"));
        assert_eq!(
            paths.service_accounts,
            root.join("service-accounts/store.json")
        );
        assert_eq!(paths.apps, Some(root.join("apps")));
    }

    #[test]
    fn one_explicit_store_never_creates_a_partial_persistent_composition() {
        let configured =
            BTreeMap::from([(CREDENTIAL_STORE_SETTING, "/private/credentials".into())]);
        let refusal = LocalStatePaths::explicit(&configured).expect_err("seven stores are missing");
        let message = refusal.to_string();

        assert!(message.contains(CREDENTIAL_STORE_SETTING), "{message}");
        assert!(message.contains(GRANT_STORE_SETTING), "{message}");
        assert!(message.contains(AUDIT_SETTING), "{message}");
        assert!(!message.contains("chmod 700 /tmp"), "{message}");
    }

    #[test]
    fn an_explicit_development_store_path_remains_authoritative() {
        let root = Path::new("/conventional/flux-exchange");
        let explicit = BTreeMap::from([(
            GRANT_STORE_SETTING,
            "/operator-selected/grants.json".to_owned(),
        )]);

        let paths = LocalStatePaths::development(root).with_explicit(&explicit);

        assert_eq!(paths.grants, Path::new("/operator-selected/grants.json"));
        assert_eq!(paths.credential, root.join("credentials/store.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn a_non_unicode_explicit_path_refuses_before_development_can_create_defaults() {
        use std::os::unix::ffi::OsStringExt as _;

        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x127-non-unicode-default-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let non_unicode = std::ffi::OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff]);

        let explicit = read_explicit_with(|setting| {
            if setting == GRANT_STORE_SETTING {
                Err(std::env::VarError::NotUnicode(non_unicode.clone()))
            } else {
                Err(std::env::VarError::NotPresent)
            }
        });
        let refusal = select(true, explicit, Some(root.clone()))
            .expect_err("an explicit path must never disappear into the development defaults");

        assert!(matches!(
            refusal,
            LocalStateRefusal::NotUnicode {
                setting: GRANT_STORE_SETTING
            }
        ));
        let message = refusal.to_string();
        assert!(message.contains(GRANT_STORE_SETTING), "{message}");
        assert!(
            !message.contains(non_unicode.to_string_lossy().as_ref()),
            "{message}"
        );
        assert!(!root.exists(), "no conventional default root was created");
    }

    #[cfg(unix)]
    #[test]
    fn a_widened_existing_root_is_refused_without_repair() {
        use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};

        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x127-wide-root-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::DirBuilder::new()
            .mode(0o755)
            .create(&root)
            .expect("fixture root");

        let refusal = ensure_owner_only_root(&root).expect_err("0755 must refuse");
        let message = refusal.to_string();
        assert!(message.contains(&root.display().to_string()), "{message}");
        assert!(message.contains("private child directory"), "{message}");
        assert!(!message.contains("chmod 700 /tmp"), "{message}");
        assert_eq!(
            std::fs::metadata(&root)
                .expect("metadata after refusal")
                .permissions()
                .mode()
                & 0o777,
            0o755,
            "refusal must not repair the planted mode"
        );
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("test cleanup");
        std::fs::remove_dir(&root).expect("cleanup");
    }
}
