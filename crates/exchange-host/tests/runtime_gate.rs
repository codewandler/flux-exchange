//! **Where the runtime gate sits in `Invoker::invoke`** (X-48) — an ordering check, and nothing
//! more than that.
//!
//! # Read this before trusting the file name
//!
//! An earlier version of this file said it asserted *"that the gate is **called**"*. It did not. It
//! asserted that a substring appeared, and a review demonstrated three mutations that left the gate
//! dead while every test here stayed green:
//!
//! ```text
//! let _ = admit_runtime(self.deployment, &ConnectorSurface::of(provider));       → 4 passed
//! if false { admit_runtime(self.deployment, &ConnectorSurface::of(provider))?; } → 4 passed
//! let _note = "the gate used to be admit_runtime(deployment, surface)";          → 4 passed
//! ```
//!
//! The third is the one that settles it: the marker was in a string literal and the guard could not
//! tell. **A check that reads text cannot tell a live call from a mention of one**, and no amount
//! of sharpening the marker changes that — it is the shape of the instrument, not a bug in this
//! instance of it.
//!
//! So the claim moved rather than being patched. `admit_runtime` now returns
//! [`Admitted`](exchange_host::Admitted), whose field is private, which has no public constructor,
//! and which is the only key to `connector_pack::resolve` from the dispatch path. All three
//! mutations above are **compile errors** now, and so is deleting the call outright. That property
//! is checked by `rustc` on every build, which is a stronger place for it than any test.
//!
//! # What is left for this file, and it is worth having
//!
//! The compiler holds *that* the gate runs. It does not hold **where**. `docs/designs/invoke.md`
//! §2 and §4 put the gate ahead of two specific steps, for reasons about consequences rather than
//! tidiness:
//!
//! - **before the credential port is bound** — a refusal that happens after a secret has been read
//!   has already moved that secret into this process's memory for a connector it was never going to
//!   run;
//! - **before dispatch** — afterwards, the connector this deployment refuses has already run.
//!
//! Moving the gate below either of those still compiles. This file is the only thing that notices.
//! That is its whole claim: **three markers, and the order they appear in, in the text of one
//! function.** It says nothing about liveness, nothing about the gate's answer, and nothing about
//! any other call site.
//!
//! # Why the refusal is not simply driven instead
//!
//! Because it cannot be, and the reason is measured rather than assumed. `admit_runtime`'s answer
//! is a function of two values — the bound `Deployment` and the runtime the catalogue declares —
//! and every `Provider` in the pinned `connector-catalog` declares `Runtime::Http`, which both
//! deployment classes admit. `invoke.rs`'s `the_whole_catalogue_declares_http` keeps that true
//! rather than remembered, and goes red the day it stops being. At that point a behavioural test
//! becomes possible and should replace this one.
//!
//! # If this test is red
//!
//! The gate moved relative to one of the two steps it is supposed to precede, or `invoke` was
//! restructured. Neither is a scanner problem: read `docs/designs/invoke.md` §4 and decide, then
//! update [`rules`] if the decision was to restructure.

use std::fs;
use std::path::{Path, PathBuf};

/// The markers, as data, so [`the_scanner_catches_what_it_claims_to`] can run the identical
/// function over bodies it **must** reject and a body it **must** accept.
///
/// A scanner that has not just proved it catches a violation is not evidence there are none — the
/// discipline `no_second_request_path.rs` and `console/test/components.test.mjs` are both held to.
/// It is necessary and it is not sufficient: the self-test proves the rules fire, never that the
/// rules mean what their names suggest. That second question is what the module documentation
/// answers, and getting it wrong is what sent this file back once already.
mod rules {
    /// The function whose body carries the ordering. Matched on its signature so a rename is a
    /// loud failure here rather than a silent pass.
    pub const INVOKE: &str = "async fn invoke_selected(";

    /// **Where the gate appears.** Not *that* it is called — `Admitted`'s private field holds that,
    /// and holds it at compile time. This is the anchor the two orderings below are measured from,
    /// and if it were really missing the crate would not build in the first place.
    pub const GATE: &str = "admit_runtime(";

    /// Binding the tenant's credential port. The gate goes in front of it because a refusal that
    /// happens *after* a secret has been read has already moved that secret into this process's
    /// memory for a connector it was never going to run.
    pub const BINDS_THE_CREDENTIAL_PORT: &str = "Credentials::new(";

    /// Dispatching the resolved operation. The gate goes in front of it for the blunter reason:
    /// afterwards, the connector this deployment refuses has already run.
    pub const DISPATCH: &str = ".execute(";
}

// ---------------------------------------------------------------------------------------------
// The rule, against the real source
// ---------------------------------------------------------------------------------------------

/// **The gate appears ahead of the credential port and dispatch in the shared execution path.**
#[test]
fn the_runtime_gate_is_ordered_ahead_of_the_credential_and_the_dispatch() {
    let source = fs::read_to_string(seam_file())
        .unwrap_or_else(|error| panic!("`{}` is readable: {error}", seam_file().display()));
    let body = body_of(&code_of(&source), rules::INVOKE);

    let found = violations(&body);
    assert!(found.is_empty(), "{}", found.join("\n"));
}

/// The extractor really did cut out one function rather than hand back the file.
///
/// Without this, a `body_of` that silently returned everything would make the rules above pass on
/// the strength of `admit_runtime`'s *definition* and its unit tests, which are in the same file —
/// and the ordering check would be measuring the wrong text.
#[test]
fn the_extractor_returns_one_function_and_not_the_file() {
    let source = fs::read_to_string(seam_file()).expect("the seam file is readable");
    let code = code_of(&source);
    let body = body_of(&code, rules::INVOKE);

    assert!(
        body.len() * 4 < code.len(),
        "the extracted body is {} of {} bytes, which is most of the file — the brace match is \
         broken and every rule above is being satisfied by some other function's text",
        body.len(),
        code.len(),
    );
    assert!(
        body.contains("principal.tenant()"),
        "the extracted body is not `invoke_selected`'s: {body}",
    );
    assert!(
        !body.contains("pub fn admit_runtime"),
        "the extracted body swallowed the gate's own definition, so `{}` would match it rather \
         than the call site",
        rules::GATE,
    );
}

/// **The scanner, proved against bodies it must reject and a body it must accept.**
///
/// Every fixture here is about *position*. There is deliberately no fixture for "the call is dead",
/// because this scanner does not catch that — pretending otherwise is how the previous version of
/// this file came to overstate itself. That case is held in `runtime.rs`, by a type.
#[test]
fn the_scanner_catches_what_it_claims_to() {
    // The shape this file exists to permit: gate first, then the port, then dispatch.
    let compliant = format!(
        "let entry = lookup(operation)?;\n\
         let admitted = {}self.deployment, &ConnectorSurface::of(provider))?;\n\
         let credentials = {}self.credentials.clone(), tenant)?;\n\
         let tool = admitted.resolve(entry, egress, credentials, settings)?;\n\
         let result = tool{}&ctx, params).await?;\n",
        rules::GATE,
        rules::BINDS_THE_CREDENTIAL_PORT,
        rules::DISPATCH,
    );
    assert!(
        violations(&compliant).is_empty(),
        "the scanner rejects the shape it exists to permit: {:?}",
        violations(&compliant),
    );

    // The gate after the credential port is bound: present, live, and too late to be the thing the
    // design's §2 says it is.
    let late = format!(
        "let credentials = {}self.credentials.clone(), tenant)?;\n\
         let admitted = {}self.deployment, &ConnectorSurface::of(provider))?;\n\
         let result = tool{}&ctx, params).await?;\n",
        rules::BINDS_THE_CREDENTIAL_PORT,
        rules::GATE,
        rules::DISPATCH,
    );
    assert!(
        !violations(&late).is_empty(),
        "the scanner accepted a gate that runs after the credential port is bound",
    );

    // The gate after dispatch: the connector this deployment refuses has already run.
    let after_dispatch = format!(
        "let credentials = {}self.credentials.clone(), tenant)?;\n\
         let result = tool{}&ctx, params).await?;\n\
         let admitted = {}self.deployment, &ConnectorSurface::of(provider))?;\n",
        rules::BINDS_THE_CREDENTIAL_PORT,
        rules::DISPATCH,
        rules::GATE,
    );
    assert!(
        !violations(&after_dispatch).is_empty(),
        "the scanner accepted a gate that runs after the request was dispatched",
    );

    // The anchor gone. The crate would not compile in that state — `admitted` would be unbound —
    // but the rule is kept, because a scanner whose anchor can silently vanish measures nothing.
    let anchorless = compliant.replace(rules::GATE, "renamed_away(");
    assert!(
        !violations(&anchorless).is_empty(),
        "the scanner accepted a body with no `{}` to order against",
        rules::GATE,
    );

    // Each ordering rule needs both of its endpoints present, or it passes vacuously the day the
    // step it orders against is renamed. That is the failure mode a scanner has: a marker that
    // stopped matching guards nothing while still reading as a guard.
    for missing in [rules::BINDS_THE_CREDENTIAL_PORT, rules::DISPATCH] {
        let without = compliant.replace(missing, "renamed_away(");
        assert!(
            !violations(&without).is_empty(),
            "the scanner accepted a body with no `{missing}`, so the rule ordering against it \
             passes on an empty premise",
        );
    }
}

/// A commented-out gate is not the anchor, and this file's own prose is not either.
///
/// The classification has to happen before the judgment — the same reason
/// `scripts/check-action-pins.sh` classifies a workflow line before deciding about it.
///
/// Note what this does **not** buy, since the previous version of this file drew the wrong
/// conclusion from it: classifying comments does not make the scanner a call graph. A marker
/// written inside a string literal still reads as code here, which is exactly how the third
/// mutation got past. The fix for that was a type, not a better classifier.
#[test]
fn a_gate_in_a_comment_is_not_the_anchor() {
    let commented = format!(
        "// we used to call {}self.deployment, &surface)?; here\n\
         let credentials = {}self.credentials.clone(), tenant)?;\n\
         let result = tool{}&ctx, params).await?;\n",
        rules::GATE,
        rules::BINDS_THE_CREDENTIAL_PORT,
        rules::DISPATCH,
    );

    assert!(
        !violations(&code_of(&commented)).is_empty(),
        "a commented-out gate satisfied the scanner",
    );
}

// ---------------------------------------------------------------------------------------------
// The scanner itself
// ---------------------------------------------------------------------------------------------

/// Every rule of [`rules`] applied to one function body, as human-readable violations.
///
/// A pure function over the body text, which is what lets the self-tests above drive it over
/// fixtures rather than over the source it is meant to guard.
fn violations(body: &str) -> Vec<String> {
    let mut found = Vec::new();

    let Some(gate) = body.find(rules::GATE) else {
        found.push(format!(
            "`{}` does not name `{}`, so there is nothing to order the two steps below against.\n\n\
             A missing gate is a compile error before it is a test failure — `Admitted` is the only \
             key to `connector_pack::resolve` and it has no constructor — so reaching this message \
             most likely means the call was renamed or the function restructured. Decide which, and \
             update `rules` if it was deliberate. See `docs/designs/invoke.md` §4.",
            rules::INVOKE,
            rules::GATE,
        ));
        return found;
    };

    for (marker, why) in [
        (
            rules::BINDS_THE_CREDENTIAL_PORT,
            "a refusal that happens after a secret has been read has already moved that secret \
             into this process's memory for a connector it was never going to run",
        ),
        (
            rules::DISPATCH,
            "afterwards, the connector this deployment refuses has already run",
        ),
    ] {
        match body.find(marker) {
            None => found.push(format!(
                "`{}` no longer names `{marker}`, so the rule ordering `{}` against it is \
                 asserting nothing. Update the marker in `rules` rather than dropping the rule.",
                rules::INVOKE,
                rules::GATE,
            )),
            Some(at) if at < gate => found.push(format!(
                "`{}` names `{marker}` before `{}`: {why}. See `docs/designs/invoke.md` §2 and §4 \
                 — that ordering is load-bearing, not stylistic.",
                rules::INVOKE,
                rules::GATE,
            )),
            Some(_) => {}
        }
    }

    found
}

/// The body of the function whose signature contains `signature`, by brace matching.
///
/// Panics rather than returning an `Option`: a signature this cannot find means the function was
/// renamed or restructured, and the one outcome a guard must never have is passing quietly because
/// it could not find what it was looking at.
///
/// Brace matching over whole text is naive — a brace inside a string literal would fool it — and
/// that is acceptable here for the reason [`code_of`] is: the only way it can go wrong is by
/// cutting the body short or long, and [`the_extractor_returns_one_function_and_not_the_file`]
/// checks both ends against facts about the real function.
fn body_of(code: &str, signature: &str) -> String {
    let at = code
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` is in the source; it was renamed or removed"));
    let opens = code[at..]
        .find('{')
        .unwrap_or_else(|| panic!("`{signature}` has a body"))
        + at;

    let mut depth = 0usize;
    for (offset, character) in code[opens..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return code[opens + 1..opens + offset].to_owned();
                }
            }
            _ => {}
        }
    }

    panic!("`{signature}`'s body is not brace-balanced");
}

/// `source` with whole-line comments removed.
///
/// Every rule above is about where a marker sits in the code, and this file's own documentation
/// names all three markers. Whole-line only, and deliberately so — the same trade
/// `no_second_request_path.rs` records: stripping a trailing `//` correctly means knowing whether
/// the slashes are inside a string literal, and an over-eager strip creates blind spots.
///
/// It does **not** strip string literals, and that is a stated limit rather than an oversight: a
/// marker written inside a `&str` reads as code to this function. Nothing here rests on that any
/// more — see the module documentation — but the next person to add a rule should know it before
/// deciding what the rule proves.
fn code_of(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The one file that dispatches an operation.
fn seam_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/invoke.rs")
}
