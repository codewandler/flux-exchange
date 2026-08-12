//! **Where this deployment's connector catalogue comes from** (X-153).
//!
//! One setting, read once, at startup. [`CATALOGUE_PACK_ENV`] names a catalogue pack on disk; unset
//! means the catalogue this build embeds, which is what every deployment served before this story.
//!
//! # Why a deployment would set it
//!
//! Decision 0022 makes the catalogue a **data** dependency rather than a code one. Until it, a new
//! provider — or a corrected vendor quirk — reached this service only through a crates.io release
//! and an Exchange rebuild. A deployment that points this setting at a newer pack serves a
//! catalogue newer than the binary it was built with, and the flux-connectors release that
//! published it attaches `catalog.pack` beside a `catalog.pack.sha256`, so there is a supported
//! place to fetch one from and a published digest to check it against.
//!
//! # Why it is an environment variable read here and nowhere else
//!
//! It is a **store binding**, and it follows the rule every other store binding in this service
//! follows: deployment configuration, read by the composition at startup, never derived from a
//! request. `FLUX_EXCHANGE_ACQUISITION_CONNECTORS`, `FLUX_EXCHANGE_GRANTS` and the rest are read the
//! same way, in the same place, for the same reason — a path a caller can influence is a path a
//! caller can point at a file the caller wrote.
//!
//! That rule is enforced rather than intended: [`tests::no_route_can_reach_the_catalogue_setting`]
//! scans the route sources for this setting's name and for the loading constructor. It is a name
//! check over one directory and it says so; what it catches is the shape of the mistake — a handler
//! that reads the setting, or loads a pack, for a caller.
//!
//! # Refuse; never repair
//!
//! A configured pack that cannot be verified refuses **startup**. It does not fall back to the
//! embedded catalogue, and the reason it must not is that the fallback has no symptom: the process
//! starts, every request succeeds, and the answers come from a catalogue nobody configured. The
//! refusals are `exchange_host::CatalogueRefusal`'s, each naming which check failed, and they arrive
//! here as one [`StartupRefusal::CataloguePack`](crate::bind::StartupRefusal::CataloguePack).

use std::sync::Arc;

use exchange_host::{CatalogueRefusal, ServedCatalogue};

/// The environment variable naming a connector catalogue pack to serve instead of the embedded one.
///
/// Unset — or set to nothing — serves what this build embeds.
pub const CATALOGUE_PACK_ENV: &str = "FLUX_EXCHANGE_CATALOGUE_PACK";

/// The catalogue this deployment serves, from its environment.
///
/// # Errors
///
/// One [`CatalogueRefusal`], naming the configured path and which verification failed. A pack that
/// does not verify is a startup refusal; there is no arm that answers with the embedded catalogue.
pub fn configured() -> Result<Arc<ServedCatalogue>, CatalogueRefusal> {
    from_environment(|name| std::env::var(name).ok())
}

/// [`configured`], reading by name from an injected source so tests do not mutate process-wide
/// environment — the shape `crate::credential_acquisition::from_environment` already uses.
fn from_environment(
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Arc<ServedCatalogue>, CatalogueRefusal> {
    ServedCatalogue::configured(lookup(CATALOGUE_PACK_ENV).as_deref()).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use exchange_host::CatalogueOrigin;

    use super::*;

    /// A deployment that configures nothing serves the catalogue this build embeds.
    #[test]
    fn an_unconfigured_deployment_serves_the_embedded_catalogue() {
        let served = from_environment(|_| None).expect("nothing configured is not a refusal");

        assert_eq!(served.origin(), CatalogueOrigin::Embedded);
        assert_eq!(served.path(), None);
    }

    /// **A configured pack that does not verify refuses startup, naming the check and the path.**
    ///
    /// The assertion that carries the story is the first one: the result is an error. A composition
    /// that answered `Ok` with the embedded catalogue here would start a deployment whose operator
    /// believes it is serving their pack.
    #[test]
    fn a_configured_pack_that_does_not_verify_refuses_rather_than_falling_back() {
        let refusal = from_environment(|name| {
            (name == CATALOGUE_PACK_ENV).then(|| "/nonexistent/x153/catalog.pack".to_owned())
        })
        .expect_err("a pack that cannot be read refuses startup");

        assert_eq!(refusal.check(), "readable");
        assert!(
            refusal
                .to_string()
                .contains("/nonexistent/x153/catalog.pack"),
            "the refusal names the path the operator configured: {refusal}",
        );
    }

    /// A setting present but blank is "unset", not the path `""`.
    #[test]
    fn a_blank_setting_is_not_a_filename() {
        let served = from_environment(|name| (name == CATALOGUE_PACK_ENV).then(String::new))
            .expect("a blank setting is not a startup refusal");

        assert_eq!(served.origin(), CatalogueOrigin::Embedded);
    }

    /// **The configured path is startup configuration and no route can reach it** (acceptance 4).
    ///
    /// The rule every store binding in this service follows, made countable for this one. A handler
    /// that read [`CATALOGUE_PACK_ENV`] would be reading deployment configuration per request; a
    /// handler that named `ServedCatalogue::load` would be loading a pack from a path that reached
    /// it through a request. Routes read the catalogue the composition already verified, through
    /// `AppState::catalogue`, and that is the only way in.
    ///
    /// **A name check over one directory**, and openly that — the same instrument, and the same
    /// limit, as `crates/exchange-host/tests/no_second_request_path.rs`'s lock 2. It cannot see a
    /// path arriving under a name nobody listed. What it catches is the shape of the mistake
    /// somebody makes by accident, at the moment they make it.
    #[test]
    fn no_route_can_reach_the_catalogue_setting() {
        let routes = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/routes");
        let sources = sources_under(&routes);

        assert!(
            sources.len() > 5,
            "only {} route sources were found under `{}`; the walk is broken and this test is \
             asserting nothing",
            sources.len(),
            routes.display(),
        );

        for (path, source) in &sources {
            let code: String = source
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");

            for (needle, why) in [
                (
                    CATALOGUE_PACK_ENV,
                    "that is deployment configuration, and a route reading it is a route reading \
                     it per request",
                ),
                (
                    "ServedCatalogue::load",
                    "loading a pack is a startup act with a startup refusal; a route that loads \
                     one is a route taking a filesystem path from something a caller can reach",
                ),
                (
                    "ServedCatalogue::configured",
                    "the composition decides which catalogue is served, once — a route that \
                     re-decided it could serve a different catalogue per request",
                ),
            ] {
                assert!(
                    !code.contains(needle),
                    "`{path}` names `{needle}`: {why}.\n\n\
                     Read the catalogue this composition already verified, through \
                     `AppState::catalogue`. See `crate::catalogue` for why the setting is read at \
                     startup and nowhere else.",
                );
            }
        }
    }

    /// **A deployment serving a loaded pack says so, on both surfaces that report one** (X-153).
    ///
    /// The half a composition with nothing configured cannot reach: on the embedded default, a
    /// `source` field hard-coded to `"embedded"` and a digest read off the wrong pack would both
    /// pass. This binds a composition to a pack that is *not* the embedded one and asserts the
    /// descriptor and the catalogue listing both report **that** pack's digest — which is what an
    /// operator debugging a missing operation actually reads, and the reason the seam is one value
    /// rather than a rendering per route.
    ///
    /// It lives here rather than beside either route because loading is this module's job:
    /// [`no_route_can_reach_the_catalogue_setting`] refuses `ServedCatalogue::load` under
    /// `src/routes`, and a test fixture is not a reason to carve an exception into a rule about
    /// where a filesystem path may be taken.
    #[tokio::test]
    async fn a_loaded_pack_is_reported_by_the_descriptor_and_the_catalogue_listing() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let directory = std::env::temp_dir().join(format!(
            "flux-exchange-x153-surfaces-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).expect("a writable temporary directory");
        let path = directory.join("catalog.pack");
        std::fs::write(&path, one_provider_pack()).expect("the fixture pack is writable");

        let loaded = ServedCatalogue::load(&path).expect("a well-formed version-1 pack loads");
        let digest = loaded.digest().to_owned();
        assert_ne!(
            digest,
            ServedCatalogue::embedded().digest(),
            "the fixture must not be the embedded pack, or this test asserts nothing",
        );

        let app = crate::routes::app(
            crate::state::AppState::without_identity().with_catalogue(Arc::new(loaded)),
        );

        let fetch = |path: &'static str| {
            let app = app.clone();
            async move {
                let response = app
                    .oneshot(
                        Request::builder()
                            .uri(path)
                            .body(Body::empty())
                            .expect("a well-formed request"),
                    )
                    .await
                    .expect("the router answers");
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("a readable body");
                String::from_utf8(bytes.to_vec()).expect("a UTF-8 body")
            }
        };

        let descriptor = fetch("/api/onboarding").await;
        let listing = fetch("/api/catalogue/connectors").await;

        for (surface, body) in [("the descriptor", &descriptor), ("the listing", &listing)] {
            let document: serde_json::Value =
                serde_json::from_str(body).unwrap_or_else(|_| panic!("{surface}: {body}"));
            let catalogue = &document["catalogue"];

            assert_eq!(
                catalogue["source"], "loaded",
                "{surface} reports the embedded catalogue on a deployment serving a loaded one",
            );
            assert_eq!(
                catalogue["digest"], digest,
                "{surface} reports a digest that is not the loaded pack's",
            );
            assert_eq!(
                catalogue["providers"], 1,
                "{surface} reports the embedded catalogue's shape rather than the loaded pack's",
            );
            assert!(
                !body.contains(&path.display().to_string()),
                "{surface} published the configured path, which is deployment layout rather than \
                 catalogue identity and reaches an anonymous caller here: {body}",
            );
        }

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// **What a loaded pack does *not* yet change, recorded rather than left to be discovered.**
    ///
    /// This is a characterization of a real split, and X-153 delivers it deliberately rather than
    /// hiding it. A loaded pack changes what the two surfaces **report** about the catalogue. It
    /// does not change what this host **serves and executes**, because both of those still resolve
    /// through `&'static` data:
    ///
    /// - the connector listing's entries come from `connector_catalog`'s generated tables;
    /// - settings and verification resolve through `connector_pack::DocumentRehearsal::of(id)`,
    ///   whose signature takes an id and nothing else and whose fields are `&'static` — it reads
    ///   `connector-resolve`'s own embedded documents, and there is no constructor on it that a
    ///   loaded pack could be handed to.
    ///
    /// So a deployment that loads a pack carrying a provider this binary was not built with will
    /// see that provider counted in `catalogue`, and **will not** be able to connect to it or
    /// invoke it. Making those converge needs upstream to retire the Flux emitter (C-540) and to
    /// offer a pack-parameterised rehearsal; it is not something this host can fix from the outside,
    /// and faking it by serving records the execution path cannot honour would be the exact
    /// "repair" this story's discipline forbids.
    ///
    /// **If this test fails, the split closed** — that is good news, and the assertion is what
    /// makes it a decision somebody makes rather than a change nobody notices.
    #[tokio::test]
    async fn a_loaded_pack_is_reported_but_does_not_yet_change_what_is_served() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let directory =
            std::env::temp_dir().join(format!("flux-exchange-x153-split-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("a writable temporary directory");
        let path = directory.join("catalog.pack");
        std::fs::write(&path, one_provider_pack()).expect("the fixture pack is writable");

        let loaded = ServedCatalogue::load(&path).expect("a well-formed version-1 pack loads");
        assert_eq!(
            loaded.provider_ids(),
            vec!["acme"],
            "the fixture carries one provider the embedded catalogue does not have",
        );

        let app = crate::routes::app(
            crate::state::AppState::without_identity().with_catalogue(Arc::new(loaded)),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/catalogue/connectors")
                    .body(Body::empty())
                    .expect("a well-formed request"),
            )
            .await
            .expect("the router answers");
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a readable body");
        let listing: serde_json::Value = serde_json::from_slice(&bytes).expect("a JSON document");

        assert_eq!(
            listing["catalogue"]["providers"], 1,
            "the reported catalogue is the loaded pack",
        );

        let entries = listing["connectors"]
            .as_array()
            .expect("connectors is an array");
        assert_eq!(
            entries.len(),
            connector_catalog::providers().len(),
            "the listing's entries still come from the generated `&'static` tables; if this moved, \
             the split this test records has closed and its documentation is what needs rewriting",
        );
        assert!(
            !entries.iter().any(|entry| entry["id"] == "acme"),
            "the loaded pack's provider is being listed as connectable, and this host cannot \
             invoke it: the execution path resolves through the pack, which reads its own \
             embedded documents",
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A minimal, well-formed version-1 pack that is not the embedded one.
    ///
    /// Digested with `sha2` rather than with the reader's vendored SHA-256, so the fixture and the
    /// code that verifies it are not one piece of arithmetic agreeing with itself.
    fn one_provider_pack() -> Vec<u8> {
        use sha2::{Digest, Sha256};

        let payload = "{\"id\":\"acme\"}\n";
        let body = format!(
            "schema 1\nproviders 1\noperations 1\n\
             p acme 0 {len}\no acme-thing-get acme default 0 {len}\npayload {len}\n{payload}",
            len = payload.len(),
        );
        let mut hex = String::with_capacity(64);
        for byte in Sha256::digest(body.as_bytes()) {
            hex.push_str(&format!("{byte:02x}"));
        }
        format!("flux-connectors-catalog-pack 1\ndigest sha256 {hex}\n{body}").into_bytes()
    }

    /// Every `.rs` file under `root`, as `(path-relative-to-root, contents)`.
    fn sources_under(root: &Path) -> Vec<(String, String)> {
        let mut sources = Vec::new();
        collect(root, root, &mut sources);
        sources.sort();
        sources
    }

    /// The recursive half of [`sources_under`].
    fn collect(root: &Path, directory: &Path, sources: &mut Vec<(String, String)>) {
        let entries = std::fs::read_dir(directory)
            .unwrap_or_else(|error| panic!("`{}` is readable: {error}", directory.display()));

        for entry in entries {
            let path: PathBuf = entry.expect("a readable directory entry").path();

            if path.is_dir() {
                collect(root, &path, sources);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let relative = path
                    .strip_prefix(root)
                    .expect("every walked path is under the root")
                    .display()
                    .to_string();
                let contents = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("`{}` is readable: {error}", path.display()));
                sources.push((relative, contents));
            }
        }
    }
}
