//! **This host constructs no request of its own** — enforced, not intended (X-12).
//!
//! `docs/vision.md` and `AGENTS.md` both state it: every execution path ends in `connector_pack`,
//! evaluating the operation's own compiled Flux. A second request-building path is how this becomes
//! the credential-injecting proxy the family already rejected.
//!
//! The rule is easy to state and easy to erode, and the erosion is always a small,
//! reasonable-looking addition — a health-check pinger, a token refresh, an OAuth code exchange, a
//! webhook registration helper. Each needs an HTTP client, and once one exists the second request
//! path is a function call away.
//!
//! `tests/invoke.rs` is lock 3, the counting transport. It proves things about the paths it drives
//! and can say nothing about paths nobody wrote a test for. **This file is what speaks to absence**,
//! and it does so twice:
//!
//! - **Lock 1** — the dispatching crate's own `[dependencies]` table is an **allow-list**. Not a
//!   deny-list: a deny-list only catches the transports somebody thought of, and passes for `ureq`,
//!   `isahc`, `attohttpc` and whatever ships next year. An allow-list fails on *any* new dependency,
//!   and whoever adds one has to write down why it is not a transport.
//! - **Lock 2** — one seam, counted, over this crate's sources.
//!
//! # If this test is red and your change is not about transports
//!
//! That is the intended cost, and the failure mode is deletion. **Add your dependency to
//! [`ALLOWED`] with its reason, or your file to the seam rules below — do not delete the rule.** A
//! diff that removes one of these is a blocker; `docs/designs/invoke.md` §3 says so in as many
//! words.
//!
//! # Why the manifest and not `cargo metadata`
//!
//! The resolved graph unifies features across the workspace, so `connector-secrets`' optional
//! `vault` feature — which pulls `reqwest` — enabled anywhere would make a closure-based check
//! either fail spuriously or need an exception that swallows the real signal. A crate's own
//! `[dependencies]` table is unaffected by unification, and it is exactly the thing a second request
//! path would have to change.

use std::fs;
use std::path::{Path, PathBuf};

/// This crate's own manifest, read at compile time. No network, no `cargo` invocation.
const HOST_MANIFEST: &str = include_str!("../Cargo.toml");

/// **Every dependency `exchange-host` may have, and why it is not a transport.**
///
/// The reason is the point of the table. A name on its own would be a list somebody extends without
/// thinking; a name with a sentence beside it is a decision recorded where the next person meets it.
const ALLOWED: &[(&str, &str)] = &[
    (
        "async-trait",
        "a proc macro. It desugars a signature and links nothing.",
    ),
    (
        "connector-address",
        "the credential address vocabulary. Its only dependency is `thiserror`; it composes strings \
         and resolves nothing.",
    ),
    (
        "connector-catalog",
        "the compiled-in catalogue: `&'static` connector data with **zero** dependencies. It is \
         where an operation's declared runtime and its provider come from, and it cannot reach \
         anything.",
    ),
    (
        "connector-pack",
        "the thing that builds the request, and the whole point. It holds no HTTP client either — \
         its transport is the `Egress` this crate hands it, which is the same port one level down.",
    ),
    (
        "connector-secrets",
        "the `SecretStore` port and its file and memory bindings. Its Vault client, and the \
         `reqwest` behind it, sit behind the non-default `vault` feature, which this crate does not \
         enable.",
    ),
    (
        "flux-core",
        "flux's `Error` and `Result` — the values `Tool::execute` answers with. Its dependencies \
         are `serde`, `serde_json` and `thiserror`. Telling a refusal that precedes dispatch from a \
         transport failure means matching on that enum, and matching on it means naming the crate.",
    ),
    (
        "flux-runtime",
        "`Tool`, `ToolContext` and `ToolRegistry` — the seam the pack hands tools out through. It \
         reaches `flux-system` transitively, which is what lock 2's source rules cover; it is not \
         itself a client.",
    ),
    ("serde", "derives. No IO."),
    ("serde_json", "a parser. No IO."),
    ("thiserror", "a proc macro. No IO."),
];

/// The source rules of lock 2, each as a sentence a failure can quote.
///
/// They are stated as data and checked by [`violations`] so that
/// [`the_scanner_catches_what_it_claims_to`] can run the identical function over sources it **must**
/// reject and sources it **must** accept. A scanner that has not just proved it catches a violation
/// is not evidence there are none — a regex that matches nothing passes every file.
mod rules {
    /// The pack's caller-facing entry point. Exactly one file may name it.
    pub const SEAM: &str = "connector_pack::resolve";

    /// The pack's *model-facing* entry point, which installs a whole provider's tools into a
    /// registry a host advertises. An execute route wants `resolve`; naming `pack` here would mean
    /// a second way in, and would silently withhold every `expose = false` operation from callers
    /// entitled to run them.
    pub const MODEL_FACING_SEAM: &str = "connector_pack::pack";

    /// Unwrapping the transport out of its `Egress`. The host hands the port to the pack and never
    /// calls the tool inside it; doing so *is* the second request path, in one line.
    pub const UNWRAPS_THE_TRANSPORT: &str = ".tool()";

    /// Dispatching a tool. Only the seam may, and only on the operation the pack resolved.
    pub const DISPATCH: &str = ".execute(";

    /// Names no source in this crate may carry. `flux-system` is where flux's real IO lives —
    /// `flux_system::net` dials — and it is reachable transitively through `flux-runtime`, which is
    /// exactly why naming it is refused here rather than only in the manifest.
    pub const FORBIDDEN: &[&str] = &[
        "flux_system",
        "std::net",
        "tokio::net",
        "reqwest",
        "hyper",
        "ureq",
        "isahc",
        "attohttpc",
        "TcpStream",
        "UdpSocket",
    ];

    /// Files allowed to name `Egress`: the seam that hands it over, and the crate root that
    /// re-exports it so a composition need not name `connector-pack`.
    pub const MAY_NAME_EGRESS: &[&str] = &["invoke.rs", "lib.rs"];
}

// ---------------------------------------------------------------------------------------------
// Lock 1 — the manifest is an allow-list
// ---------------------------------------------------------------------------------------------

/// Every `[dependencies]` entry of this crate is one somebody wrote a reason for.
#[test]
fn the_dispatching_crate_declares_only_dependencies_that_are_not_transports() {
    let declared = dependencies_of(HOST_MANIFEST);

    assert!(
        !declared.is_empty(),
        "no `[dependencies]` were parsed out of the manifest, so this test is asserting nothing — \
         either the table moved or the parser stopped matching it",
    );

    for name in &declared {
        assert!(
            ALLOWED.iter().any(|(allowed, _)| allowed == name),
            "`{name}` is a new dependency of the crate that dispatches every operation.\n\n\
             This table is an allow-list, not a deny-list, and it is deliberately annoying: the \
             property that makes this host safe is that there is no transport here to build a \
             second request with, and a deny-list would pass for any client nobody listed.\n\n\
             The cheapest correct fix is to add `{name}` to `ALLOWED` in this file **with a \
             sentence saying why it is not a transport**. Deleting this test is a blocker — see \
             `docs/designs/invoke.md` §3, lock 1.",
        );
    }
}

/// The allow-list does not accumulate entries for dependencies that have gone.
///
/// The other half of "subset": without it the table grows a permanent record of every dependency
/// this crate ever had, and the next person reads a list that no longer describes anything.
#[test]
fn the_allow_list_carries_no_entry_for_a_dependency_that_is_gone() {
    let declared = dependencies_of(HOST_MANIFEST);

    for (allowed, _) in ALLOWED {
        assert!(
            declared.iter().any(|name| name == allowed),
            "`{allowed}` is on the allow-list and is no longer a dependency; drop the entry",
        );
    }
}

/// **The complementary assertion, and it is one line of intent.**
///
/// The crate that can build a request cannot name the pack; the crate that names the pack cannot
/// build a request. `exchange-server` holds `flux_web`'s `HttpRequestTool` and the server
/// framework, and it reaches the pack only through `exchange_host`'s re-exports.
#[test]
fn the_crate_that_holds_a_transport_never_names_the_pack() {
    let sources = sources_under(&workspace_path("crates/exchange-server/src"));

    assert!(
        sources.len() > 5,
        "only {} source files were found under `exchange-server/src`; the walk is broken and this \
         test is asserting nothing",
        sources.len(),
    );

    for (path, source) in &sources {
        assert!(
            !code_of(source).contains("connector_pack"),
            "`{path}` names `connector_pack`, and it is the crate that holds an HTTP client.\n\n\
             Bind the pack's ports through `exchange_host`'s re-exports (`exchange_host::Egress`, \
             `exchange_host::ConfigStore`) instead. See `docs/designs/invoke.md` §3, lock 1.",
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Lock 2 — one seam, counted
// ---------------------------------------------------------------------------------------------

/// The real sources satisfy every rule in [`rules`].
#[test]
fn the_dispatch_seam_is_one_file_and_nothing_here_can_open_a_socket() {
    let sources = sources_under(&workspace_path("crates/exchange-host/src"));

    assert!(
        sources.len() > 3,
        "only {} source files were found under `exchange-host/src`; the walk is broken",
        sources.len(),
    );

    let found = violations(&sources);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// **The scanner, proved against sources it must reject and sources it must accept.**
///
/// Source scanning is a blunt instrument and it is used here because this repository already runs
/// one and already knows how to keep it honest — `console/test/components.test.mjs` is guarded
/// exactly this way. Without this, a rule whose marker stopped matching would pass every file
/// forever and read as though it were guarding something.
#[test]
fn the_scanner_catches_what_it_claims_to() {
    // The minimum shape of a compliant crate: one seam file, and nothing else notable.
    let seam = || {
        (
            "invoke.rs".to_owned(),
            format!(
                "let tool = {}(entry, egress, credentials, settings)?;\n\
                 let result = tool.execute(&ctx, params).await?;\n",
                rules::SEAM
            ),
        )
    };

    assert!(
        violations(&[seam()]).is_empty(),
        "the scanner rejects the shape it exists to permit",
    );

    // Comments are not references. A rule that could not tell them apart would make this file's own
    // documentation a violation, and the fix somebody would reach for is to stop documenting.
    let commented = (
        "notes.rs".to_owned(),
        format!(
            "//! We do not use reqwest, and never name {} or flux_system::net here.\n\
             /// See `.tool()` for what this deliberately does not call.\n",
            rules::MODEL_FACING_SEAM
        ),
    );
    assert!(
        violations(&[seam(), commented]).is_empty(),
        "the scanner cannot tell a comment from a reference, so every real rule below is one \
         doc-comment away from a false positive",
    );

    // And every rule, each driven on its own so a failure names the rule that stopped working.
    let must_reject: &[(&str, &str)] = &[
        ("a direct HTTP client", "use reqwest::Client;"),
        ("a raw socket", "use tokio::net::TcpStream;"),
        ("flux's IO crate", "use flux_system::System;"),
        (
            "unwrapping the transport",
            "let http = self.egress.tool().clone();",
        ),
        (
            "the model-facing pack entry point",
            "connector_pack::pack(&[provider], egress, credentials, settings)",
        ),
    ];

    for (what, line) in must_reject {
        let found = violations(&[seam(), ("second.rs".to_owned(), (*line).to_owned())]);
        assert!(
            !found.is_empty(),
            "the scanner accepted {what} (`{line}`), so the rule that should catch it is dead",
        );
    }

    // A second file naming the seam is a second request path by definition.
    let found = violations(&[seam(), ("other.rs".to_owned(), seam().1)]);
    assert!(
        !found.is_empty(),
        "the scanner accepted two dispatch seams, which is the whole thing it counts",
    );

    // And no seam at all must fail too, or deleting `invoke.rs` would satisfy every rule here.
    let found = violations(&[("empty.rs".to_owned(), String::new())]);
    assert!(
        !found.is_empty(),
        "the scanner accepted a crate with no dispatch seam, so `exactly one` is really `at most \
         one` and a scanner that matches nothing passes",
    );

    // `Egress` outside the two files that may name it.
    let found = violations(&[
        seam(),
        ("helper.rs".to_owned(), "fn f(e: Egress) {}".to_owned()),
    ]);
    assert!(
        !found.is_empty(),
        "the scanner accepted `Egress` in a third file",
    );
}

// ---------------------------------------------------------------------------------------------
// The scanner itself
// ---------------------------------------------------------------------------------------------

/// Every rule of [`rules`] applied to `sources`, as human-readable violations.
///
/// A pure function over `(name, source)` pairs, which is what lets the self-test above drive it
/// over fixtures rather than over the tree it is meant to guard.
fn violations(sources: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();

    let code: Vec<(&str, String)> = sources
        .iter()
        .map(|(path, source)| (path.as_str(), code_of(source)))
        .collect();

    let naming = |needle: &str| -> Vec<&str> {
        code.iter()
            .filter(|(_, source)| source.contains(needle))
            .map(|(path, _)| *path)
            .collect()
    };

    let seams = naming(rules::SEAM);
    if seams.len() != 1 {
        found.push(format!(
            "`{}` is named in {} files ({seams:?}); exactly one file in this crate dispatches an \
             operation, and that is what makes \"this host builds no request of its own\" a \
             countable fact rather than a habit.",
            rules::SEAM,
            seams.len(),
        ));
    }

    for (needle, why) in [
        (
            rules::MODEL_FACING_SEAM,
            "that is the pack's model-facing entry point; an execute route resolves one named \
             operation through `connector_pack::resolve`, which withholds nothing",
        ),
        (
            rules::UNWRAPS_THE_TRANSPORT,
            "unwrapping the transport out of its `Egress` is the second request path, in one line",
        ),
    ] {
        for path in naming(needle) {
            found.push(format!("`{path}` names `{needle}`: {why}."));
        }
    }

    for path in naming(rules::DISPATCH) {
        if !seams.contains(&path) {
            found.push(format!(
                "`{path}` dispatches a tool (`{}`) and is not the seam; only the operation the pack \
                 resolved may be executed, and only from there.",
                rules::DISPATCH,
            ));
        }
    }

    for needle in rules::FORBIDDEN {
        for path in naming(needle) {
            found.push(format!(
                "`{path}` names `{needle}`. This crate reaches the network through the `Egress` a \
                 composition bound and through nothing else — see `docs/designs/invoke.md` §3, \
                 lock 2.",
            ));
        }
    }

    for path in naming("Egress") {
        if !rules::MAY_NAME_EGRESS
            .iter()
            .any(|allowed| path.ends_with(allowed))
        {
            found.push(format!(
                "`{path}` names `Egress`; the transport port belongs to the seam and to the \
                 crate root that re-exports it, so that where it travels is readable in one place.",
            ));
        }
    }

    found
}

/// `source` with whole-line comments removed.
///
/// Every rule above is about what the code *does*, and this file's own documentation names several
/// of the markers it forbids. A scanner that could not tell the two apart would make documenting
/// the rule a violation of it — so the classification happens before the judgment, exactly as
/// `scripts/check-action-pins.sh` classifies a workflow line before deciding about it.
///
/// Whole-line only, and deliberately so: a trailing `//` comment is rare in this repository's style
/// and stripping one correctly means knowing whether the slashes are inside a string literal. An
/// over-eager strip would create blind spots; this one can only ever produce a false *positive*,
/// which is the failure direction a guard should have.
fn code_of(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The names in one manifest's `[dependencies]` table.
///
/// Deliberately not a TOML parse: reading these lines needs no dependency, and the shapes it accepts
/// are the shapes this workspace actually writes (`name.workspace = true`, `name = { … }`,
/// `name = "…"`). `the_dispatching_crate_…` asserts it found some, so a table it cannot read is a
/// failure rather than a silent pass — which is the failure mode a hand-rolled parser has.
fn dependencies_of(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;

    for line in manifest.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            inside = line == "[dependencies]";
            continue;
        }
        if !inside || line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, _)) = line.split_once('=') else {
            continue;
        };
        let name = key.trim().split('.').next().unwrap_or_default().trim();
        if !name.is_empty() {
            names.push(name.to_owned());
        }
    }

    names
}

/// A path relative to the workspace root.
fn workspace_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
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
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("`{}` is readable: {error}", directory.display()));

    for entry in entries {
        let path = entry.expect("a readable directory entry").path();

        if path.is_dir() {
            collect(root, &path, sources);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path
                .strip_prefix(root)
                .expect("every walked path is under the root")
                .display()
                .to_string();
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("`{}` is readable: {error}", path.display()));
            sources.push((relative, contents));
        }
    }
}
