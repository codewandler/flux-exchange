//! Where a file store may sit, decided before anything is created.
//!
//! Two functions, both pure path logic, both moved here verbatim from [`crate::credentials`] when
//! X-47 added a second file store beside the credential one. The move is the whole reason this
//! module exists: *"is this path somewhere a commit could pick it up"* is one question, and two
//! stores answering it with two copies of the walk is how they come to answer it differently.
//!
//! Nothing here reads a store, opens a file or knows what will be written. It is asked before the
//! store is opened so that a refused path is one nothing was created at — opening first would leave
//! the directory, and the file, inside the checkout it refused.

use std::path::{Component, Path, PathBuf};

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
pub(crate) fn enclosing_working_tree(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

/// Make `path` absolute, resolving every symlink and every `..` the way the kernel will.
///
/// **Downwards, one component at a time**, because the guard above is only worth anything if the
/// path it inspects is the path the store will create. Walking *up* from the whole path and
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
pub(crate) fn resolve(path: &Path) -> std::io::Result<PathBuf> {
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
