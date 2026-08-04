//! The Service Account tokens this host has minted, and the verifiers it keeps **instead of** them.
//!
//! A Service Account is a non-human API principal, not a Flux Agent. The latter owns a model and an
//! authored loop; this type owns only a bearer identity whose authority remains bounded by grants.
//! The original X-36 through X-38 implementation called these records “agents”; the v0.16 migration
//! keeps that word only where it names the legacy disk key or a historical test.
//!
//! # This is not a session, and it must not share one's machinery
//!
//! The design doc has the table and it is the mistake it most wants to avoid:
//!
//! | | Scope | Dies when |
//! |---|---|---|
//! | a session | a conversation | closed, or the human's identity expires |
//! | a Service Account token | a principal | revoked, or its stated expiry passes |
//!
//! A session dies when the human's identity does. A Service Account token outlives every session and is
//! killed by an operator. So: a different type, a different store, and a different clock — see
//! [`ServiceAccount::expires_at`].
//!
//! # Where this store lives, and why it is not the two stores already here
//!
//! It is a **file of its own**, durable across a restart. Neither existing store was right:
//!
//! - **Not [`SessionStore`](crate::session::SessionStore).** That one is in memory because a session
//!   is short and minted for one browser, so losing it on restart costs a human one sign-in. An
//!   Service Account token is long-lived and an operator pastes it into a config; losing all of them
//!   on restart would take out every automation at once, silently, with no attributable failure
//!   to the restart. That is the "looks like it worked" failure this repository refuses elsewhere.
//! - **Not the credential store.** Two reasons, and the first is structural: [`SecretStore`] cannot
//!   enumerate. It is a `get`/`put`/`delete` port over an address, and `routes::connections` gets
//!   away with that only because the set of addresses is the compiled-in catalogue. The set of
//!   Service Accounts are not a fixed set, so *"which Service Accounts exist"* — X-38's whole story, and the thing that
//!   makes minting something other than a one-way door — would be unanswerable. The second is that
//!   a Service Account verifier is **this host's own record**, not a tenant's vendor credential: putting it
//!   in the credential file would enter it into that file's vocabulary, its addressing and its
//!   per-tenant occupancy accounting, for a value that is none of those things.
//!
//! [`SecretStore`]: exchange_host::SecretStore
//!
//! # What an attacker who reads this store obtains
//!
//! **The roster, and nothing they can present.** For each Service Account: its id, its tenant, and when its
//! token expires. What it does *not* contain, anywhere, is a value that authenticates as anybody —
//! only `SHA-256(token)`, and a digest is not a token. Reading this file end to end and replaying
//! every byte of it resolves to nothing;
//! [`an_attacker_who_reads_the_store_obtains_no_usable_token`](tests::an_attacker_who_reads_the_store_obtains_no_usable_token)
//! is that sentence as a test, and it presents every value in the file to [`ServiceAccountStore::resolve`]
//! rather than merely checking that the token is absent from it.
//!
//! ## Why a bare digest, and no password hash
//!
//! This is the question the fence on dependencies makes worth answering rather than assuming. A
//! password hash — argon2, bcrypt, PBKDF2 — buys two things, and this store needs neither:
//!
//! - **Iteration** makes brute force expensive against a *guessable* secret. A token here is 256
//!   bits drawn from [`entropy`], so there is nothing to guess; multiplying the cost of a search
//!   nobody can finish changes nothing.
//! - **A salt** stops one precomputed table from covering many secrets at once. That table would
//!   have to enumerate 256-bit random values, which is the same search again.
//!
//! So the digest is a one-way function of an unguessable input, which is exactly and completely the
//! property this store needs. Adding a password-hashing dependency would not strengthen it; it would
//! imply a threat — a low-entropy secret — that the type system here rules out.
//!
//! # Reading it is a disclosure; **writing** it is an authentication bypass
//!
//! These are not the same exposure and this module does not treat them as one. Anyone who can
//! *write* this file can insert a verifier of their own choosing and then present the matching token
//! as any Service Account in any tenant. That is a full bypass, and the file mode is the only thing standing
//! in front of it — so [`ServiceAccountStore::open`] **refuses** a store anyone but its owner can write.
//! A merely group- or world-*readable* store discloses the roster and yields no access, so that
//! warns instead: refusing to start over it would take the whole host down for a disclosure nobody
//! can spend. `exchange_host::CredentialStore` refuses both, and correctly — every byte of *that*
//! file is a live vendor credential.

use std::collections::BTreeMap;
use std::fmt;
#[cfg(test)]
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use exchange_host::{Principal, PrincipalKind, Tenant};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::entropy;

/// The setting a composing binary reads this store's path from.
///
/// The name lives here, beside the refusal that quotes it, for the reason
/// `exchange_host::CREDENTIAL_STORE_SETTING` does: a refusal and the reader that would have produced
/// the value must not drift into two spellings.
pub const SERVICE_ACCOUNT_STORE_SETTING: &str = "FLUX_EXCHANGE_SERVICE_ACCOUNTS";

/// A location that would have worked, quoted in a refusal. Written with `$HOME` rather than
/// expanded: nothing here reads the environment.
const EXAMPLE_PATH: &str = "$HOME/.local/share/flux-exchange/service_accounts.json";

/// How many bytes of entropy a Service Account token carries. 256 bits, from the OS.
const TOKEN_BYTES: usize = 32;

/// The longest life this host will mint a Service Account token for. One year.
///
/// **Refuse; never repair**, in both directions — X-16 set that precedent for sessions and the
/// argument is identical, so this is that argument and not a new one. Clamping a request for ten
/// years down to one would issue a credential neither the operator nor this host described, and the
/// operator would go on believing the ten years. Refusing names it at the moment it can still be
/// typed differently.
///
/// A year rather than a month, because this is not a session: an operator pastes a Service Account token into
/// a config and a token that expired every thirty days would be a monthly outage. A year rather than
/// forever, because a token nobody is ever forced to rotate is a token that outlives the laptop it
/// leaked from — X-39 is the rotation story, and this bound is what makes it a chore rather than an
/// afterthought.
const MAX_SERVICE_ACCOUNT_TOKEN_SECONDS: i64 = 365 * 24 * 60 * 60;

/// The most service_accounts one store will hold at once.
///
/// A bound and a refusal, following [`SessionStore`](crate::session::SessionStore) rather than
/// `oidc::flow`: eviction here would silently kill a live agent — a config somewhere stops working
/// for a reason nobody can attribute — and reaching this many service_accounts means something is minting in a
/// loop. Expired service_accounts are swept before it is tested, so it is only ever reached by service_accounts somebody
/// could still use.
const MAX_LIVE_SERVICE_ACCOUNTS: usize = 4096;

/// The longest a Service Account id may be, matching `Tenant`'s bound for the reason that one has: an
/// identifier that ends up in a log line, a metric label and a principal should be short enough that
/// it can never be the interesting part of any of them.
const MAX_SERVICE_ACCOUNT_ID: usize = 64;

/// An opaque Service Account token: this host mints it once and never holds it again.
///
/// A bearer credential, so it gets a bearer credential's protections and gets them the way
/// [`SessionToken`](crate::session::SessionToken) and
/// [`Binder`](crate::oidc::flow::Binder) already do rather than in a third shape: drawn from
/// [`entropy`], no `Display`, and a `Debug` that redacts. The value leaves this type only through
/// [`ServiceAccountToken::as_str`], which is called in exactly one place — the response that shows it once.
#[derive(Clone, PartialEq, Eq)]
pub struct ServiceAccountToken(String);

impl ServiceAccountToken {
    /// Draw one, with 256 bits of entropy from the operating system.
    ///
    /// Through [`entropy`], which is where the session token, the OIDC `state`, `nonce` and the PKCE
    /// verifier also come from. One source read one way: a second entropy path is how one of them
    /// quietly becomes weaker than the others.
    fn draw() -> Result<Self, io::Error> {
        entropy::hex::<TOKEN_BYTES>().map(|material| Self(format!("fxsa_{material}")))
    }

    /// The token as it goes on the wire, once. The only disclosure, and a deliberate one.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ServiceAccountToken {
    /// Redacts. A Service Account token in a log line is a Service Account anyone reading the log can be.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ServiceAccountToken(redacted)")
    }
}

/// What this host keeps in place of a token: `SHA-256` of it, hex encoded.
///
/// **This one deliberately does not redact**, and that is the claim rather than an oversight. A
/// value safe to print is the whole point of storing it instead of the token — see the module
/// documentation for why a bare digest is the right and complete choice here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
struct Verifier(String);

impl Verifier {
    /// The verifier for a presented value.
    ///
    /// Derived rather than stored alongside, so a token and its verifier cannot drift apart —
    /// `oidc::pkce::Verifier::challenge` derives its challenge for the same reason.
    fn of(presented: &str) -> Self {
        Self(hex(&Sha256::digest(presented.as_bytes())))
    }
}

/// Hex encoding, lower case.
///
/// [`entropy::hex`] encodes what it draws, and a digest is not something it drew, so this is the
/// same two-line table applied to bytes that came from `sha2`. Kept here rather than made public
/// there because `entropy`'s job is the source, not the alphabet.
fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[usize::from(byte >> 4)] as char);
        encoded.push(HEX[usize::from(byte & 0x0f)] as char);
    }

    encoded
}

/// When a Service Account token stops resolving.
///
/// Two fields and no default, following `session::Expiry::Credential`: `as_of` is the caller's
/// reading of the wall clock rather than one taken here, so whether the request is admissible and
/// how long the token lives are decided against the *same* instant. X-24 recorded what happens when
/// they are two readings.
///
/// Unlike a session there is no `WhileTheProcessLives` arm, and the absence is the Acceptance's
/// sixth item: a Service Account token **always** states an expiry. A session may legitimately have none —
/// the development identity's roster handles carry no `exp` to inherit, and that port is already
/// loopback-only — but nothing here is bounded by the process, so a token with no expiry would be a
/// token that is only ever killed by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expiry {
    /// When the token stops resolving, as seconds since the Unix epoch.
    pub expires_at: i64,
    /// The instant `expires_at` is judged against, as seconds since the Unix epoch.
    pub as_of: i64,
}

/// One agent this host has minted a token for.
///
/// Keyed in the store by its [`Verifier`], the way `SessionStore` is keyed by its token: it makes
/// [`ServiceAccountStore::resolve`] a lookup rather than a scan over every agent on the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceAccount {
    /// The tenant this Service Account belongs to, as the validated string it was minted from.
    ///
    /// A `String` on disk and a [`Tenant`] in every decision: `Tenant` validates at construction and
    /// has no `Deserialize`, deliberately, so a hand-edited file cannot introduce a tenant nothing
    /// checked. [`ServiceAccountStore::open`] re-validates every one it reads.
    tenant: String,

    /// Its identifier within that tenant, and the `id` of the [`Principal`] it resolves to.
    id: String,

    /// When it stops resolving, as seconds since the Unix epoch.
    ///
    /// **Wall clock, not an [`Instant`](std::time::Instant)** — the one place this store must differ
    /// from `SessionStore`, which holds a monotonic deadline precisely so that a backward clock step
    /// cannot extend a session. A deadline that has to survive a process restart cannot be
    /// monotonic: an `Instant` means nothing in the next process. So this store inherits the
    /// weakness `session` writes against — an NTP step backwards extends every Service Account token by the
    /// size of the step — and it is the cost of the durability the module documentation argues for
    /// rather than an oversight. The remedy for a token that must die now is revocation (X-38), not
    /// the clock.
    expires_at: i64,
}

impl ServiceAccount {
    /// Whether this Service Account's token is over, at the caller's reading of the clock.
    ///
    /// At the deadline and not after it: a token that ends at `t` does not resolve at `t`.
    fn has_expired(&self, as_of: i64) -> bool {
        as_of >= self.expires_at
    }
}

/// What minting produced: the token, once, and the principal it names.
#[derive(Debug)]
pub struct Minted {
    /// The token. Returned to exactly one caller, exactly once, and never held here.
    pub token: ServiceAccountToken,
    /// The Service Account principal the token resolves to.
    pub principal: Principal,
    /// When it stops resolving, as seconds since the Unix epoch.
    pub expires_at: i64,
}

/// A Service Account as the management API may list it.
///
/// No token and no verifier field can be serialized from this type; the route receives only the
/// stable name and expiry needed to choose a revocation target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServiceAccountSummary {
    /// Stable identifier within the tenant.
    pub id: String,
    /// When its bearer token stops resolving.
    pub expires_at: i64,
}

/// The file as it is written. A version, so a later shape can be read rather than guessed at.
#[derive(Serialize, Deserialize)]
struct StoredFile {
    /// The format version. `1` is the only one this build writes or reads.
    version: u32,
    /// Verifier to agent.
    #[serde(rename = "agents")]
    service_accounts: BTreeMap<Verifier, ServiceAccount>,
}

/// The service_accounts this host has minted tokens for.
///
/// Durable, because a Service Account token is. See the module documentation for why this is neither the
/// session store nor the credential store, and for what a reader of the file behind it obtains.
#[derive(Debug)]
pub struct ServiceAccountStore {
    /// The file this store is kept in, resolved.
    path: PathBuf,
    /// Verifier to agent. Written through to [`path`](Self::path) inside this lock, so no two mints
    /// can serialise two different views of the same map.
    live: Mutex<BTreeMap<Verifier, ServiceAccount>>,
}

impl ServiceAccountStore {
    /// Open — or create — the Service Account store at `path`, or refuse.
    ///
    /// **Refuse; never repair.** A file that cannot be parsed is not an empty store: reading it as
    /// one would start the host having silently revoked every agent, which is indistinguishable
    /// from a host that never had any. A store anyone but its owner can write is refused for the
    /// reason the module documentation gives — that is an authentication bypass, not a disclosure.
    ///
    /// # Errors
    ///
    /// [`ServiceAccountStoreError::Unconfigured`] for an empty path; [`ServiceAccountStoreError::Unusable`] when the
    /// file or its directory cannot be created or read; [`ServiceAccountStoreError::Unreadable`] when the
    /// file exists and is not this format; [`ServiceAccountStoreError::Writable`] when its mode would let
    /// somebody else plant a verifier.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ServiceAccountStoreError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() {
            return Err(ServiceAccountStoreError::Unconfigured {
                setting: SERVICE_ACCOUNT_STORE_SETTING,
            });
        }

        let path = absolute(path).map_err(|source| ServiceAccountStoreError::Unusable {
            path: path.display().to_string(),
            source,
        })?;

        // The directory first, at `0700` set in the `mkdir(2)` rather than `chmod`-ed afterwards —
        // a window in which the directory exists at a wider mode is a window, however short.
        if let Some(parent) = path.parent() {
            exchange_host::ensure_private_state_directory(parent).map_err(|source| {
                ServiceAccountStoreError::Unusable {
                    path: parent.display().to_string(),
                    source: io::Error::other(source),
                }
            })?;
        }

        let live =
            match exchange_host::read_private_state_file(&path, 1024 * 1024).map_err(|source| {
                ServiceAccountStoreError::Unusable {
                    path: path.display().to_string(),
                    source: io::Error::other(source),
                }
            })? {
                Some(bytes) => read(&bytes, &path)?,
                // Nothing minted yet. The file is created by the first mint with native owner-only metadata, rather than
                // here — an empty store has nothing to protect and nothing to lose.
                None => BTreeMap::new(),
            };

        Ok(Self {
            path,
            live: Mutex::new(live),
        })
    }

    /// The file this store is kept in.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The line a binary prints at startup, naming the store it is actually holding.
    ///
    /// The path is this store's own rather than the value that was configured, so it cannot name a
    /// file this process did not open — `exchange_host::CredentialStore::banner`'s rule. What the
    /// file holds is named in the same line, because an operator who reads only `service_accounts: /var/lib/…`
    /// will assume it holds tokens.
    pub fn banner(&self) -> String {
        format!(
            "service_accounts: {} (platform owner-only file store, verifiers only — no token is recoverable from it)",
            self.path().display(),
        )
    }

    /// Mint a Service Account principal for `minted_by`'s tenant, and return its token **once**.
    ///
    /// # Where the tenant comes from
    ///
    /// [`minted_by`](Principal) and nowhere else. There is deliberately no tenant parameter on this
    /// function, so there is no argument a path segment, a body field or a header could reach —
    /// the same shape `routes::identity` states for sessions, expressed as a signature rather than
    /// as a check somebody has to remember. `routes::service_accounts`' vector tests are what keep it true
    /// from the wire inwards.
    ///
    /// # Who may mint (X-40)
    ///
    /// Only a [`PrincipalKind::User`]. The argument — that a principal which can create principals
    /// makes revocation an incomplete remedy, and why `Service` is refused too — lives on
    /// `routes::service_accounts::MAY_MINT`, where a reader meets the rule.
    ///
    /// It is enforced **here as well as at the route**, and the two are not the same claim: the
    /// route's declaration is what a caller meets and what the surface enumeration can see, while
    /// this is what holds for any caller of this store — including a handler a later story adds
    /// that reaches `mint` without declaring an access. The store is the thing that creates a
    /// principal, so it is the thing that has to refuse.
    ///
    /// # Errors
    ///
    /// Every variant of [`ServiceAccountError`], and none of them carries the token — which is structural
    /// rather than disciplinary, since on any path that returns one nothing has been drawn yet.
    pub fn mint(
        &self,
        minted_by: &Principal,
        id: &str,
        expiry: Expiry,
    ) -> Result<Minted, ServiceAccountError> {
        // First, and before the identifier is even looked at: a principal that may not create one
        // learns nothing from this call — not whether the name it chose was admissible, not whether
        // it was taken, not how full this store is.
        if minted_by.kind() != PrincipalKind::User {
            return Err(ServiceAccountError::MayNotMint {
                kind: minted_by.kind(),
            });
        }

        let id = admit_id(id)?;
        // Decided before the lock, so an expiry this host will not honour refuses on its own terms
        // rather than depending on how full the store happens to be.
        let expires_at = admit_expiry(expiry)?;
        let tenant = minted_by.tenant().clone();

        let mut live = self.live();

        // Swept first, so the bound below is tested against service_accounts somebody can still use and an
        // expired one does not hold a place against it.
        live.retain(|_, agent| !agent.has_expired(expiry.as_of));

        // Refuse; never repair. Expiry bounds how long an entry lives, never how many there are.
        if live.len() >= MAX_LIVE_SERVICE_ACCOUNTS {
            return Err(ServiceAccountError::TooManyLive {
                max: MAX_LIVE_SERVICE_ACCOUNTS,
            });
        }

        // Scoped to the minting principal's tenant, so this refusal can only ever be about a Service Account
        // the caller's own tenant holds. A check across the whole store would answer a caller with
        // the fact that some other tenant uses that name.
        if live
            .values()
            .any(|agent| agent.id == id && agent.tenant == tenant.as_str())
        {
            return Err(ServiceAccountError::AlreadyMinted { id });
        }

        let token = ServiceAccountToken::draw()
            .map_err(|source| ServiceAccountError::NoEntropy { source })?;
        let verifier = Verifier::of(token.as_str());

        live.insert(
            verifier.clone(),
            ServiceAccount {
                tenant: tenant.as_str().to_string(),
                id: id.clone(),
                expires_at,
            },
        );

        // Written before the token is handed out, and rolled back if it cannot be. A token this
        // host returned but did not record is a token nobody can revoke and nobody can even see —
        // the one-way door X-38 exists to close, held open by a failed write.
        if let Err(source) = self.write(&live) {
            live.remove(&verifier);
            return Err(ServiceAccountError::Unwritable {
                path: self.path.display().to_string(),
                source,
            });
        }

        Ok(Minted {
            token,
            principal: Principal::new(PrincipalKind::ServiceAccount, id, tenant),
            expires_at,
        })
    }

    /// The Service Account principal a presented token names, if it names one at `as_of`.
    ///
    /// The request identity boundary calls this for bearer credentials, which is what makes a
    /// Service Account token authenticate. This is also the only function that can state what the
    /// stored verifier is worth:
    /// [`tests::an_attacker_who_reads_the_store_obtains_no_usable_token`] presents every value in
    /// the file to it and gets `None` from each, and a property nothing can check is a property
    /// nobody is held to.
    ///
    /// `as_of` is the caller's reading of the wall clock rather than one taken here, for
    /// [`Expiry`]'s reason.
    ///
    /// The presented value is hashed and the digest looked up, which is **not** constant time —
    /// deliberately, on the reasoning `SessionStore::resolve` records. Timing tells an attacker
    /// something only if it narrows a search, and there is nothing here to narrow: a token is 256
    /// bits from the OS, and what is compared is a digest of what they sent, so a partial match
    /// leaks a prefix of a hash they can already compute for themselves.
    ///
    /// Expired Service Accounts are filtered rather than removed, which is where this parts company with
    /// `SessionStore` — that one sweeps on resolve, because its map is free to mutate. Sweeping
    /// here would rewrite a file on every authenticated request. The bound is swept at
    /// [`mint`](Self::mint) instead, which is the only place it is enforced.
    pub fn resolve(&self, presented: &str, as_of: i64) -> Option<Principal> {
        // An empty presented value must never match, and could not: `Verifier::of("")` is the
        // digest of the empty string, which is not a value this store ever inserted. Stated anyway,
        // because "the caller sent nothing" becoming "the caller sent something that matched" is
        // the shape of mistake `oidc::flow::Binder::matches` guards the same way.
        if presented.is_empty() {
            return None;
        }

        self.live()
            .get(&Verifier::of(presented))
            .filter(|agent| !agent.has_expired(as_of))
            .and_then(|agent| {
                // A tenant that no longer validates is not a principal. It cannot arise from a file
                // this process wrote, since `open` re-validates everything it reads and `mint` only
                // ever inserts a `Tenant` — so this arm is unreachable rather than merely unlikely,
                // and refusing is the direction to be unreachable in.
                Tenant::new(agent.tenant.clone())
                    .ok()
                    .map(|tenant| Principal::new(PrincipalKind::ServiceAccount, &agent.id, tenant))
            })
    }

    /// List the caller's unexpired Service Accounts without exposing tokens or verifiers.
    pub fn list(
        &self,
        actor: &Principal,
        as_of: i64,
    ) -> Result<Vec<ServiceAccountSummary>, ServiceAccountError> {
        admit_manager(actor)?;
        let mut accounts: Vec<_> = self
            .live()
            .values()
            .filter(|account| {
                account.tenant == actor.tenant().as_str() && !account.has_expired(as_of)
            })
            .map(|account| ServiceAccountSummary {
                id: account.id.clone(),
                expires_at: account.expires_at,
            })
            .collect();
        accounts.sort_by(|left, right| left.id.cmp(&right.id));
        Ok(accounts)
    }

    /// Revoke one Service Account in the actor's tenant.
    ///
    /// Another tenant's matching id is indistinguishable from an absent one. The map is changed
    /// only after the complete replacement file has been written; a failed write leaves the token
    /// resolvable and reports that revocation did not happen.
    pub fn revoke(&self, actor: &Principal, id: &str) -> Result<(), ServiceAccountError> {
        admit_manager(actor)?;
        let id = admit_id(id)?;
        let mut live = self.live();
        let mut candidate = live.clone();
        let before = candidate.len();
        candidate
            .retain(|_, account| account.tenant != actor.tenant().as_str() || account.id != id);
        if candidate.len() == before {
            return Err(ServiceAccountError::NotFound { id });
        }
        self.write(&candidate)
            .map_err(|source| ServiceAccountError::Unwritable {
                path: self.path.display().to_string(),
                source,
            })?;
        *live = candidate;
        Ok(())
    }

    /// Write the whole store, atomically.
    ///
    /// A native owner-only sibling temporary, durable flush, then atomic replacement — the shape
    /// `connector_secrets`' file store uses, for its reason: a crash part way through leaves either
    /// the old file or the new one, never half of either. A truncate-and-write in place would leave a store that parses
    /// as fewer service_accounts than exist, which reads exactly like a revocation nobody performed.
    ///
    /// Called with the map's guard held, so the bytes on disk are always some state this map was
    /// actually in.
    fn write(&self, live: &BTreeMap<Verifier, ServiceAccount>) -> io::Result<()> {
        let encoded = serde_json::to_vec_pretty(&StoredFile {
            version: FORMAT_VERSION,
            service_accounts: live.clone(),
        })
        .map_err(io::Error::other)?;

        exchange_host::write_private_state_file(&self.path, &encoded).map_err(io::Error::other)
    }

    /// The live service_accounts.
    ///
    /// Recovers from a poisoned lock rather than propagating it, for `SessionStore::live`'s reason:
    /// the guarded value has no cross-key invariant a panic could have left half-updated, and
    /// refusing every subsequent request because an unrelated handler panicked would turn one
    /// failure into an outage.
    fn live(&self) -> std::sync::MutexGuard<'_, BTreeMap<Verifier, ServiceAccount>> {
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The only format version this build writes or reads.
const FORMAT_VERSION: u32 = 1;

/// Parse a store file, re-validating everything a hand edit could have introduced.
///
/// `Tenant` and the id are checked here rather than trusted, because the file is a thing an operator
/// can open in an editor and because `Tenant` deliberately has no `Deserialize` — validating at
/// construction is worth nothing if a deserializer can walk around it.
fn read(
    bytes: &[u8],
    path: &Path,
) -> Result<BTreeMap<Verifier, ServiceAccount>, ServiceAccountStoreError> {
    let stored: StoredFile =
        serde_json::from_slice(bytes).map_err(|source| ServiceAccountStoreError::Unreadable {
            path: path.display().to_string(),
            reason: source.to_string(),
        })?;

    if stored.version != FORMAT_VERSION {
        return Err(ServiceAccountStoreError::Unreadable {
            path: path.display().to_string(),
            reason: format!(
                "it is format version {}, and this build reads version {FORMAT_VERSION}",
                stored.version,
            ),
        });
    }

    for service_account in stored.service_accounts.values() {
        Tenant::new(service_account.tenant.clone()).map_err(|source| {
            ServiceAccountStoreError::Unreadable {
                path: path.display().to_string(),
                reason: format!(
                    "Service Account `{}` names an unusable tenant: {source}",
                    service_account.id
                ),
            }
        })?;
        admit_id(&service_account.id).map_err(|source| ServiceAccountStoreError::Unreadable {
            path: path.display().to_string(),
            reason: source.to_string(),
        })?;
    }

    Ok(stored.service_accounts)
}

/// Validate a Service Account identifier.
///
/// The same alphabet `Tenant` accepts — ASCII alphanumerics, `-` and `_` — and for the same reason,
/// stated once there: it refuses `.` and `/`, so an id can never be the interesting part of a path,
/// a log line or a metric label it is interpolated into.
fn admit_id(id: &str) -> Result<String, ServiceAccountError> {
    if id.is_empty() {
        return Err(ServiceAccountError::UnusableId {
            reason: "a Service Account identifier may not be empty",
        });
    }
    if id.len() > MAX_SERVICE_ACCOUNT_ID {
        return Err(ServiceAccountError::UnusableId {
            reason: "a Service Account identifier is at most 64 bytes",
        });
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ServiceAccountError::UnusableId {
            reason:
                "a Service Account identifier may contain only ASCII alphanumerics, `-` and `_`",
        });
    }

    Ok(id.to_string())
}

/// The instant a token minted now stops resolving, or the refusal that says why it will not be
/// minted at all.
///
/// A pure function of the [`Expiry`] it is given. Both refusals are refusals rather than repairs,
/// and both are `session::deadline`'s, restated for the store that has to make the same decision:
///
/// - **An expiry that has already passed** mints nothing. Minting a token that is dead the moment it
///   exists would report success while handing an operator a value that never works, and they would
///   go looking for the fault everywhere except at the expiry they typed.
/// - **An expiry further out than [`MAX_SERVICE_ACCOUNT_TOKEN_SECONDS`]** mints nothing either. See that
///   constant.
fn admit_expiry(expiry: Expiry) -> Result<i64, ServiceAccountError> {
    // Saturating, because `expires_at` is a number a caller sent and `i64::MIN` must arrive here as
    // a very expired token rather than as an overflow.
    let remaining = expiry.expires_at.saturating_sub(expiry.as_of);

    if remaining <= 0 {
        return Err(ServiceAccountError::AlreadyExpired {
            expires_at: expiry.expires_at,
        });
    }
    if remaining > MAX_SERVICE_ACCOUNT_TOKEN_SECONDS {
        return Err(ServiceAccountError::ImplausibleLifetime {
            seconds: remaining,
            max: MAX_SERVICE_ACCOUNT_TOKEN_SECONDS,
        });
    }

    Ok(expiry.expires_at)
}

fn admit_manager(actor: &Principal) -> Result<(), ServiceAccountError> {
    if actor.kind() == PrincipalKind::User {
        Ok(())
    } else {
        Err(ServiceAccountError::MayNotManage { kind: actor.kind() })
    }
}

/// Make `path` absolute without touching the filesystem.
///
/// Deliberately **not** `exchange_host::credentials`' full symlink resolution. That one exists to
/// decide whether a store of live vendor credentials would land inside a working tree, where one
/// `git add -A` commits it. This file holds no value anybody can present, so the same accident
/// commits a roster rather than a secret — and paying for the walk here would imply a protection the
/// contents do not need. What is wanted is only that [`ServiceAccountStore::banner`] and every refusal name
/// one stable spelling rather than whatever the process's current directory was.
fn absolute(path: &Path) -> io::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

/// Why a Service Account store could not be opened. Every variant refuses; none falls back.
///
/// Hand-written `Display`, not derived: `thiserror` is the library's convention and this binary
/// does not carry the dependency — `bind::StartupRefusal` and `session::SessionError` say the same
/// and are written the same way. The obligation the convention encodes is met below: name the path,
/// never a value, and distinguish failures an operator answers differently.
#[derive(Debug)]
pub enum ServiceAccountStoreError {
    /// No store was named, and there is no default worth choosing on an operator's behalf.
    Unconfigured {
        /// The setting that would have named one.
        setting: &'static str,
    },

    /// The store, or the directory holding it, could not be created or read.
    Unusable {
        /// The path in question.
        path: String,
        /// What the filesystem said.
        source: io::Error,
    },

    /// The file exists and is not a store this build can read.
    Unreadable {
        /// The store path.
        path: String,
        /// Why it could not be read.
        reason: String,
    },

    /// The store, or its directory, is writable by somebody other than its owner.
    Writable {
        /// The path in question.
        path: String,
        /// Whether it was the file or the directory.
        what: String,
        /// The mode found, reported rather than repaired.
        mode: u32,
    },
}

impl fmt::Display for ServiceAccountStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unconfigured { setting } => write!(
                f,
                "no Service Account store is configured: set `{setting}` to a path, for example \
                 `{EXAMPLE_PATH}`. This host does not fall back to an in-memory store — one would \
                 mint tokens that work until the next restart and then stop, with nothing to \
                 attribute the failure to",
            ),
            Self::Unusable { path, source } => {
                write!(f, "the Service Account store at `{path}` cannot be opened: {source}")
            }
            Self::Unreadable { path, reason } => write!(
                f,
                "the Service Account store at `{path}` cannot be read: {reason}. Refusing rather than \
                 starting with an empty roster, which would silently revoke every Service Account",
            ),
            Self::Writable { path, what, mode } => write!(
                f,
                "refusing the Service Account store at `{path}`: the {what} is mode {mode:04o}, so somebody \
                 other than its owner can write it — and whoever can write it can plant a verifier \
                 and authenticate as any Service Account in any tenant. `chmod 0600` on the file and `0700` \
                 on its directory",
            ),
        }
    }
}

impl std::error::Error for ServiceAccountStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unusable { source, .. } => Some(source),
            Self::Unconfigured { .. } | Self::Unreadable { .. } | Self::Writable { .. } => None,
        }
    }
}

/// Why a Service Account token could not be minted.
///
/// **No variant carries the token**, which is structural rather than a rule anybody has to keep: on
/// every path that returns one of these, either nothing has been drawn yet or the draw itself is
/// what failed. Hand-written `Display` for [`ServiceAccountStoreError`]'s reason.
#[derive(Debug)]
pub enum ServiceAccountError {
    /// The minting principal is not of a kind that may create a principal. See
    /// [`ServiceAccountStore::mint`], and `routes::service_accounts::MAY_MINT` for why.
    MayNotMint {
        /// The kind that asked. **Currently write-only**, and this doc used to claim otherwise:
        /// nothing emits it, because the published route is gated by its declaration and the guard
        /// logs the whole principal instead. It is kept because a caller reaching `mint` without a
        /// declared access — the case this variant exists for — has no other record of *what* asked,
        /// and it must never reach the answer, which quotes the rule rather than the caller.
        kind: PrincipalKind,
    },

    /// A non-human principal attempted to list or revoke Service Accounts.
    MayNotManage {
        /// The refused principal kind, for the audit-side diagnostic only.
        kind: PrincipalKind,
    },

    /// The identifier the caller supplied cannot name a Service Account.
    UnusableId {
        /// Which rule it broke. The rule and not the value: a refusal that echoed the identifier
        /// would put whatever a caller sent into this host's own log lines and answers.
        reason: &'static str,
    },

    /// This tenant already has a Service Account by that name.
    AlreadyMinted {
        /// The identifier that is taken, which is the caller's own tenant's and never another's.
        id: String,
    },

    /// No Service Account by this id exists in the actor's tenant.
    NotFound {
        /// The actor's own requested id.
        id: String,
    },

    /// The expiry has already passed, so the token would be born dead.
    AlreadyExpired {
        /// The expiry that was asked for.
        expires_at: i64,
    },

    /// The expiry is further out than this host will mint for. See [`MAX_SERVICE_ACCOUNT_TOKEN_SECONDS`].
    ImplausibleLifetime {
        /// The life that was asked for, in seconds.
        seconds: i64,
        /// The longest this host mints.
        max: i64,
    },

    /// This store already holds as many service_accounts as it will.
    TooManyLive {
        /// The limit that was reached.
        max: usize,
    },

    /// The operating system's randomness was unavailable.
    NoEntropy {
        /// What went wrong reading it.
        source: io::Error,
    },

    /// The store could not be written, so the token was not handed out.
    Unwritable {
        /// The store path.
        path: String,
        /// What the filesystem said.
        source: io::Error,
    },
}

impl fmt::Display for ServiceAccountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MayNotMint { kind } => write!(
                f,
                "a principal of kind `{kind}` may not mint a Service Account: minting creates a principal, \
                 and a principal that can create principals is one whose revocation does not end \
                 the access it gave",
            ),
            Self::MayNotManage { kind } => write!(
                f,
                "a principal of kind `{kind}` may not manage Service Accounts"
            ),
            Self::UnusableId { reason } => f.write_str(reason),
            Self::AlreadyMinted { id } => write!(
                f,
                "this tenant already holds a Service Account called `{id}`. Refusing rather than replacing \
                 it — a replacement would revoke the live token of whatever is using that name, \
                 and the first anybody would know of it is a Service Account that stopped working",
            ),
            Self::NotFound { id } => {
                write!(f, "this tenant has no Service Account called `{id}`")
            }
            Self::AlreadyExpired { expires_at } => write!(
                f,
                "cannot mint a Service Account token: the expiry {expires_at} (seconds since the Unix \
                 epoch) is in the past. Refusing rather than minting a token that never resolves",
            ),
            Self::ImplausibleLifetime { seconds, max } => write!(
                f,
                "cannot mint a Service Account token: {seconds} seconds was asked for and this host mints \
                 at most {max}. Refusing rather than quietly shortening it — a token whose life is \
                 not the life that was asked for is one nobody will rotate at the right moment",
            ),
            Self::TooManyLive { max } => write!(
                f,
                "cannot mint a Service Account token: this store already holds its maximum of {max}. \
                 Expired service_accounts are swept before this is decided, so these are all live",
            ),
            Self::NoEntropy { source } => write!(
                f,
                "cannot mint a Service Account token: {} is unreadable ({source}). Refusing rather than \
                 falling back to a predictable token",
                entropy::SOURCE,
            ),
            Self::Unwritable { path, source } => write!(
                f,
                "cannot mint a Service Account token: the store at `{path}` could not be written \
                 ({source}). Refusing rather than returning a token this host has no record of, \
                 which nobody could revoke and nobody could see",
            ),
        }
    }
}

impl std::error::Error for ServiceAccountError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoEntropy { source } | Self::Unwritable { source, .. } => Some(source),
            Self::MayNotMint { .. }
            | Self::MayNotManage { .. }
            | Self::UnusableId { .. }
            | Self::AlreadyMinted { .. }
            | Self::NotFound { .. }
            | Self::AlreadyExpired { .. }
            | Self::ImplausibleLifetime { .. }
            | Self::TooManyLive { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};

    /// A scratch directory under the system temporary directory, removed on drop.
    ///
    /// The same shape `exchange_host::credentials`' tests use, and hand-rolled for its reason: a
    /// store's tests are the last place to add a dependency for four lines of `create_dir_all`.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(label: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flux-exchange-service_accounts-{label}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir_all(&path).expect("a scratch directory");
            Self(path.canonicalize().expect("a resolvable scratch directory"))
        }

        /// The store path a test opens. Under a subdirectory, so the directory the store creates is
        /// the store's own and its mode is the store's doing rather than the scratch root's.
        fn store(&self) -> PathBuf {
            self.0.join("state").join("service_accounts.json")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn alice() -> Principal {
        Principal::new(
            PrincipalKind::User,
            "alice",
            Tenant::new("acme").expect("a literal tenant"),
        )
    }

    /// A fixed clock, so nothing here depends on when it runs.
    const NOW: i64 = 1_800_000_000;

    /// Thirty days out, as an operator wiring a Service Account into a config would state it.
    fn in_thirty_days() -> Expiry {
        Expiry {
            expires_at: NOW + 30 * 24 * 60 * 60,
            as_of: NOW,
        }
    }

    /// **X-40 at the store.** Only a `User` mints, and the refusal happens before anything else
    /// this function decides.
    ///
    /// The route's declaration is what a caller meets — `routes::service_accounts::MAY_MINT`, enforced by the
    /// guard and enumerated over the published surface. This is the same rule where it cannot be
    /// bypassed by a handler that reaches the store without declaring an access, which is the shape
    /// a later story would most plausibly reintroduce the hole in.
    ///
    /// Two things asserted beyond the refusal itself:
    ///
    /// 1. **Nothing was written.** A `403` that had already recorded the Service Account is the whole defect
    ///    wearing the right status code.
    /// 2. **It refuses before `admit_id`.** A Service Account handed an identifier this host would reject
    ///    anyway must be told it may not mint, not that its name was unusable — the second answer
    ///    is a probe into what this store would have accepted.
    #[test]
    fn only_a_user_mints_at_the_store() {
        let scratch = Scratch::new("who-may-mint");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        for kind in [PrincipalKind::ServiceAccount, PrincipalKind::Service] {
            let minter = Principal::new(
                kind,
                "incumbent",
                Tenant::new("acme").expect("a literal tenant"),
            );

            assert!(
                matches!(
                    store.mint(&minter, "successor", in_thirty_days()),
                    Err(ServiceAccountError::MayNotMint { kind: refused }) if refused == kind,
                ),
                "a `{kind}` minted a successor, so revoking its own credential would not end the \
                 access it gave",
            );

            // Leg 2: an identifier this host would refuse on its own terms still refuses for the
            // kind, so the answer is never a probe into what the store would have taken.
            assert!(
                matches!(
                    store.mint(&minter, "../../etc/passwd", in_thirty_days()),
                    Err(ServiceAccountError::MayNotMint { .. }),
                ),
                "a `{kind}` was told about its identifier rather than about its kind",
            );
        }

        // Leg 1: the refusals wrote nothing. A user's mint follows, so this is not a store that
        // simply never writes.
        assert!(!scratch.store().exists(), "a refused mint wrote the store",);

        store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("a user still mints, or every refusal above is vacuous");

        let on_disk = fs::read_to_string(scratch.store()).expect("a user's mint writes the store");
        assert!(
            on_disk.contains("triage-bot") && !on_disk.contains("successor"),
            "only the user's agent may exist: {on_disk}",
        );
    }

    /// **X-36, the Acceptance's first and fourth items.** Minting yields a token, and the value
    /// returned is not recoverable from anything this host stores.
    ///
    /// Asserted against the **store** and not against the API's shape, which is the difference
    /// between this and a test that would pass for a host keeping the token in a field it merely
    /// did not serialise. Three legs, and each rules out a way the other two pass wrongly:
    ///
    /// 1. The token is absent from the file's bytes. On its own this passes for a host that stored
    ///    the token base64-encoded, or that stored nothing at all.
    /// 2. The file genuinely records the Service Account — its id and its tenant are there — so "nothing was
    ///    stored" is ruled out.
    /// 3. **Every value in the file is presented to [`ServiceAccountStore::resolve`] and every one is
    ///    refused**, while the token itself resolves. That is the claim in the form an attacker
    ///    would test it: they have the file, they try what is in it, and none of it is a token.
    #[test]
    fn an_attacker_who_reads_the_store_obtains_no_usable_token() {
        let scratch = Scratch::new("verifier-only");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        let minted = store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("the OS has randomness and the store is writable");
        let token = minted.token.as_str().to_string();

        let on_disk = fs::read_to_string(store.path()).expect("minting writes the store");

        assert!(
            !on_disk.contains(&token),
            "the store holds the token this host handed out, so it can display it twice",
        );
        assert!(
            on_disk.contains("triage-bot") && on_disk.contains("acme"),
            "the store must actually record the Service Account, or the assertion above passes for a store \
             that recorded nothing: {on_disk}",
        );

        assert_eq!(
            store.resolve(&token, NOW),
            Some(Principal::new(
                PrincipalKind::ServiceAccount,
                "triage-bot",
                Tenant::new("acme").expect("a literal tenant"),
            )),
            "the minted token must resolve, or every other leg of this passes vacuously",
        );

        // What an attacker who read the file actually holds: every string in it, and the whole of
        // it. None of them is a token.
        let mut everything: Vec<String> = on_disk
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .filter(|word| !word.is_empty())
            .map(str::to_string)
            .collect();
        everything.push(on_disk.clone());

        assert!(
            everything.len() > 4,
            "the split must have found the file's values, or this leg proves nothing: {everything:?}",
        );
        for value in &everything {
            assert_eq!(
                store.resolve(value, NOW),
                None,
                "a value read straight out of the store authenticated as a Service Account: {value}",
            );
        }
    }

    /// **The Acceptance's second item, at the store.** The tenant is the minting principal's, and
    /// there is no argument anything a caller sent could have reached.
    ///
    /// The wire-level half — a body field, a header, a path segment — is
    /// `routes::service_accounts::tests`, which is where a caller's claim can actually be delivered.
    #[test]
    fn the_minted_principal_is_a_service_account_of_the_minting_principals_tenant() {
        let scratch = Scratch::new("tenant");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        let minted = store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("randomness");

        assert_eq!(minted.principal.kind(), PrincipalKind::ServiceAccount);
        assert_eq!(minted.principal.id(), "triage-bot");
        assert_eq!(
            minted.principal.tenant().as_str(),
            "acme",
            "the tenant must be the minting principal's",
        );
    }

    /// **The Acceptance's sixth item.** An expiry this host will not honour refuses in both
    /// directions rather than being repaired into something plausible.
    ///
    /// X-16 made this decision for sessions and the argument is identical: clamping would issue a
    /// credential neither the operator nor this host described, and the operator would keep the
    /// number they typed and believe it.
    #[test]
    fn an_expiry_this_host_will_not_honour_refuses_rather_than_being_clamped() {
        let scratch = Scratch::new("expiry");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        let expired = store.mint(
            &alice(),
            "already-dead",
            Expiry {
                expires_at: NOW - 1,
                as_of: NOW,
            },
        );
        assert!(
            matches!(expired, Err(ServiceAccountError::AlreadyExpired { .. })),
            "an expiry in the past must mint nothing at all",
        );

        // The extreme a caller reaches by writing milliseconds into a seconds field, or by meaning
        // "never".
        let forever = store.mint(
            &alice(),
            "immortal",
            Expiry {
                expires_at: i64::MAX,
                as_of: NOW,
            },
        );
        assert!(
            matches!(
                forever,
                Err(ServiceAccountError::ImplausibleLifetime { .. })
            ),
            "an expiry beyond what this host mints must refuse, not be shortened to fit",
        );

        // At the boundary exactly, which is the case a clamp would be indistinguishable from.
        assert!(store
            .mint(
                &alice(),
                "annual",
                Expiry {
                    expires_at: NOW + MAX_SERVICE_ACCOUNT_TOKEN_SECONDS,
                    as_of: NOW,
                },
            )
            .is_ok());

        assert_eq!(
            store.live().len(),
            1,
            "neither refusal may leave an entry, and the admitted one must",
        );
    }

    /// A token stops resolving at the expiry it was minted with, and not before.
    #[test]
    fn a_token_stops_resolving_when_its_stated_expiry_passes() {
        let scratch = Scratch::new("lifetime");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        let minted = store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("randomness");
        let token = minted.token.as_str();

        assert!(store.resolve(token, minted.expires_at - 1).is_some());
        assert!(
            store.resolve(token, minted.expires_at).is_none(),
            "a token that ends at `t` must not resolve at `t`",
        );
        assert!(store.resolve(token, minted.expires_at + 1).is_none());
    }

    /// **The store survives a restart**, which is the whole reason it is a file.
    ///
    /// An in-memory store would pass every other test in this module and lose every Service
    /// Account's access on the next deploy — and an operator whose automation stopped working would have nothing to
    /// attribute it to. This is that decision as an assertion.
    #[test]
    fn a_service_account_token_survives_a_restart() {
        let scratch = Scratch::new("restart");
        let path = scratch.store();

        let store = ServiceAccountStore::open(&path).expect("a fresh store");
        let minted = store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("randomness");
        let token = minted.token.as_str().to_string();
        drop(store);

        let restarted = ServiceAccountStore::open(&path).expect("the store reopens");
        assert_eq!(
            restarted.resolve(&token, NOW),
            Some(Principal::new(
                PrincipalKind::ServiceAccount,
                "triage-bot",
                Tenant::new("acme").expect("a literal tenant"),
            )),
            "a token an operator pasted into a config must not be revoked by a restart",
        );
    }

    /// A store that cannot be read is refused, not treated as empty.
    ///
    /// Reading it as empty would start the host having silently revoked every agent, which is
    /// indistinguishable from a host that never had any — the "refuse; never repair" case, in the
    /// direction where repairing looks like nothing happened at all.
    #[test]
    fn a_store_that_cannot_be_read_is_refused_rather_than_treated_as_empty() {
        let scratch = Scratch::new("unreadable");
        let path = scratch.store();
        exchange_host::ensure_private_state_directory(path.parent().expect("a parent"))
            .expect("a directory");

        for (label, contents) in [
            ("not json", "this is not a store".to_string()),
            (
                "a later format",
                serde_json::json!({ "version": 2, "service_accounts": {} }).to_string(),
            ),
            (
                "an unusable tenant",
                serde_json::json!({
                    "version": 1,
                    "service_accounts": { "0f": { "tenant": "../../etc", "id": "x", "expires_at": 0 } },
                })
                .to_string(),
            ),
            (
                "an unusable id",
                serde_json::json!({
                    "version": 1,
                    "service_accounts": { "0f": { "tenant": "acme", "id": "a/b", "expires_at": 0 } },
                })
                .to_string(),
            ),
        ] {
            exchange_host::write_private_state_file(&path, contents.as_bytes())
                .expect("a store file");
            let refused = ServiceAccountStore::open(&path)
                .expect_err("a store that cannot be read must be refused, {label}");
            assert!(
                matches!(refused, ServiceAccountStoreError::Unreadable { .. }),
                "for `{label}`, expected Unreadable, got: {refused}",
            );
            assert!(
                refused.to_string().contains("silently revoke"),
                "the refusal must say what starting anyway would have done: {refused}",
            );
        }
    }

    /// A fresh store is tight from the moment it exists.
    #[cfg(unix)]
    #[test]
    fn a_fresh_store_is_written_at_0600_inside_a_0700_directory() {
        use std::os::unix::fs::PermissionsExt as _;

        let scratch = Scratch::new("modes");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");
        store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("randomness");

        let mode = |path: &Path| {
            fs::metadata(path)
                .expect("the path exists")
                .permissions()
                .mode()
                & 0o777
        };

        assert_eq!(mode(store.path()), 0o600);
        assert_eq!(mode(store.path().parent().expect("a parent")), 0o700);
    }

    /// Every wider mode refuses without repair.
    ///
    /// The file carries verifiers and identity inventory. The portable local-state contract keeps
    /// every store behind one owner-only boundary rather than weakening one because it contains no
    /// recoverable token.
    #[cfg(unix)]
    #[test]
    fn every_wider_store_mode_is_refused_without_repair() {
        use std::os::unix::fs::PermissionsExt as _;

        let scratch = Scratch::new("exposure");
        let path = scratch.store();
        let store = ServiceAccountStore::open(&path).expect("a fresh store");
        store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("randomness");
        drop(store);

        for mode in [0o666, 0o644] {
            fs::set_permissions(&path, fs::Permissions::from_mode(mode)).expect("a widened mode");
            let refused = ServiceAccountStore::open(&path)
                .expect_err("a non-owner-only store must be refused");
            assert!(
                matches!(refused, ServiceAccountStoreError::Unusable { .. }),
                "expected Unusable, got: {refused}",
            );
            assert!(refused.to_string().contains("wider than 0600"), "{refused}");
            assert_eq!(
                fs::metadata(&path)
                    .expect("the file exists")
                    .permissions()
                    .mode()
                    & 0o777,
                mode,
                "refusal must not repair the planted mode",
            );
        }
    }

    /// Replacing a live agent by minting over its name is refused.
    ///
    /// A replacement would revoke the token of whatever is using that name, and the first anybody
    /// would know of it is a Service Account that stopped working. The refusal is scoped to the caller's own
    /// tenant, so it can never answer a caller with the fact that some other tenant uses the name.
    #[test]
    fn a_name_already_taken_in_this_tenant_refuses_and_does_not_replace() {
        let scratch = Scratch::new("collision");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        let first = store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("randomness");
        let refused = store.mint(&alice(), "triage-bot", in_thirty_days());

        assert!(matches!(
            refused,
            Err(ServiceAccountError::AlreadyMinted { .. })
        ));
        assert!(
            store.resolve(first.token.as_str(), NOW).is_some(),
            "the live token must survive the refusal",
        );

        // Another tenant may use the same name, and neither learns of the other.
        let bob = Principal::new(
            PrincipalKind::User,
            "bob",
            Tenant::new("globex").expect("a literal tenant"),
        );
        let theirs = store
            .mint(&bob, "triage-bot", in_thirty_days())
            .expect("another tenant's name is its own");
        assert_eq!(theirs.principal.tenant().as_str(), "globex");
    }

    /// An identifier that could be the interesting part of a path or a log line is refused.
    #[test]
    fn an_unusable_identifier_is_refused() {
        let scratch = Scratch::new("ids");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        for hostile in [
            "",
            "a/b",
            "../../etc",
            "a.b",
            " ",
            &"x".repeat(MAX_SERVICE_ACCOUNT_ID + 1),
        ] {
            assert!(
                matches!(
                    store.mint(&alice(), hostile, in_thirty_days()),
                    Err(ServiceAccountError::UnusableId { .. }),
                ),
                "`{hostile}` must be refused as a Service Account identifier",
            );
        }

        assert!(store
            .mint(
                &alice(),
                &"x".repeat(MAX_SERVICE_ACCOUNT_ID),
                in_thirty_days()
            )
            .is_ok());
    }

    /// Two service_accounts must never share a token, or one caller would resolve as another.
    #[test]
    fn every_minted_token_is_distinct() {
        let scratch = Scratch::new("distinct");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        let tokens: std::collections::BTreeSet<String> = (0..64)
            .map(|n| {
                store
                    .mint(&alice(), &format!("agent-{n}"), in_thirty_days())
                    .expect("randomness")
                    .token
                    .as_str()
                    .to_string()
            })
            .collect();

        assert_eq!(tokens.len(), 64, "minted tokens must not repeat");
        for token in &tokens {
            assert_eq!(token.len(), 5 + TOKEN_BYTES * 2, "fxsa_ plus 256 bits");
            assert!(token.starts_with("fxsa_"));
        }
    }

    #[test]
    fn a_legacy_unprefixed_token_still_resolves_after_reopen() {
        const LEGACY: &str = "LEGACY-SENTINEL-NOT-A-REAL-TOKEN";
        let scratch = Scratch::new("legacy-token");
        let path = scratch.store();
        let store = ServiceAccountStore::open(&path).expect("open");
        {
            let mut live = store.live();
            live.insert(
                Verifier::of(LEGACY),
                ServiceAccount {
                    tenant: "acme".to_owned(),
                    id: "legacy-runner".to_owned(),
                    expires_at: NOW + 3600,
                },
            );
            store.write(&live).expect("persist legacy-shaped record");
        }
        drop(store);

        let reopened = ServiceAccountStore::open(path).expect("reopen");
        let principal = reopened
            .resolve(LEGACY, NOW)
            .expect("legacy token resolves");
        assert_eq!(principal.kind(), PrincipalKind::ServiceAccount);
        assert_eq!(principal.id(), "legacy-runner");
        assert_eq!(principal.tenant().as_str(), "acme");
    }

    #[test]
    fn a_user_lists_and_revokes_only_its_own_service_accounts() {
        let scratch = Scratch::new("list-revoke");
        let store = ServiceAccountStore::open(scratch.store()).expect("open");
        let minted = store
            .mint(&alice(), "runner", in_thirty_days())
            .expect("mint");
        let token = minted.token.as_str().to_owned();

        assert_eq!(
            store.list(&alice(), NOW).expect("list"),
            vec![ServiceAccountSummary {
                id: "runner".to_owned(),
                expires_at: in_thirty_days().expires_at,
            }]
        );
        store.revoke(&alice(), "runner").expect("revoke");
        assert!(store.resolve(&token, NOW).is_none());
        assert!(store.list(&alice(), NOW).expect("list after").is_empty());
        assert!(matches!(
            store.revoke(&alice(), "runner"),
            Err(ServiceAccountError::NotFound { .. })
        ));
    }

    /// A bearer credential that prints itself is a bearer credential in the logs.
    ///
    /// The companion claim — that the **verifier** deliberately does not redact — is asserted in the
    /// same test, because the pair is the point: one of these is safe to print and the other is not,
    /// and a change that made them agree would be wrong whichever way it went.
    #[test]
    fn a_token_redacts_itself_and_a_verifier_does_not() {
        let scratch = Scratch::new("redaction");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");
        let minted = store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("randomness");

        let printed = format!("{:?}", minted.token);
        assert!(!printed.contains(minted.token.as_str()), "{printed}");
        assert_eq!(printed, "ServiceAccountToken(redacted)");

        // And `Minted` as a whole, since that is the value a handler holds.
        let whole = format!("{minted:?}");
        assert!(!whole.contains(minted.token.as_str()), "{whole}");

        let verifier = Verifier::of(minted.token.as_str());
        assert!(
            format!("{verifier:?}").contains(&verifier.0),
            "a verifier is safe to print, and printing it is how a store entry is identified",
        );
        assert_ne!(verifier.0, minted.token.as_str());
    }

    /// No refusal carries the token, down every way a refusal can be produced.
    ///
    /// Structural — none of these variants has a field a token could sit in — but asserted anyway,
    /// because the property that matters is about the rendered string an operator sees in a log.
    #[test]
    fn no_refusal_carries_a_token() {
        let scratch = Scratch::new("refusals");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");
        let minted = store
            .mint(&alice(), "triage-bot", in_thirty_days())
            .expect("randomness");
        let token = minted.token.as_str();

        let refusals = [
            store.mint(&alice(), "triage-bot", in_thirty_days()),
            store.mint(&alice(), "a/b", in_thirty_days()),
            store.mint(
                &alice(),
                "dead",
                Expiry {
                    expires_at: NOW - 1,
                    as_of: NOW,
                },
            ),
            store.mint(
                &alice(),
                "immortal",
                Expiry {
                    expires_at: i64::MAX,
                    as_of: NOW,
                },
            ),
        ];

        for refusal in &refusals {
            let error = refusal.as_ref().expect_err("these all refuse");
            assert!(!error.to_string().contains(token), "{error}");
            assert!(!format!("{error:?}").contains(token), "{error:?}");
        }
    }

    /// The store is bounded, and it refuses at the bound rather than evicting a live agent —
    /// which would stop a config working somewhere for a reason nobody could attribute.
    ///
    /// The second half is what keeps expiry from becoming a way round the refusal: service_accounts nobody
    /// can use must not hold a place against the bound.
    #[test]
    fn a_full_store_refuses_and_expired_service_accounts_do_not_consume_the_bound() {
        let scratch = Scratch::new("bound");
        let store = ServiceAccountStore::open(scratch.store()).expect("a fresh store");

        // Inserted directly. Minting 4096 times would write the file 4096 times, and what is under
        // test is the bound rather than the writing — which its own tests cover.
        {
            let mut live = store.live();
            for n in 0..MAX_LIVE_SERVICE_ACCOUNTS {
                live.insert(
                    Verifier(format!("{n:064x}")),
                    ServiceAccount {
                        tenant: "acme".to_string(),
                        id: format!("agent-{n}"),
                        expires_at: NOW + 60,
                    },
                );
            }
        }

        assert!(
            matches!(
                store.mint(&alice(), "one-more", in_thirty_days()),
                Err(ServiceAccountError::TooManyLive { .. }),
            ),
            "a full store must refuse",
        );

        // The same store, read a minute later: every entry has expired, so none of them holds a
        // place any longer.
        assert!(
            store
                .mint(
                    &alice(),
                    "one-more",
                    Expiry {
                        expires_at: NOW + 3600,
                        as_of: NOW + 61,
                    },
                )
                .is_ok(),
            "a store full of expired service_accounts must admit an honest caller",
        );
        assert_eq!(
            store.live().len(),
            1,
            "and the expired service_accounts must be gone rather than merely unresolvable",
        );
    }
}
