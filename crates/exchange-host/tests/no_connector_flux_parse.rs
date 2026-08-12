//! **This crate reaches no connector Flux text on the way to a settings or verification answer**
//! (X-152).
//!
//! Retiring a parse is not the same as keeping it retired. Until X-152 the whole configuration
//! surface — what a connections page asks a tenant for, what effective discovery narrows to, and
//! whether an operator-approved origin verifies — was recovered by handing
//! `connector_catalog::Operation::flux` to `connector_pack::Rehearsal`, which compiles the emitted
//! Flux at runtime. It is now read from the catalogue's canonical document through
//! `connector_pack::DocumentRehearsal`. Both entry points still exist upstream and take the same
//! questions, so nothing about the type system stops the old one coming back one call site at a
//! time; the swap survives as a fact only while something counts it.
//!
//! # The distinction this file is careful about, and X-98 is why
//!
//! **`flux_lang` is not going anywhere, and this file must never be read as saying it should.**
//! There are two entirely separate Flux parses in this repository:
//!
//! - the **connector** parse, which took an operation's emitted Flux and recovered the request the
//!   connector would compose. That is the one X-152 retired, because the same facts are published
//!   in the catalogue document and recovering them was a second derivation of somebody else's data.
//! - the **workflow** parse (X-98), which compiles a *tenant's own* draft — `flux_lang::parse`,
//!   `flux_lang::analyze::lower` and the editor projection in `workflow.rs` and `invoke.rs`. There
//!   is no artifact to read that from: the source is the tenant's, written minutes ago, and
//!   compiling it is the whole feature.
//!
//! So [`the_workflow_flux_parse_is_untouched`] asserts the second one is still here. A rule that
//! forbade Flux parsing outright would be satisfied by deleting X-98, which is the opposite of what
//! this file is for.
//!
//! # Where this sits relative to `no_second_request_path.rs`
//!
//! That file's lock 2 bounds *how many ways into `connector-pack` this crate has*, and counts
//! `connector_pack::Rehearsal` as one of them. This file bounds *which derivation the settings and
//! verification answers come from*. The two overlap on one string and answer different questions:
//! lock 2 would be satisfied by any number of `Rehearsal` calls inside `settings.rs`, and this file
//! is satisfied by none anywhere.
//!
//! **X-156 widened lock 2 to count `connector_pack::DocumentRehearsal` as a fourth pack entry
//! point**, with the sentence in `docs/designs/invoke.md`'s lock 2 section that
//! `the_design_says_what_every_lock_2_rule_is` demands. Both files were outside X-152's fence,
//! which is why the bound lived here alone for a story's span.
//!
//! [`the_document_backed_rehearsal_stays_in_the_settings_module`] is **kept** rather than subsumed,
//! and the reason is the one this section already gives about `Rehearsal`: the two answer different
//! questions. Lock 2's rule is a *bounded-files refusal* — it is satisfied by `settings.rs` naming
//! the rehearsal any number of times, and equally by **no file naming it at all**, which is the
//! shape of a silent revert to the parse this story retired. The assertion below is an equality
//! against `["settings.rs"]`, so it fails in that second direction too. Deleting it would keep the
//! half that says *not elsewhere* and lose the half that says *here, and still here*.

use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------
// The rules, as data
// ---------------------------------------------------------------------------------------------

/// The rules, stated once so [`the_scanner_catches_what_it_claims_to`] can drive the identical
/// predicate over sources it **must** reject and sources it **must** accept.
///
/// A scanner that has not just proved it catches a violation is not evidence there are none; this
/// repository already says so about `scripts/check-action-pins.sh`, and the same standard applies to
/// a rule written in a test.
mod rules {
    /// **Reading a catalogue entry's emitted Flux.** `entry.flux`, `operation.flux`, `verify.flux`
    /// — every spelling ends in this, and there is no other reason for the four characters to
    /// appear in this crate's code.
    pub const CONNECTOR_FLUX_TEXT: &str = ".flux";

    /// **The emitted-Flux rehearsal**, matched as a whole identifier so that
    /// [`DOCUMENT_REHEARSAL`] — which is a different identifier ending in the same letters — is not
    /// mistaken for it.
    pub const FLUX_REHEARSAL: &str = "Rehearsal";

    /// **The document-backed rehearsal**, the replacement.
    pub const DOCUMENT_REHEARSAL: &str = "DocumentRehearsal";

    /// The one file that derives a connector's configuration surface, and therefore the one file
    /// that may enter [`DOCUMENT_REHEARSAL`]. The same bound `no_second_request_path.rs` puts on
    /// the pack's other entry points, for the same reason: the list of ways into `connector-pack`
    /// from this crate is small on purpose.
    pub const MAY_NAME_DOCUMENT_REHEARSAL: &[&str] = &["settings.rs"];

    /// The tenant-workflow parse, which is **not** what this file forbids. See the module doc.
    pub const WORKFLOW_PARSE: &str = "flux_lang::parse";
}

/// **What the canonical document carries that this crate does not read**, recorded rather than
/// silently ignored.
///
/// **flux-connectors'** `docs/designs/catalog-artifact.md` — the upstream design, not one of this
/// repository's; there is no `docs/designs/catalog-artifact.md` here — names four surfaces the
/// document publishes which reached no artifact before it. A document richer than the parse it
/// replaced should not leave a consumer quietly dropping the difference, so each is written down
/// here with why Exchange does not consume it, and
/// [`the_unconsumed_document_surfaces_are_still_unconsumed`] fails the moment one of them starts
/// being consumed — which is when this list needs re-deciding rather than extending.
///
/// The common reason, and it is a hard one rather than a preference: `connector-pack` publishes no
/// accessor for any of them. `DocumentRehearsal` exposes the contract, exposure, endpoint variables,
/// endpoint slots, caller path parameters and the composed request; `connector_pack::document_of` is
/// `pub(crate)`, and `connector-resolve`'s own document module states that quirks, response schemas,
/// events and the OAuth2 spec are *"skipped rather than modelled"*. So consuming one of these is not
/// a call this crate can add — it is an upstream surface that has to exist first.
const NOT_CONSUMED: &[(&str, &str)] = &[
    (
        "roles",
        "a per-service classification (`llm_catalogue`, …). Exchange selects operations by grant \
         metadata — risk, effects, idempotency — never by a name or a label, so a role would be a \
         second selection vocabulary beside the one `AGENTS.md` says is the only one.",
    ),
    (
        "quirks.pagination",
        "how a vendor pages a collection. Exchange runs one operation for a caller and hands the \
         result back; it does not iterate, and a host that paged on the caller's behalf would be \
         composing a second request.",
    ),
    (
        "quirks.rate_limit",
        "the vendor's declared limits. Exchange does not schedule or throttle vendor calls today; \
         when it does, this is the input, and that is a story rather than a line of plumbing.",
    ),
    (
        "graphs",
        "connector-declared operation graphs. Exchange composes multi-step work through X-98's \
         tenant workflows, which are the tenant's own Flux; adopting connector-declared graphs is a \
         product decision about which of the two composes, not a migration detail.",
    ),
];

// ---------------------------------------------------------------------------------------------
// The scanner
// ---------------------------------------------------------------------------------------------

/// Whether `identifier` appears in `source` as a whole word.
///
/// Substring matching is what makes this rule wrong rather than strict: `DocumentRehearsal`
/// contains `Rehearsal`, so a `contains` check would report the replacement as the thing it
/// replaced and this file would fail on the very change it exists to protect.
fn names_identifier(source: &str, identifier: &str) -> bool {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    source.match_indices(identifier).any(|(at, _)| {
        let before = source[..at].chars().next_back();
        let after = source[at + identifier.len()..].chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

/// `source` with whole-line `//` comments removed.
///
/// The same shape `no_second_request_path.rs` uses, and load-bearing here for the same reason: this
/// crate's modules explain at length *why* the connector parse was retired, and a scanner that read
/// prose would refuse the explanation.
fn code_of(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every rule applied to `sources`, as human-readable violations.
///
/// A pure function over `(name, source)` pairs, so the self-test drives it over fixtures rather
/// than over the tree it guards.
fn violations(sources: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();

    for (path, source) in sources {
        let code = code_of(source);

        if code.contains(rules::CONNECTOR_FLUX_TEXT) {
            found.push(format!(
                "`{path}` reads a catalogue entry's `{}` — the operation's emitted Flux as text. \
                 The configuration surface and the connection-verification answer come from the \
                 catalogue's canonical document through `connector_pack::DocumentRehearsal`; \
                 handing the emitted Flux to a runtime parse is the derivation X-152 retired, and \
                 the verification site is where a silent difference between the two looks like an \
                 operator's mistake rather than ours.",
                rules::CONNECTOR_FLUX_TEXT,
            ));
        }

        if names_identifier(&code, rules::FLUX_REHEARSAL) {
            found.push(format!(
                "`{path}` names `{}`, the pack entry point that compiles an operation's emitted \
                 Flux at runtime. `{}` answers the same questions from the published document.",
                rules::FLUX_REHEARSAL,
                rules::DOCUMENT_REHEARSAL,
            ));
        }

        if names_identifier(&code, rules::DOCUMENT_REHEARSAL)
            && !rules::MAY_NAME_DOCUMENT_REHEARSAL
                .iter()
                .any(|allowed| path.ends_with(allowed))
        {
            found.push(format!(
                "`{path}` names `{}` outside the module that derives a connector's configuration \
                 surface. The list of ways into `connector-pack` from this crate is bounded on \
                 purpose — see `no_second_request_path.rs` — and a new one belongs in \
                 `MAY_NAME_DOCUMENT_REHEARSAL` with a reason.",
                rules::DOCUMENT_REHEARSAL,
            ));
        }
    }

    found
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

/// This crate's sources, with the walk asserted rather than assumed.
///
/// A walk that found nothing produces no violations, which is a green test that has scanned
/// nothing at all.
fn host_sources() -> Vec<(String, String)> {
    let sources = sources_under(&workspace_path("crates/exchange-host/src"));
    assert!(
        sources.len() > 10,
        "only {} source files were found under `exchange-host/src`; the walk is broken and this \
         file is asserting nothing",
        sources.len(),
    );
    assert!(
        sources
            .iter()
            .any(|(path, _)| path.ends_with("settings.rs")),
        "the walk did not reach `settings.rs`, which is the module every rule here is about",
    );
    sources
}

// ---------------------------------------------------------------------------------------------
// The locks
// ---------------------------------------------------------------------------------------------

/// **No path from the catalogue to a settings or verification answer reaches connector Flux text.**
#[test]
fn no_settings_or_verification_answer_reaches_connector_flux() {
    let found = violations(&host_sources());
    assert!(
        found.is_empty(),
        "the connector Flux parse is back in `exchange-host`:\n\n{}",
        found.join("\n\n"),
    );
}

/// **The document-backed rehearsal is entered from one module**, so its blast radius stays readable.
#[test]
fn the_document_backed_rehearsal_stays_in_the_settings_module() {
    let naming: Vec<String> = host_sources()
        .into_iter()
        .filter(|(_, source)| names_identifier(&code_of(source), rules::DOCUMENT_REHEARSAL))
        .map(|(path, _)| path)
        .collect();

    assert_eq!(
        naming,
        vec!["settings.rs".to_owned()],
        "`{}` is entered from somewhere other than the module that derives a connector's \
         configuration surface",
        rules::DOCUMENT_REHEARSAL,
    );
}

/// **X-98's workflow parse is untouched**, and this file is not an argument for removing it.
///
/// The connector parse and the tenant-workflow parse are different things that happen to use the
/// same word. A future reader arriving at the rule above should find this sentence beside it rather
/// than infer that `flux_lang` is on its way out.
#[test]
fn the_workflow_flux_parse_is_untouched() {
    let sources = host_sources();
    let parsing: Vec<&str> = sources
        .iter()
        .filter(|(_, source)| code_of(source).contains(rules::WORKFLOW_PARSE))
        .map(|(path, _)| path.as_str())
        .collect();

    assert!(
        parsing.contains(&"workflow.rs"),
        "`workflow.rs` no longer calls `{}`. X-98 compiles a *tenant's own* draft, which no \
         artifact publishes and which nothing in X-152 touches; if the workflow parse really is \
         going away, that is a story rather than a consequence of retiring the connector parse.",
        rules::WORKFLOW_PARSE,
    );
}

/// **The document surfaces this crate does not consume are still unconsumed**, and still recorded.
///
/// The record is [`NOT_CONSUMED`], and this holds it to the code both ways: every entry carries a
/// reason, and none of them is named anywhere in this crate. A consumer appearing is not a failure
/// of the code — it is the moment the record stops being true, which is exactly when someone should
/// be made to rewrite it rather than leave a stale sentence behind.
#[test]
fn the_unconsumed_document_surfaces_are_still_unconsumed() {
    for (surface, reason) in NOT_CONSUMED {
        assert!(
            reason.len() > 40,
            "`{surface}` is recorded as not consumed with no reason worth reading",
        );
    }

    let consumed: Vec<String> = host_sources()
        .into_iter()
        .flat_map(|(path, source)| {
            let code = code_of(&source);
            NOT_CONSUMED
                .iter()
                .filter(|(surface, _)| {
                    // The leaf, so `quirks.pagination` is looked for as `pagination`: the document
                    // nests it, and a consumer would name the leaf.
                    let leaf = surface.rsplit('.').next().unwrap_or(surface);
                    names_identifier(&code, leaf)
                })
                .map(|(surface, _)| format!("`{path}` names `{surface}`"))
                .collect::<Vec<_>>()
        })
        .collect();

    assert!(
        consumed.is_empty(),
        "a document surface recorded as not consumed is now named in this crate:\n  {}\n\nThat is \
         not a bug — it is the record going stale. Decide whether the surface is genuinely \
         consumed, and either move it out of `NOT_CONSUMED` with a note saying where it is read, \
         or rename what collided with it.",
        consumed.join("\n  "),
    );
}

// ---------------------------------------------------------------------------------------------
// The self-test
// ---------------------------------------------------------------------------------------------

/// **The scanner catches what it claims to**, driven over fixtures rather than over the tree.
///
/// Each case is a source this rule set must reject or must accept, and the accept cases are the
/// ones that matter most: a rule that refused `DocumentRehearsal` for containing `Rehearsal`, or
/// refused the prose explaining the migration, would be red on the correct code and would be
/// "fixed" by weakening it.
#[test]
fn the_scanner_catches_what_it_claims_to() {
    let refused: &[(&str, &str)] = &[
        (
            "the emitted Flux, handed to a parse",
            "let r = Rehearsal::of(entry.id, provider.id, entry.service, entry.flux)?;",
        ),
        (
            "the qualified spelling of the same call",
            "connector_pack::Rehearsal::of(id, provider, service, text)",
        ),
        (
            "the flux field alone, without a rehearsal",
            "let text = operation.flux;",
        ),
    ];

    for (what, source) in refused {
        assert!(
            !violations(&[("settings.rs".to_owned(), (*source).to_owned())]).is_empty(),
            "the scanner accepted {what}: `{source}`",
        );
    }

    let accepted: &[(&str, &str)] = &[
        (
            "the document-backed replacement, which merely ends in the same letters",
            "let r = connector_pack::DocumentRehearsal::of(entry.id)?;",
        ),
        (
            "prose explaining what was retired and why",
            "// Rehearsal::of(.., entry.flux) parsed the emitted Flux; the document publishes it.",
        ),
        (
            "a doc comment naming both derivations",
            "/// Replaces `Rehearsal` with `DocumentRehearsal`; no `.flux` text is read.",
        ),
        (
            "the tenant-workflow parse, which is a different Flux entirely",
            "let ast = flux_lang::parse::parse(&version.source)?;",
        ),
    ];

    for (what, source) in accepted {
        let found = violations(&[("settings.rs".to_owned(), (*source).to_owned())]);
        assert!(
            found.is_empty(),
            "the scanner refused {what}: `{source}`\n{}",
            found.join("\n"),
        );
    }

    // The allow-list is a real bound rather than decoration: the same line is fine in `settings.rs`
    // and a violation anywhere else.
    let elsewhere = "let r = connector_pack::DocumentRehearsal::of(entry.id)?;";
    assert!(
        !violations(&[("invoke.rs".to_owned(), elsewhere.to_owned())]).is_empty(),
        "the scanner let a second module enter the pack's document-backed rehearsal",
    );
}
