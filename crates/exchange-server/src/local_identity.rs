//! Verified local users loaded from an owner-only file.
//!
//! This is intentionally not [`crate::dev_identity::DevIdentity`]. A development handle is a
//! guessable name and therefore loopback-only; a local user proves possession of a generated
//! 256-bit secret and may sit behind a reachable bind.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use exchange_host::{
    async_trait, Identity, IdentityError, Principal, PrincipalKind, Tenant, TenantError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::entropy;
use crate::session::{Expiry, SessionError, SessionStore, SessionToken};

/// The file whose entries bind generated local-user verifiers to principals.
pub const LOCAL_USERS_SETTING: &str = "FLUX_EXCHANGE_LOCAL_USERS";

const SECRET_BYTES: usize = 32;
const SECRET_PREFIX: &str = "fxlu_";
const VERIFIER_HEX_LENGTH: usize = 64;

/// One verifier-bearing file entry. It can be serialized for the generator, but never carries the
/// generated secret.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalUserEntry {
    user: String,
    tenant: String,
    verifier: String,
}

/// A generated local-user credential. No `Debug` or `Display`: adding either would make logging
/// the credential a one-character formatter choice. The command that creates it calls the one
/// explicit disclosure method and then lets it drop.
pub struct LocalUserSecret(String);

impl LocalUserSecret {
    /// The single intentional disclosure, used by the generator's one-time output.
    pub fn expose_once(&self) -> &str {
        &self.0
    }
}

/// Generate a bearer-quality local-user secret and the verifier-only entry stored for it.
pub fn generate(
    user: &str,
    tenant: &str,
) -> Result<(LocalUserSecret, LocalUserEntry), LocalUserRefusal> {
    validate_user(user, 1)?;
    let tenant = Tenant::new(tenant)
        .map_err(|source| LocalUserRefusal::UnusableTenant { entry: 1, source })?;
    let secret = LocalUserSecret(format!(
        "{SECRET_PREFIX}{}",
        entropy::hex::<SECRET_BYTES>()?
    ));
    let entry = LocalUserEntry {
        user: user.to_owned(),
        tenant: tenant.as_str().to_owned(),
        verifier: digest(secret.expose_once()),
    };
    Ok((secret, entry))
}

struct LocalUser {
    principal: Principal,
    verifier: String,
}

/// A distinct, verifier-backed identity binding for self-hosted human users.
pub struct LocalUsers {
    users: BTreeMap<String, LocalUser>,
    sessions: SessionStore,
}

impl LocalUsers {
    /// Open and validate the configured users file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, LocalUserRefusal> {
        let path = path.as_ref();
        admit_mode(path)?;
        let raw = fs::read_to_string(path).map_err(|source| LocalUserRefusal::Unreadable {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_json(&raw)
    }

    /// Parse verifier entries. Kept separate so malformed-entry behavior is testable without
    /// mutating process environment or relying on filesystem failure order.
    pub fn from_json(raw: &str) -> Result<Self, LocalUserRefusal> {
        let values: Vec<serde_json::Value> = serde_json::from_str(raw)
            .map_err(|source| LocalUserRefusal::MalformedDocument { source })?;
        if values.is_empty() {
            return Err(LocalUserRefusal::Empty);
        }

        let mut users = BTreeMap::new();
        for (offset, value) in values.into_iter().enumerate() {
            let entry_number = offset + 1;
            let entry: LocalUserEntry = serde_json::from_value(value).map_err(|source| {
                LocalUserRefusal::MalformedEntry {
                    entry: entry_number,
                    reason: source.to_string(),
                }
            })?;
            validate_user(&entry.user, entry_number)?;
            validate_verifier(&entry.verifier, entry_number)?;
            let tenant =
                Tenant::new(&entry.tenant).map_err(|source| LocalUserRefusal::UnusableTenant {
                    entry: entry_number,
                    source,
                })?;
            let user = LocalUser {
                principal: Principal::new(PrincipalKind::User, &entry.user, tenant),
                verifier: entry.verifier,
            };
            if users.insert(entry.user.clone(), user).is_some() {
                return Err(LocalUserRefusal::DuplicateUser { user: entry.user });
            }
        }

        Ok(Self {
            users,
            sessions: SessionStore::new(),
        })
    }

    /// Number of configured principals, safe for a startup summary that names nobody.
    pub fn len(&self) -> usize {
        self.users.len()
    }

    /// Verify a form credential. Unknown users and wrong secrets take the same digest/compare path
    /// and produce the same typed refusal.
    pub fn authenticate(&self, user: &str, secret: &str) -> Result<Principal, IdentityError> {
        const ABSENT_USER_VERIFIER: &str =
            "0000000000000000000000000000000000000000000000000000000000000000";

        // Hash before looking up the user and always compare against a correctly shaped verifier.
        // The outside answer is one `Rejected` in both cases, and an absent user does not get a
        // cheap path that can be distinguished from a wrong secret by request duration.
        let presented = digest(secret);
        let stored = self
            .users
            .get(user)
            .map_or(ABSENT_USER_VERIFIER, |entry| entry.verifier.as_str());
        let matches = constant_time_eq(presented.as_bytes(), stored.as_bytes());

        match (matches, self.users.get(user)) {
            (true, Some(entry)) => Ok(entry.principal.clone()),
            _ => Err(IdentityError::Rejected),
        }
    }

    /// Open a browser session only after [`authenticate`](Self::authenticate) returned a principal.
    pub fn open_session(&self, principal: Principal) -> Result<SessionToken, SessionError> {
        self.sessions.open(principal, Expiry::WhileTheProcessLives)
    }

    /// Close whatever session the caller proved it holds.
    pub fn close_session(&self, presented: &str) {
        self.sessions.close(presented);
    }
}

#[async_trait]
impl Identity for LocalUsers {
    async fn resolve(&self, presented: &str) -> Result<Option<Principal>, IdentityError> {
        if presented.is_empty() {
            return Ok(None);
        }
        self.sessions
            .resolve(presented)
            .map(Some)
            .ok_or(IdentityError::Rejected)
    }
}

fn validate_user(user: &str, entry: usize) -> Result<(), LocalUserRefusal> {
    if user.is_empty()
        || user.len() > 128
        || !user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'@'))
    {
        return Err(LocalUserRefusal::UnusableUser { entry });
    }
    Ok(())
}

fn validate_verifier(verifier: &str, entry: usize) -> Result<(), LocalUserRefusal> {
    if verifier.len() != VERIFIER_HEX_LENGTH
        || !verifier
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(LocalUserRefusal::UnusableVerifier { entry });
    }
    Ok(())
}

fn digest(secret: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = Sha256::digest(secret.as_bytes());
    let mut encoded = String::with_capacity(VERIFIER_HEX_LENGTH);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (&left, &right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

#[cfg(unix)]
fn admit_mode(path: &Path) -> Result<(), LocalUserRefusal> {
    use std::os::unix::fs::PermissionsExt as _;

    let metadata = fs::metadata(path).map_err(|source| LocalUserRefusal::Unreadable {
        path: path.display().to_string(),
        source,
    })?;
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(LocalUserRefusal::WideMode {
            path: path.display().to_string(),
            mode,
        });
    }
    Ok(())
}

#[cfg(not(unix))]
fn admit_mode(_path: &Path) -> Result<(), LocalUserRefusal> {
    Ok(())
}

/// Why a static local-user binding would not arm. No variant carries a secret or verifier.
#[derive(Debug)]
pub enum LocalUserRefusal {
    Unreadable { path: String, source: io::Error },
    WideMode { path: String, mode: u32 },
    MalformedDocument { source: serde_json::Error },
    Empty,
    MalformedEntry { entry: usize, reason: String },
    UnusableUser { entry: usize },
    UnusableTenant { entry: usize, source: TenantError },
    UnusableVerifier { entry: usize },
    DuplicateUser { user: String },
}

impl From<io::Error> for LocalUserRefusal {
    fn from(source: io::Error) -> Self {
        Self::Unreadable {
            path: entropy::SOURCE.to_owned(),
            source,
        }
    }
}

impl fmt::Display for LocalUserRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, source } => write!(f, "cannot read local users file {path}: {source}"),
            Self::WideMode { path, mode } => write!(
                f,
                "refusing local users file {path}: mode {mode:04o} is wider than 0600; make it owner-only"
            ),
            Self::MalformedDocument { source } => write!(f, "local users document is malformed: {source}"),
            Self::Empty => write!(f, "local users document names no users"),
            Self::MalformedEntry { entry, reason } => write!(f, "local users entry {entry} is malformed: {reason}"),
            Self::UnusableUser { entry } => write!(f, "local users entry {entry} names an unusable user"),
            Self::UnusableTenant { entry, source } => write!(f, "local users entry {entry} names an unusable tenant: {source}"),
            Self::UnusableVerifier { entry } => write!(f, "local users entry {entry} has an unusable verifier; expected 64 lowercase hexadecimal characters"),
            Self::DuplicateUser { user } => write!(f, "local users file names user {user:?} twice"),
        }
    }
}

impl std::error::Error for LocalUserRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured() -> LocalUsers {
        LocalUsers::from_json(&format!(
            r#"[{{"user":"alice","tenant":"acme","verifier":"{}"}}]"#,
            digest("correct horse")
        ))
        .expect("valid users")
    }

    #[test]
    fn a_secret_authenticates_to_the_file_principal_and_tenant() {
        let principal = configured()
            .authenticate("alice", "correct horse")
            .expect("the generated secret matches");
        assert_eq!(principal.kind(), PrincipalKind::User);
        assert_eq!(principal.id(), "alice");
        assert_eq!(principal.tenant().as_str(), "acme");
    }

    #[test]
    fn unknown_user_and_wrong_secret_are_the_same_refusal() {
        let users = configured();
        let wrong = users.authenticate("alice", "wrong");
        let absent = users.authenticate("nobody", "wrong");
        assert!(matches!(wrong, Err(IdentityError::Rejected)));
        assert!(matches!(absent, Err(IdentityError::Rejected)));
    }

    #[test]
    fn malformed_input_names_the_entry_without_echoing_its_verifier() {
        let marker = "do-not-echo-this";
        let refusal = match LocalUsers::from_json(&format!(
            r#"[{{"user":"alice","tenant":"acme","verifier":"{marker}"}}]"#
        )) {
            Err(refusal) => refusal,
            Ok(_) => panic!("bad verifier was accepted"),
        };
        let message = refusal.to_string();
        assert!(message.contains("entry 1"), "{message}");
        assert!(!message.contains(marker), "{message}");
    }

    #[test]
    fn generated_output_stores_a_verifier_and_not_the_secret() {
        let (secret, entry) = generate("alice", "acme").expect("OS entropy");
        let stored = serde_json::to_string(&vec![entry]).expect("serializable entry");
        assert!(!stored.contains(secret.expose_once()), "{stored}");
        assert!(!stored.contains("secret"), "{stored}");
        assert!(stored.contains("verifier"), "{stored}");
    }

    #[cfg(unix)]
    #[test]
    fn a_wide_users_file_is_refused_and_not_repaired() {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};

        let root = std::env::temp_dir().join(format!(
            "flux-exchange-local-users-{}",
            entropy::hex::<8>().expect("OS entropy")
        ));
        fs::create_dir(&root).expect("scratch directory");
        let path = root.join("users.json");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .and_then(|_| fs::set_permissions(&path, fs::Permissions::from_mode(0o644)))
            .expect("wide users file");

        let refusal = match LocalUsers::open(&path) {
            Err(refusal) => refusal,
            Ok(_) => panic!("wide file was accepted"),
        };
        assert!(matches!(
            refusal,
            LocalUserRefusal::WideMode { mode: 0o644, .. }
        ));
        assert_eq!(
            fs::metadata(&path).expect("metadata").permissions().mode() & 0o777,
            0o644,
            "refuse; never repair"
        );
        fs::remove_dir_all(root).expect("remove scratch directory");
    }
}
