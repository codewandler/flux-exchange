//! **Which connector catalogue this deployment serves** (X-153).
//!
//! Decision 0022 makes the catalogue a *data* dependency rather than a code one: the compile
//! destination is one versioned, digest-carrying pack, and a host that wants a catalogue newer than
//! the binary it was built with loads one from a path. That is the capability this module exists
//! for — **a new provider stops requiring an Exchange release** — and the reason it is worth a
//! module rather than a function is the seam, not the loading.
//!
//! # The seam
//!
//! [`ServedCatalogue`] is *the* answer to "which catalogue is answering". A composition builds
//! exactly one, at startup, and every surface that reports the catalogue or resolves through it
//! reads that one value. The alternative — each call site choosing between "the embedded pack" and
//! "the loaded pack" — is how two surfaces come to answer the same question differently, and the
//! operator debugging a missing operation is the person who finds out.
//!
//! One consequence is deliberate and worth stating where it will be read: [`CatalogueReport`] is
//! the single wire projection, so `GET /api/catalogue/connectors` and the onboarding descriptor do
//! not each render the catalogue's identity their own way. They serialise the same value.
//!
//! # Refuse; never repair
//!
//! Every failure here is a refusal *before a single record is served*, and none of them falls back
//! to the embedded catalogue. The fallback is the failure mode with no symptom: the deployment
//! starts, every request succeeds, and the answers come from a catalogue nobody configured. So
//! [`ServedCatalogue::load`] returns a `Result` with no repairing arm, and
//! [`CatalogueRefusal::check`] names *which* verification failed so an operator reads one remedy
//! rather than four possibilities.
//!
//! # What this is not
//!
//! Not an authentication boundary. The digest catches truncation, corruption and a hand-edit; it
//! does not catch an author who can rewrite both the payload and the digest line above it. A pack
//! is trusted exactly as far as the filesystem it was read from — the same trust every other store
//! this host binds already has, and stated here because a digest invites the stronger reading.
//!
//! Not a second catalogue API either. The typed `connector_catalog` surface is unchanged and is
//! still what the invoke path resolves through: those `&'static` tables carry `Operation::flux`,
//! the emitted Flux text the canonical documents replaced with a request template, and
//! `connector-pack` still parses it. They reduce to the pack upstream when the emitter is retired
//! (C-540). Until then a loaded pack is served by the surfaces below and **not** by the execution
//! path — see `docs/designs/catalog-artifact.md` and this story's report for exactly which surfaces
//! answer from which catalogue, because that split is a fact about this deployment rather than a
//! detail.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// The canonical-document schema version this build serves.
///
/// Read off the reader rather than restated, so a reader upgrade cannot leave this crate claiming
/// to serve a schema it no longer does.
pub const SUPPORTED_CATALOGUE_SCHEMA: u32 = connector_catalog_reader::SUPPORTED_SCHEMA;

/// The pack container format version this build implements. Read off the reader, for the reason
/// above.
pub const SUPPORTED_CATALOGUE_FORMAT: u32 = connector_catalog_reader::FORMAT_VERSION;

/// Where the catalogue being served came from.
///
/// Two values and no third: a deployment either serves what it was built with or serves a pack an
/// operator pointed it at. There is deliberately no "loaded, then fell back" — that state is what
/// [`ServedCatalogue::load`]'s `Result` exists to make unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CatalogueOrigin {
    /// The pack compiled into this binary.
    Embedded,
    /// A pack read from a path this deployment's configuration named.
    Loaded,
}

/// The catalogue's identity, as every surface reports it.
///
/// **One projection, serialised by both surfaces**, which is the whole point of it being a type
/// rather than four fields assembled per route. Two renderings of the same fact drift, and the
/// drift is invisible until somebody compares two pages.
///
/// The configured **path is deliberately absent**. Both surfaces that publish this are reachable
/// anonymously, and a filesystem path describes where a deployment keeps its files — which is
/// deployment layout rather than catalogue identity, and answers no question an operator asked. The
/// digest identifies the catalogue; the path is named in the startup refusal, where the reader is
/// the operator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogueReport {
    /// Embedded, or loaded from a path.
    pub source: CatalogueOrigin,
    /// The verified content digest, lowercase hex SHA-256 — the catalogue's identity.
    pub digest: String,
    /// The document schema version the pack carries.
    pub schema_version: u32,
    /// How many providers it serves.
    pub providers: usize,
    /// How many operations it serves.
    pub operations: usize,
}

/// The pack this host serves, and where it came from.
///
/// Constructed once per process by a composition — [`ServedCatalogue::embedded`] when nothing is
/// configured, [`ServedCatalogue::load`] when a path is — and then only read. There is no
/// constructor that takes caller input and no way to swap the pack afterwards, which is what makes
/// "the configured path is startup configuration, never derived from a request" a property of the
/// type rather than a habit of its callers.
pub struct ServedCatalogue {
    held: Held,
}

/// The pack, borrowed from the binary or owned from a file.
///
/// A private enum rather than two public types: every accessor below answers the same way for both,
/// and a caller that had to match on which one it held would be a caller making the per-call-site
/// choice this module exists to remove.
enum Held {
    /// The compiled-in pack, parsed once for the whole process by the reader.
    Embedded(&'static connector_catalog_reader::Pack),
    /// A pack read from `path` and verified before it was accepted.
    Loaded {
        /// What the deployment configured, kept for the operator-facing refusal and for logs.
        path: PathBuf,
        /// The verified pack.
        pack: connector_catalog_reader::Pack,
    },
}

impl std::fmt::Debug for ServedCatalogue {
    /// The identity and the shape, never the payload — the reader's own `Debug` rule, kept here
    /// because a `{:?}` of this in a startup log must not print the catalogue at an operator.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServedCatalogue")
            .field("source", &self.origin())
            .field("path", &self.path())
            .field("digest", &self.digest())
            .field("schema_version", &self.schema_version())
            .field("providers", &self.provider_count())
            .field("operations", &self.operation_count())
            .finish()
    }
}

impl Default for ServedCatalogue {
    /// The embedded pack: what a deployment that configures nothing serves, which is what every
    /// deployment served before this story.
    fn default() -> Self {
        Self::embedded()
    }
}

impl ServedCatalogue {
    /// The catalogue compiled into this binary.
    pub fn embedded() -> Self {
        Self {
            held: Held::Embedded(connector_catalog_reader::embedded()),
        }
    }

    /// Read, verify and serve the pack at `path`.
    ///
    /// The container format, the digest and the document schema are all checked **before a single
    /// record is served**, and each failure is its own refusal — see [`CatalogueRefusal::check`].
    ///
    /// # Errors
    ///
    /// [`CatalogueRefusal`], always. There is no arm that returns the embedded catalogue: a pack an
    /// operator configured and this host could not verify is a refusal to start, not a reason to
    /// answer from somewhere else.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, CatalogueRefusal> {
        let path = path.as_ref();
        let pack = connector_catalog_reader::Pack::load(path)
            .map_err(|error| CatalogueRefusal::of(path, &error))?;

        Ok(Self {
            held: Held::Loaded {
                path: path.to_path_buf(),
                pack,
            },
        })
    }

    /// The catalogue this deployment serves, from what its configuration said.
    ///
    /// `None` — the setting unset, or set to nothing at all — is the embedded catalogue, because a
    /// deployment that named no pack asked for no change. A setting present but blank is treated
    /// the same way rather than as the path `""`: an empty environment variable is how a shell
    /// spells "unset" often enough that reading it as a filename would refuse startup with a
    /// baffling message.
    ///
    /// # Errors
    ///
    /// Everything [`load`](Self::load) refuses.
    pub fn configured(setting: Option<&str>) -> Result<Self, CatalogueRefusal> {
        match setting.map(str::trim).filter(|value| !value.is_empty()) {
            Some(path) => Self::load(path),
            None => Ok(Self::embedded()),
        }
    }

    /// The verified pack, whichever it is.
    fn pack(&self) -> &connector_catalog_reader::Pack {
        match &self.held {
            Held::Embedded(pack) => pack,
            Held::Loaded { pack, .. } => pack,
        }
    }

    /// Embedded, or loaded from a path.
    pub fn origin(&self) -> CatalogueOrigin {
        match &self.held {
            Held::Embedded(_) => CatalogueOrigin::Embedded,
            Held::Loaded { .. } => CatalogueOrigin::Loaded,
        }
    }

    /// The path this catalogue was loaded from, if it was loaded from one.
    ///
    /// For operator-facing output — a startup line, a refusal — and deliberately not part of
    /// [`report`](Self::report); see that type for why.
    pub fn path(&self) -> Option<&Path> {
        match &self.held {
            Held::Embedded(_) => None,
            Held::Loaded { path, .. } => Some(path),
        }
    }

    /// The verified content digest, lowercase hex — this catalogue's identity.
    pub fn digest(&self) -> &str {
        self.pack().digest()
    }

    /// The document schema version the served pack carries.
    pub fn schema_version(&self) -> u32 {
        self.pack().schema_version()
    }

    /// How many providers it serves.
    pub fn provider_count(&self) -> usize {
        self.pack().providers().len()
    }

    /// How many operations it serves.
    pub fn operation_count(&self) -> usize {
        self.pack().operations().len()
    }

    /// Every provider id it serves, in id order.
    pub fn provider_ids(&self) -> Vec<&str> {
        self.pack()
            .providers()
            .map(|provider| provider.id())
            .collect()
    }

    /// Every operation id it serves, in id order.
    pub fn operation_ids(&self) -> Vec<&str> {
        self.pack()
            .operations()
            .map(|operation| operation.id())
            .collect()
    }

    /// One provider's canonical document, as JSON text, or `None` if this catalogue has no such
    /// provider.
    ///
    /// **This is the accessor that makes the seam load-bearing rather than decorative.** X-154
    /// recorded the fact the generated `&'static` tables do not carry: `OAuth2::endpoint` names a
    /// service, and only the *default* service has a `base_url` in those tables, so GitLab's `login`
    /// endpoint base is refused by name rather than guessed. The document carries it under
    /// `services[].base_url`, and resolving it *through the catalogue being served* — rather than
    /// through whichever catalogue a call site reached for — is X-154 round 2.
    ///
    /// Text rather than a parsed model on purpose: the pack's contract is that a record is canonical
    /// JSON and interpreting it is the consumer's job, and a model here would be this crate's
    /// opinion about a document upstream owns.
    pub fn provider_document(&self, id: &str) -> Option<&str> {
        self.pack().provider(id).map(|provider| provider.document())
    }

    /// One operation's own JSON record, sliced out of its provider's document.
    pub fn operation_record(&self, id: &str) -> Option<&str> {
        self.pack()
            .operation(id)
            .map(|operation| operation.record())
    }

    /// This catalogue's identity, as every surface publishes it.
    pub fn report(&self) -> CatalogueReport {
        CatalogueReport {
            source: self.origin(),
            digest: self.digest().to_owned(),
            schema_version: self.schema_version(),
            providers: self.provider_count(),
            operations: self.operation_count(),
        }
    }
}

/// Why a configured catalogue pack is not one this host will serve.
///
/// Every variant refuses; none repairs, and none of them is a reason to serve the embedded
/// catalogue instead. The variants exist separately because an operator does different things about
/// them — retype a path, re-fetch a file, upgrade this binary — and a single "the pack is bad" would
/// send two of those three readers to the wrong place.
///
/// The path is named. A path is an address rather than a value, and the operator who configured it
/// is the one reading the refusal.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CatalogueRefusal {
    /// The file could not be read at all — absent, or not readable by this process.
    #[error(
        "refusing to start: the configured connector catalogue pack `{path}` could not be read \
         ({reason}). Point the setting at a readable `catalog.pack`, or unset it to serve the \
         catalogue this build embeds"
    )]
    Unreadable {
        /// The configured path.
        path: String,
        /// The operating system's reason.
        reason: String,
    },

    /// The file is not a catalogue pack.
    #[error(
        "refusing to start: `{path}` is not a connector catalogue pack. A pack is the file a \
         flux-connectors release publishes as `catalog.pack`, beside its `catalog.pack.sha256`"
    )]
    NotAPack {
        /// The configured path.
        path: String,
    },

    /// The pack is structurally incomplete — a header line missing, a span outside the payload, a
    /// file that ends early.
    #[error(
        "refusing to start: the connector catalogue pack `{path}` is incomplete ({reason}). It is \
         most likely a partial download; re-fetch it and check it against the release's \
         `catalog.pack.sha256`"
    )]
    Incomplete {
        /// The configured path.
        path: String,
        /// What the reader found missing or misaligned.
        reason: String,
    },

    /// The bytes do not hash to the digest the pack states for them.
    #[error(
        "refusing to start: the connector catalogue pack `{path}` states digest {stated} and its \
         bytes hash to {computed}, so it is truncated, corrupted or edited. No record is served \
         from bytes that disagree with their own header"
    )]
    DigestMismatch {
        /// The configured path.
        path: String,
        /// The digest the pack's header states.
        stated: String,
        /// The digest its bytes actually have.
        computed: String,
    },

    /// The documents inside carry a schema version this build does not serve.
    #[error(
        "refusing to start: the connector catalogue pack `{path}` carries document schema \
         {found} and this build serves schema {supported}. Fail closed rather than hand out \
         records it cannot vouch for — run a build that serves schema {found}, or point the \
         setting at a pack at schema {supported}"
    )]
    UnsupportedSchema {
        /// The configured path.
        path: String,
        /// The schema version the pack carries.
        found: u32,
        /// The schema version this build serves.
        supported: u32,
    },

    /// The container itself is a format version this build's reader does not implement.
    ///
    /// Deliberately distinct from [`UnsupportedSchema`](Self::UnsupportedSchema): that one means
    /// the file parsed and this build will not vouch for what is inside; this one means it could
    /// not be parsed at all. The remedy is the same upgrade and the report upstream is not.
    #[error(
        "refusing to start: the connector catalogue pack `{path}` declares container format \
         {found} and this build implements format {supported}; a newer pack needs a newer binary"
    )]
    UnsupportedFormat {
        /// The configured path.
        path: String,
        /// The container format the pack declares.
        found: u32,
        /// The container format this build implements.
        supported: u32,
    },

    /// The reader refused for a reason this build has no variant for.
    ///
    /// The reader's error type is `#[non_exhaustive]`, so a newer one may refuse in a way this
    /// mapping has never seen. Filing that under the nearest existing variant would tell an
    /// operator a confident and wrong thing; this says what happened and still refuses. *Refuse;
    /// never repair* applies to the mapping as much as to the load.
    #[error(
        "refusing to start: the connector catalogue pack `{path}` was refused by the reader \
         ({reason}). This build has no more specific answer for that refusal"
    )]
    Unrecognised {
        /// The configured path.
        path: String,
        /// The reader's own rendering.
        reason: String,
    },
}

impl CatalogueRefusal {
    /// The reader's refusal, as this host's.
    fn of(path: &Path, error: &connector_catalog_reader::Error) -> Self {
        use connector_catalog_reader::Error;

        let at = path.display().to_string();
        match error {
            Error::Io(reason) => Self::Unreadable {
                path: at,
                // The reader's `Io` message already carries the path; the refusal above names it
                // too, so the operating system's half is taken on its own.
                reason: reason
                    .split_once(": ")
                    .map(|(_, reason)| reason.to_owned())
                    .unwrap_or_else(|| reason.clone()),
            },
            // Not UTF-8 is not "corrupt text" — a pack is a text container, so a file that is not
            // text was never one.
            Error::NotAPack | Error::NotText => Self::NotAPack { path: at },
            Error::Malformed(reason) => Self::Incomplete {
                path: at,
                reason: reason.clone(),
            },
            Error::DigestMismatch { stated, computed } => Self::DigestMismatch {
                path: at,
                stated: stated.clone(),
                computed: computed.clone(),
            },
            Error::UnsupportedSchema { found } => Self::UnsupportedSchema {
                path: at,
                found: *found,
                supported: SUPPORTED_CATALOGUE_SCHEMA,
            },
            Error::UnsupportedFormat { found } => Self::UnsupportedFormat {
                path: at,
                found: *found,
                supported: SUPPORTED_CATALOGUE_FORMAT,
            },
            other => Self::Unrecognised {
                path: at,
                reason: other.to_string(),
            },
        }
    }

    /// **Which verification failed**, as one stable word.
    ///
    /// The refusal message is for a person; this is for everything that has to tell two refusals
    /// apart without matching a sentence — a test, a log field, an operator scanning four
    /// possibilities for the one that happened. Four fixtures, four words, and
    /// `tests/served_catalogue.rs` holds them to being four rather than one repeated.
    pub fn check(&self) -> &'static str {
        match self {
            Self::Unreadable { .. } => "readable",
            Self::NotAPack { .. } | Self::UnsupportedFormat { .. } => "container-format",
            Self::Incomplete { .. } => "structure",
            Self::DigestMismatch { .. } => "digest",
            Self::UnsupportedSchema { .. } => "schema-version",
            Self::Unrecognised { .. } => "unrecognised",
        }
    }

    /// The path the deployment configured.
    pub fn path(&self) -> &str {
        match self {
            Self::Unreadable { path, .. }
            | Self::NotAPack { path }
            | Self::Incomplete { path, .. }
            | Self::DigestMismatch { path, .. }
            | Self::UnsupportedSchema { path, .. }
            | Self::UnsupportedFormat { path, .. }
            | Self::Unrecognised { path, .. } => path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default is the embedded pack, so a composition that binds nothing keeps serving what it
    /// served before this story.
    #[test]
    fn the_default_catalogue_is_the_embedded_one() {
        let served = ServedCatalogue::default();

        assert_eq!(served.origin(), CatalogueOrigin::Embedded);
        assert_eq!(served.path(), None);
        assert_eq!(served.schema_version(), SUPPORTED_CATALOGUE_SCHEMA);
    }

    /// An unset or blank setting is "serve what this build embeds", not "load the file named by the
    /// empty string".
    #[test]
    fn an_unset_or_blank_setting_serves_the_embedded_catalogue() {
        for unset in [None, Some(""), Some("   ")] {
            let served = ServedCatalogue::configured(unset)
                .expect("nothing configured is not a startup refusal");
            assert_eq!(served.origin(), CatalogueOrigin::Embedded);
        }
    }

    /// A configured path that is not there refuses; it does not quietly become the embedded
    /// catalogue, which is the whole discipline.
    #[test]
    fn a_configured_path_that_is_absent_refuses_rather_than_defaulting() {
        let refusal = ServedCatalogue::configured(Some("/nonexistent/x153/catalog.pack"))
            .expect_err("a configured pack that is not there refuses");

        assert_eq!(refusal.check(), "readable");
        assert_eq!(refusal.path(), "/nonexistent/x153/catalog.pack");
    }

    /// The report is what both surfaces publish, and it carries no path.
    ///
    /// Asserted over the serialised form rather than the struct, because the property is about what
    /// reaches an anonymous caller: a field added later without thinking about disclosure fails
    /// here rather than on a deployment.
    #[test]
    fn the_published_report_names_the_catalogue_and_not_the_deployment() {
        let report = ServedCatalogue::embedded().report();
        let json = serde_json::to_value(&report).expect("the report serialises");
        let object = json.as_object().expect("a JSON object");

        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "digest",
                "operations",
                "providers",
                "schema_version",
                "source"
            ],
            "the catalogue report gained or lost a field; both anonymous surfaces publish it",
        );
        assert_eq!(object["source"], "embedded");
    }
}
