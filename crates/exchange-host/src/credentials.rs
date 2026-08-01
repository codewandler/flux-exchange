//! Where this host keeps credentials, and what does not protect them.
//!
//! The store itself is [`connector_secrets::FileStore`] — one `0600` file in a `0700` directory,
//! both modes set in the `open(2)`/`mkdir(2)` call rather than `chmod`-ed afterwards and re-checked
//! every time the store is opened, and every mutation a whole-file rewrite through a sibling
//! temporary, `fsync` and `rename(2)`. None of that is reimplemented here: this host builds a
//! *view* of an upstream model and never a second model of one, the same way
//! [`ConnectorSurface`](crate::ConnectorSurface) is a view of a connector manifest.
//!
//! What this module adds is the part that is about *this* host being started correctly.
//!
//! - **A path inside a working tree is refused.** A credential kept under a checkout is one
//!   `git add -A` from being committed, and that is not a risk worth leaving to attention.
//! - **There is no fallback.** A configuration that names no store is a startup error naming what
//!   would have worked. A host that quietly fell back to memory would start, serve every route
//!   correctly, look exactly like a working one, and lose every credential on restart.
//! - **The banner reads its path back off the store that was bound**, so it cannot describe a
//!   store this process is not holding.
//!
//! # What protects a value here, stated plainly
//!
//! A file mode, and nothing else. There is no encryption at rest, no passphrase, no OS keychain,
//! and no protection from `root` or from a backup that copies the file. That is the right store for
//! a single-operator deployment and the wrong one for a shared machine; `connector-secrets` ships a
//! Vault-backed store for the second case, behind its `vault` feature.
//!
//! # Removing a store
//!
//! Remove the **directory**, not the file. A write interrupted between the `fsync` and the
//! `rename(2)` leaves a complete `0600` copy of every credential in a sibling temporary, and `rm`
//! on the store file alone does not touch it. `FileStore` reaps those the next time it opens the
//! store — but a store being deleted is one nobody is going to open again.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use connector_secrets::{FileStore, SecretStore, StoreError};

/// The setting a composing binary reads the store path from.
///
/// The host does not read the environment itself — a binary passes the value it found to
/// [`CredentialStore::bind_configured`]. The *name* lives here so the refusal below and the reader
/// that produced the value cannot drift apart into two different spellings.
pub const CREDENTIAL_STORE_SETTING: &str = "FLUX_EXCHANGE_CREDENTIALS";

/// A location that would have worked, quoted in every refusal.
///
/// Written with `$HOME` rather than expanded: nothing here reads the environment, and a refusal
/// that quoted this machine's home directory would be a refusal that had already made a choice.
const EXAMPLE_PATH: &str = "$HOME/.local/share/flux-exchange/credentials";

/// The credential store this process is holding, bound at startup.
///
/// Construct one with [`bind`](Self::bind) or [`bind_configured`](Self::bind_configured), and hand
/// [`secrets`](Self::secrets) to whatever resolves a credential. Injection at construction is the
/// intended shape — there is no global to look one up from, and no constructor that succeeds
/// without a durable store behind it.
#[derive(Debug)]
pub struct CredentialStore {
    // An `Arc` because one bound store serves every route: the binding happens once, at startup,
    // and is shared rather than reopened.
    store: Arc<FileStore>,
}

impl CredentialStore {
    /// Bind the store a configuration value names, or refuse.
    ///
    /// `configured` is what the operator set — `None` when nothing was set at all, which is a
    /// startup error rather than a default. Surrounding whitespace is trimmed and a value that is
    /// then empty counts as unset, because `FLUX_EXCHANGE_CREDENTIALS=` in an environment file is
    /// an operator who has not chosen a path, not one who has chosen the current directory.
    ///
    /// # Errors
    ///
    /// [`CredentialStoreError::Unconfigured`] when nothing was named, and otherwise whatever
    /// [`bind`](Self::bind) refuses with.
    pub fn bind_configured(configured: Option<&str>) -> Result<Self, CredentialStoreError> {
        match configured.map(str::trim).filter(|value| !value.is_empty()) {
            Some(path) => Self::bind(path),
            None => Err(CredentialStoreError::Unconfigured {
                setting: CREDENTIAL_STORE_SETTING,
            }),
        }
    }

    /// Open — or create — the file store at `path`, or refuse.
    ///
    /// The path is resolved against the current directory and through any symlink that already
    /// exists, so what is checked below is the location the store will really occupy rather than
    /// the spelling it was given.
    ///
    /// # Errors
    ///
    /// [`CredentialStoreError::Unconfigured`] for an empty path, which is not a location;
    /// [`CredentialStoreError::InsideWorkingTree`] for a path under a checkout;
    /// [`CredentialStoreError::Unresolvable`] when the path cannot be made absolute at all; and
    /// [`CredentialStoreError::Unusable`] when the store itself refuses — a widened mode, a
    /// directory that cannot be created, a file it cannot parse.
    pub fn bind(path: impl AsRef<Path>) -> Result<Self, CredentialStoreError> {
        let requested = path.as_ref();
        if requested.as_os_str().is_empty() {
            return Err(CredentialStoreError::Unconfigured {
                setting: CREDENTIAL_STORE_SETTING,
            });
        }

        let resolved = resolve(requested).map_err(|error| CredentialStoreError::Unresolvable {
            path: requested.display().to_string(),
            reason: error.to_string(),
        })?;

        // Checked before the store is opened, so a refused path is one nothing was created at:
        // opening first would leave the directory — and the file — inside the checkout it refused.
        if let Some(root) = enclosing_working_tree(&resolved) {
            return Err(CredentialStoreError::InsideWorkingTree {
                path: resolved.display().to_string(),
                root: root.display().to_string(),
            });
        }

        let store =
            FileStore::open(&resolved).map_err(|source| CredentialStoreError::Unusable {
                path: resolved.display().to_string(),
                source,
            })?;

        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// The file this store is kept in.
    ///
    /// Read back off the bound store, not remembered from the configuration.
    pub fn path(&self) -> &Path {
        self.store.path()
    }

    /// The store, as the port a caller resolves credentials through.
    pub fn secrets(&self) -> Arc<dyn SecretStore> {
        self.store.clone()
    }

    /// The line a binary prints at startup, naming the store it is actually holding.
    ///
    /// The path comes from the bound store rather than from the value that was configured, so it
    /// cannot name a file this process did not open. The protection is named in the same line: an
    /// operator who reads only `credentials: /var/lib/…` will assume more than a file mode.
    pub fn banner(&self) -> String {
        format!(
            "credentials: {} (file store, mode 0600, not encrypted)",
            self.path().display()
        )
    }
}

/// Why a credential store could not be bound. Every variant refuses; none falls back.
#[derive(Debug, thiserror::Error)]
pub enum CredentialStoreError {
    /// No store was named, and there is no default worth choosing on an operator's behalf.
    #[error(
        "no credential store is configured: set `{setting}` to a path outside every working tree, \
         for example `{EXAMPLE_PATH}`. This host does not fall back to an in-memory store — one \
         would serve every route correctly and lose every credential on restart"
    )]
    Unconfigured {
        /// The setting that would have named one.
        setting: &'static str,
    },

    /// The configured path is inside a working tree.
    #[error(
        "refusing a credential store at `{path}`: it is inside the working tree at `{root}`, one \
         `git add -A` from a committed credential. Put it outside a checkout, for example \
         `{EXAMPLE_PATH}`"
    )]
    InsideWorkingTree {
        /// The store path, as resolved.
        path: String,
        /// The root of the working tree it falls under.
        root: String,
    },

    /// The path could not be made absolute — an unreadable current directory, most likely.
    #[error("refusing a credential store at `{path}`: it cannot be resolved: {reason}")]
    Unresolvable {
        /// The path as configured.
        path: String,
        /// What the filesystem said.
        reason: String,
    },

    /// The store itself refused to open at that path.
    ///
    /// Carries the underlying [`StoreError`] rather than flattening it, because an operator
    /// responds differently to each: a [`Denied`](StoreError::Denied) is a mode to look at and an
    /// [`Unreachable`](StoreError::Unreachable) is a filesystem to look at.
    #[error("the credential store at `{path}` cannot be opened: {source}")]
    Unusable {
        /// The store path, as resolved.
        path: String,
        /// The store's own refusal.
        #[source]
        source: StoreError,
    },
}

/// The root of the working tree `path` falls inside, if it falls inside one.
///
/// A working tree is recognised by its `.git`, which is a directory in a normal clone and a *file*
/// in a linked worktree or a submodule — so this asks whether the entry exists rather than whether
/// it is a directory. The innermost tree wins, because it is the one whose `git add -A` would reach
/// the store first.
///
/// This is deliberately a check of the filesystem and not of a repository's configuration: the
/// question is not "is this path tracked", which an ignore rule can answer differently tomorrow,
/// but "is this path somewhere a commit could pick it up".
fn enclosing_working_tree(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Make `path` absolute, resolving every symlink and every `..` the way the kernel will.
///
/// **Downwards, one component at a time**, because the guard above is only worth anything if the
/// path it inspects is the path `FileStore` will create. Walking *up* from the whole path and
/// canonicalising the deepest part that happens to exist is the obvious cheaper version and it is
/// wrong: `state/gone/../credentials`, where `state` is a symlink into a checkout and `gone` does
/// not exist yet, leaves the walk with a `..` it cannot take a file name from, and any answer at
/// that point that is less than fully resolved is an answer with the symlink still in it. There is
/// deliberately no arm here that returns one.
///
/// Each component is resolved against a prefix that is already resolved, so:
///
/// - a `Normal` component is canonicalised if it exists, and appended verbatim if it does not —
///   the store's own file, and usually its directory, do not exist yet;
/// - a `..` is a `pop`, which is exactly what the kernel does once the prefix it applies to has
///   been resolved, and is safe on a prefix that does not exist because a component that does not
///   exist is not a symlink.
///
/// Anything that fails for a reason other than "not there yet" is returned rather than guessed
/// past: a path this cannot resolve is a path the guard cannot vouch for, and the caller turns that
/// into a refusal.
fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };

    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            Component::RootDir | Component::Prefix(_) => resolved.push(component),
            Component::Normal(name) => {
                let candidate = resolved.join(name);
                match candidate.canonicalize() {
                    Ok(canonical) => resolved = canonical,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        resolved = candidate;
                    }
                    Err(error) => return Err(error),
                }
            }
        }
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::future::Future;
    use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};

    use connector_secrets::{CredentialRef, Secret};

    use super::*;

    /// A scratch directory under the system temporary directory, removed on drop.
    ///
    /// Hand-rolled rather than pulled in: a credential store's tests are the last place to add a
    /// dependency for four lines of `create_dir_all`.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "exchange-host-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("a scratch directory");
            // Canonicalised once, here, so every expectation below is written in the spelling the
            // store will report: the system temporary directory is itself a symlink on macOS
            // (`/var` → `/private/var`), and this module is `#[cfg(unix)]`, which includes it.
            let path = path.canonicalize().expect("a resolvable scratch directory");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            // Best effort: a test that already removed it is not a test failure.
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Drive a future to completion on this thread.
    ///
    /// `exchange-host` does not depend on `tokio`, and the store's futures never yield, so a bare
    /// executor is enough to exercise the `SecretStore` port from a unit test.
    fn block_on<F: Future>(future: F) -> F::Output {
        struct Unpark(std::thread::Thread);
        impl Wake for Unpark {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
        }

        let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    /// Create a directory the store would accept, so a test about something else is not answered
    /// by the mode check.
    fn tight_directory(path: &Path) {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .expect("a directory");
    }

    fn mode(path: &Path) -> u32 {
        fs::metadata(path)
            .expect("the path exists")
            .permissions()
            .mode()
            & 0o777
    }

    fn reference() -> CredentialRef {
        CredentialRef::new("dev-local", "com.zendesk.api", "support", "api_token")
            .expect("a valid address")
    }

    #[test]
    fn a_fresh_store_is_created_at_0600_inside_a_0700_directory() {
        let scratch = Scratch::new("fresh");
        let directory = scratch.join("state");
        let store = CredentialStore::bind(directory.join("credentials")).expect("a fresh store");

        assert_eq!(mode(store.path()), 0o600);
        assert_eq!(mode(&directory), 0o700);
    }

    #[test]
    fn a_widened_file_mode_is_refused_and_not_repaired() {
        let scratch = Scratch::new("widened-file");
        let path = scratch.join("state").join("credentials");
        let store = CredentialStore::bind(&path).expect("a fresh store");
        let path = store.path().to_path_buf();
        drop(store);

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("a widened mode");

        let refused = CredentialStore::bind(&path).expect_err("a widened mode must be refused");
        assert!(
            matches!(
                &refused,
                CredentialStoreError::Unusable {
                    source: StoreError::Denied { .. },
                    ..
                }
            ),
            "expected a refusal naming the mode, got: {refused}"
        );
        // The exposure has already happened; tightening it here would hide that it ever did.
        assert_eq!(mode(&path), 0o644);
    }

    #[test]
    fn a_widened_directory_mode_is_refused() {
        let scratch = Scratch::new("widened-directory");
        let directory = scratch.join("state");
        let store = CredentialStore::bind(directory.join("credentials")).expect("a fresh store");
        drop(store);

        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).expect("a widened mode");

        let refused = CredentialStore::bind(directory.join("credentials"))
            .expect_err("a widened directory mode must be refused");
        assert!(
            matches!(
                &refused,
                CredentialStoreError::Unusable {
                    source: StoreError::Denied { .. },
                    ..
                }
            ),
            "expected a refusal naming the directory mode, got: {refused}"
        );
        assert_eq!(mode(&directory), 0o755);
    }

    #[test]
    fn a_store_inside_a_working_tree_is_refused() {
        let scratch = Scratch::new("working-tree");
        let checkout = scratch.join("checkout");
        fs::create_dir_all(checkout.join(".git")).expect("a working tree");
        let path = checkout.join("var").join("credentials");

        let refused =
            CredentialStore::bind(&path).expect_err("a store inside a checkout must be refused");

        let CredentialStoreError::InsideWorkingTree { root, .. } = &refused else {
            panic!("expected InsideWorkingTree, got: {refused}");
        };
        assert!(
            root.ends_with("checkout"),
            "the refusal must name the working tree root, got: {root}"
        );
        // Refused before anything was created: a store that had already written the directory
        // would have left the exposure it refused.
        assert!(!path.exists());
        assert!(!path.parent().expect("a parent").exists());
    }

    #[test]
    fn a_symlink_into_a_working_tree_is_refused_too() {
        let scratch = Scratch::new("working-tree-symlink");
        let checkout = scratch.join("checkout");
        fs::create_dir_all(checkout.join(".git")).expect("a working tree");
        // At a mode the store would otherwise accept, so the only thing that can refuse this is
        // the working-tree check.
        tight_directory(&checkout.join("var"));

        let link = scratch.join("state");
        std::os::unix::fs::symlink(checkout.join("var"), &link).expect("a symlink");

        let refused = CredentialStore::bind(link.join("credentials"))
            .expect_err("a store reached through a symlink must be refused");
        assert!(
            matches!(&refused, CredentialStoreError::InsideWorkingTree { .. }),
            "expected InsideWorkingTree, got: {refused}"
        );
    }

    /// A symlink alone is caught, and a `..` alone is caught — the escape is the composition of the
    /// two, because a `..` that follows a component which does not exist yet is where a resolution
    /// that walks *up* from the whole path stops seeing the symlink underneath it.
    ///
    /// The assertion that matters is the last one: the bypass was not that the refusal was missing,
    /// it was that the store then created a `0600` credential file inside the checkout.
    #[test]
    fn a_symlink_followed_by_a_parent_hop_cannot_escape_the_check() {
        let scratch = Scratch::new("working-tree-escape");
        let checkout = scratch.join("checkout");
        fs::create_dir_all(checkout.join(".git")).expect("a working tree");
        tight_directory(&checkout.join("var"));

        let link = scratch.join("state");
        std::os::unix::fs::symlink(checkout.join("var"), &link).expect("a symlink");

        // `state` → `checkout/var`, `gone` does not exist, and `..` puts the store back in
        // `checkout/var` — which is where `FileStore` would create it.
        let escaping = link.join("gone").join("..").join("credentials");

        let refused = CredentialStore::bind(&escaping)
            .expect_err("a store that lands inside a checkout must be refused however it is spelt");
        let CredentialStoreError::InsideWorkingTree { path, root } = &refused else {
            panic!("expected InsideWorkingTree, got: {refused}");
        };
        assert!(
            root.ends_with("checkout"),
            "the refusal must name the working tree root, got: {root}"
        );
        assert!(
            path.ends_with("checkout/var/credentials"),
            "the refusal must name the path the store would have occupied, got: {path}"
        );
        assert!(
            !checkout.join("var").join("credentials").exists(),
            "a refused store must not have been created inside the checkout"
        );
    }

    /// The other half of the property above, on the path that is *not* refused: what the guard
    /// inspected is what the store went on to occupy, so the two can never disagree.
    #[test]
    fn a_path_that_hops_through_a_parent_binds_where_it_actually_lands() {
        let scratch = Scratch::new("parent-hop");
        let directory = scratch.join("state");
        tight_directory(&directory);

        let store = CredentialStore::bind(directory.join("gone").join("..").join("credentials"))
            .expect("a store");

        assert_eq!(store.path(), directory.join("credentials"));
        assert!(
            !directory.join("gone").exists(),
            "the store must be created where the path resolves, not through the spelling"
        );
    }

    #[test]
    fn nothing_configured_is_a_startup_error_naming_what_would_have_worked() {
        for configured in [None, Some(""), Some("   ")] {
            let refused = CredentialStore::bind_configured(configured)
                .expect_err("an unconfigured store must refuse");
            assert!(matches!(refused, CredentialStoreError::Unconfigured { .. }));
            let message = refused.to_string();
            assert!(message.contains(CREDENTIAL_STORE_SETTING), "{message}");
            assert!(message.contains(EXAMPLE_PATH), "{message}");
            assert!(message.contains("in-memory"), "{message}");
        }
    }

    #[test]
    fn the_banner_names_the_store_that_was_bound_and_not_the_value_configured() {
        let scratch = Scratch::new("banner");
        let real = scratch.join("state");
        tight_directory(&real);
        let link = scratch.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("a symlink");

        let store =
            CredentialStore::bind_configured(Some(&link.join("credentials").display().to_string()))
                .expect("a store");

        let banner = store.banner();
        assert!(
            banner.contains(&real.join("credentials").display().to_string()),
            "the banner must name the bound path: {banner}"
        );
        assert!(
            !banner.contains("/link/"),
            "the banner must not repeat the configured spelling: {banner}"
        );
        assert!(banner.contains("not encrypted"), "{banner}");
    }

    #[test]
    fn a_credential_survives_a_restart() {
        let scratch = Scratch::new("restart");
        let path = scratch.join("state").join("credentials");

        let store = CredentialStore::bind(&path).expect("a store");
        block_on(
            store
                .secrets()
                .put(&reference(), &Secret::new("SENTINEL-NOT-A-REAL-SECRET")),
        )
        .expect("the write lands");
        drop(store);

        let restarted = CredentialStore::bind(&path).expect("the store reopens");
        let secret =
            block_on(restarted.secrets().get(&reference())).expect("the value is still there");
        assert_eq!(secret.expose_secret(), "SENTINEL-NOT-A-REAL-SECRET");
    }

    #[test]
    fn a_deleted_credential_does_not_return_on_restart() {
        let scratch = Scratch::new("delete");
        let path = scratch.join("state").join("credentials");

        let store = CredentialStore::bind(&path).expect("a store");
        block_on(
            store
                .secrets()
                .put(&reference(), &Secret::new("SENTINEL-NOT-A-REAL-SECRET")),
        )
        .expect("the write lands");
        block_on(store.secrets().delete(&reference())).expect("the delete lands");
        drop(store);

        let restarted = CredentialStore::bind(&path).expect("the store reopens");
        let refused = block_on(restarted.secrets().get(&reference()))
            .expect_err("a revoked credential must not come back");
        assert!(refused.is_not_found(), "{refused}");
    }

    #[test]
    fn a_write_leaves_no_temporary_beside_the_store() {
        let scratch = Scratch::new("atomic");
        let directory = scratch.join("state");
        let store = CredentialStore::bind(directory.join("credentials")).expect("a store");
        block_on(
            store
                .secrets()
                .put(&reference(), &Secret::new("SENTINEL-NOT-A-REAL-SECRET")),
        )
        .expect("the write lands");

        let left: Vec<_> = fs::read_dir(&directory)
            .expect("the directory is readable")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, vec!["credentials".to_string()]);
    }

    #[test]
    fn not_found_and_unreachable_do_not_collapse() {
        let scratch = Scratch::new("distinct-failures");
        let directory = scratch.join("state");
        let store = CredentialStore::bind(directory.join("credentials")).expect("a store");

        let missing = block_on(store.secrets().get(&reference()))
            .expect_err("nothing is stored at that address");
        assert!(missing.is_not_found(), "{missing}");

        // The store is gone from under the process: a write cannot land, and that is not the same
        // event as "this tenant has not connected that integration".
        fs::remove_dir_all(&directory).expect("the store is removed");
        let gone = block_on(
            store
                .secrets()
                .put(&reference(), &Secret::new("SENTINEL-NOT-A-REAL-SECRET")),
        )
        .expect_err("the write cannot land");
        assert!(
            matches!(gone, StoreError::Unreachable { .. }),
            "a vanished store is unreachable, not not-found: {gone}"
        );
        assert!(!gone.is_not_found());
    }
}
