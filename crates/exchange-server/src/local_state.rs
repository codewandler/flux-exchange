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
            apps: None,
        })
    }
}

/// Resolve the process configuration once, before opening any store.
pub fn configured(development: bool) -> Result<Option<LocalStatePaths>, LocalStateRefusal> {
    let configured = read_explicit()?;
    let root = std::env::var_os(LOCAL_STATE_SETTING)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);

    if development || root.is_some() {
        let root = match root {
            Some(root) => root,
            None => conventional_root()?,
        };
        let root = ensure_owner_only_root(&root)?;
        return Ok(Some(
            LocalStatePaths::development(&root).with_explicit(&configured),
        ));
    }

    if configured.is_empty() {
        Ok(None)
    } else {
        LocalStatePaths::explicit(&configured).map(Some)
    }
}

fn read_explicit() -> Result<BTreeMap<&'static str, String>, LocalStateRefusal> {
    SETTINGS
        .into_iter()
        .filter_map(|setting| std::env::var(setting).ok().map(|value| (setting, value)))
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
    #[cfg(target_os = "windows")]
    const BASE: &str = "LOCALAPPDATA";
    #[cfg(not(target_os = "windows"))]
    const BASE: &str = "XDG_STATE_HOME";

    #[cfg(target_os = "windows")]
    if let Some(base) = std::env::var_os(BASE).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(base).join("Flux/Exchange"));
    }

    #[cfg(not(target_os = "windows"))]
    if let Some(base) = std::env::var_os(BASE).filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(base).join("flux-exchange"));
    }

    let home = std::env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .filter(|value| !value.is_empty())
    .ok_or(LocalStateRefusal::NoPerUserRoot { setting: BASE })?;

    #[cfg(target_os = "macos")]
    return Ok(PathBuf::from(home).join("Library/Application Support/Flux/Exchange"));
    #[cfg(all(unix, not(target_os = "macos")))]
    return Ok(PathBuf::from(home).join(".local/state/flux-exchange"));
    #[cfg(target_os = "windows")]
    return Ok(PathBuf::from(home).join("AppData/Local/Flux/Exchange"));
    #[allow(unreachable_code)]
    Err(LocalStateRefusal::UnsupportedPlatform)
}

#[cfg(unix)]
fn ensure_owner_only_root(root: &Path) -> Result<PathBuf, LocalStateRefusal> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _, PermissionsExt as _};

    match std::fs::symlink_metadata(root) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(LocalStateRefusal::UnsafeRoot {
                    path: root.to_path_buf(),
                    reason: "it is not a directory".to_owned(),
                });
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(root)
                .map_err(|source| LocalStateRefusal::UnusableRoot {
                    path: root.to_path_buf(),
                    source,
                })?;
        }
        Err(source) => {
            return Err(LocalStateRefusal::UnusableRoot {
                path: root.to_path_buf(),
                source,
            })
        }
    }
    // Inspect after creation too: a umask may narrow creation but can never make it wider, while a
    // concurrently replaced object must still be refused as what is actually present now.
    let metadata =
        std::fs::symlink_metadata(root).map_err(|source| LocalStateRefusal::UnusableRoot {
            path: root.to_path_buf(),
            source,
        })?;
    if !metadata.file_type().is_dir() {
        return Err(LocalStateRefusal::UnsafeRoot {
            path: root.to_path_buf(),
            reason: "it is not a directory".to_owned(),
        });
    }
    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o700 {
        return Err(LocalStateRefusal::UnsafeRoot {
            path: root.to_path_buf(),
            reason: format!(
                "its mode is {mode:04o}; create a private child directory at mode 0700. Exchange did not change the existing metadata"
            ),
        });
    }
    // SAFETY: `geteuid` takes no pointer, has no preconditions, and reads process identity. It is
    // the authority the filesystem checks; HOME may name another user under sudo.
    let process_owner = unsafe { libc::geteuid() };
    if metadata.uid() != process_owner {
        return Err(LocalStateRefusal::UnsafeRoot {
            path: root.to_path_buf(),
            reason: format!(
                "it is owned by uid {}, not the process effective uid {process_owner}; Exchange did not change the owner",
                metadata.uid()
            ),
        });
    }

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

#[cfg(windows)]
fn ensure_owner_only_root(root: &Path) -> Result<PathBuf, LocalStateRefusal> {
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
    Incomplete {
        configured: Vec<&'static str>,
        missing: Vec<&'static str>,
    },
    NoPerUserRoot {
        setting: &'static str,
    },
    UnsupportedPlatform,
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
            Self::Incomplete {
                configured,
                missing,
            } => write!(
                formatter,
                "persistent local state is all-or-nothing: configured {}, but missing {}",
                configured.join(", "),
                missing.join(", ")
            ),
            Self::NoPerUserRoot { setting } => write!(
                formatter,
                "cannot select a conventional per-user Exchange state root because {setting} and the platform home setting are unset"
            ),
            Self::UnsupportedPlatform => {
                formatter.write_str("this platform has no supported local-state root")
            }
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
