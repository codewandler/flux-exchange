//! **`Invoker::invoke` consults the runtime gate, and does so before the credential** (X-48).
//!
//! X-12 put `admit_runtime` in the right place and nothing held it there. Every invoke-level test
//! drives an `http` connector and every refusal test calls [`admit_runtime`] directly, so **the
//! `admit_runtime(…)?` line could be deleted from `Invoker::invoke` and the whole workspace stayed
//! green** — 326 tests, none of them about the gate. This file is the tripwire that was missing.
//!
//! # Why this reads the source rather than driving the refusal
//!
//! Because the refusal is not drivable, and the reason is measured rather than assumed.
//! `admit_runtime`'s answer is a function of exactly two values: the [`Deployment`] a composition
//! bound, and the runtime the *catalogue* declares for the operation's provider. **Every connector
//! in the compiled-in catalogue declares `http`**, which both deployment classes admit — a fact
//! `invoke.rs`'s own `the_whole_catalogue_declares_http` keeps true rather than remembered. So no
//! value a test can reach through `Invoker::invoke`'s parameters makes the gate answer anything but
//! `Ok`, and a behavioural test of "the gate refused" would need a catalogue entry that does not
//! exist. `docs/designs/invoke.md` §4 records this as a measured gap and says it makes the path
//! *more* worth pinning, not less.
//!
//! # What this proves, and what it does not
//!
//! Stated plainly, because a guard that overstates its reach is worse than one that admits its
//! edge. This reads the text of one function and asserts three things about it: that the gate is
//! **called**, and that the call comes **before** the two steps the design's §2 puts it in front of.
//! That is a claim about source order, not about behaviour.
//!
//! It cannot see whether the gate's *answer* is correct — [`admit_runtime`]'s own tests in
//! `invoke.rs` and `tests/invoke.rs` do that, driven against a constructed `ConnectorSurface`. It
//! cannot see a gate defeated from inside `admit_runtime`, nor an `Invoker` composed with the wrong
//! [`Deployment`]. It is a presence-and-order check on the one call site nothing else can observe,
//! and it is worth exactly that.
//!
//! # If this test is red
//!
//! The gate moved, was renamed, or was removed. Renaming it means updating [`rules`]; removing it
//! means a deployment now runs a locally-executing runtime for many tenants in one process, which
//! `AGENTS.md` lists as an invariant and `docs/vision.md` argues for. Deleting this file is the
//! same class of change as deleting `no_second_request_path.rs`, and that is a blocker.

use std::fs;
use std::path::{Path, PathBuf};

/// The markers, as data, so [`the_scanner_catches_what_it_claims_to`] can run the identical
/// function over bodies it **must** reject and a body it **must** accept.
///
/// A scanner that has not just proved it catches a violation is not evidence there are none — the
/// discipline `no_second_request_path.rs` and `console/test/components.test.mjs` are both held to.
mod rules {
    /// The function whose body carries the ordering. Matched on its signature so a rename is a
    /// loud failure here rather than a silent pass.
    pub const INVOKE: &str = "pub async fn invoke(";

    /// **The gate.** `docs/designs/invoke.md` §4: this deployment will not serve the connector's
    /// declared runtime, ever, for anyone.
    pub const GATE: &str = "admit_runtime(";

    /// Binding the tenant's credential port. The gate goes in front of it because a refusal that
    /// happens *after* a secret has been read has already moved that secret into this process's
    /// memory for a connector it was never going to run.
    pub const BINDS_THE_CREDENTIAL_PORT: &str = "Credentials::new(";

    /// The dispatch seam — where the pack resolves the credential and builds the request. The gate
    /// goes in front of it because there is no reason to project a connector this deployment will
    /// not execute.
    pub const SEAM: &str = "connector_pack::resolve(";
}

// ---------------------------------------------------------------------------------------------
// The rule, against the real source
// ---------------------------------------------------------------------------------------------

/// **`Invoker::invoke` calls the runtime gate, before it binds a credential port and before it
/// dispatches.**
#[test]
fn invoke_consults_the_runtime_gate_before_it_touches_a_credential() {
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
/// and the tripwire would be guarding nothing.
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
        "the extracted body is not `invoke`'s: {body}",
    );
    assert!(
        !body.contains("pub fn admit_runtime"),
        "the extracted body swallowed the gate's own definition, so `{}` would match it rather \
         than a call",
        rules::GATE,
    );
}

/// **The scanner, proved against bodies it must reject and a body it must accept.**
#[test]
fn the_scanner_catches_what_it_claims_to() {
    // The shape this file exists to permit: gate first, then the ports, then the seam.
    let compliant = format!(
        "let entry = lookup(operation)?;\n\
         {}self.deployment, &ConnectorSurface::of(provider))?;\n\
         let credentials = {}self.credentials.clone(), tenant)?;\n\
         let tool = {}entry, egress, credentials, settings)?;\n",
        rules::GATE,
        rules::BINDS_THE_CREDENTIAL_PORT,
        rules::SEAM,
    );
    assert!(
        violations(&compliant).is_empty(),
        "the scanner rejects the shape it exists to permit: {:?}",
        violations(&compliant),
    );

    // **The finding, as a fixture.** This is X-12's hole exactly: the line is gone and everything
    // else is untouched.
    let ungated = compliant.replace(
        &format!(
            "{}self.deployment, &ConnectorSurface::of(provider))?;\n",
            rules::GATE
        ),
        "",
    );
    assert!(
        !violations(&ungated).is_empty(),
        "the scanner accepted an `invoke` with no runtime gate, which is the one thing it is for",
    );

    // The gate after the credential port is bound: present, and too late to be the thing the
    // design's §2 says it is.
    let late = format!(
        "let credentials = {}self.credentials.clone(), tenant)?;\n\
         {}self.deployment, &ConnectorSurface::of(provider))?;\n\
         let tool = {}entry, egress, credentials, settings)?;\n",
        rules::BINDS_THE_CREDENTIAL_PORT,
        rules::GATE,
        rules::SEAM,
    );
    assert!(
        !violations(&late).is_empty(),
        "the scanner accepted a gate that runs after the credential port is bound",
    );

    // The gate after dispatch: the connector this deployment refuses has already run.
    let after_dispatch = format!(
        "let credentials = {}self.credentials.clone(), tenant)?;\n\
         let tool = {}entry, egress, credentials, settings)?;\n\
         {}self.deployment, &ConnectorSurface::of(provider))?;\n",
        rules::BINDS_THE_CREDENTIAL_PORT,
        rules::SEAM,
        rules::GATE,
    );
    assert!(
        !violations(&after_dispatch).is_empty(),
        "the scanner accepted a gate that runs after the request was dispatched",
    );

    // Each ordering rule needs both of its endpoints present, or it passes vacuously the day the
    // step it orders against is renamed. This is the failure mode a scanner has: a marker that
    // stopped matching guards nothing while still reading as a guard.
    for missing in [rules::BINDS_THE_CREDENTIAL_PORT, rules::SEAM] {
        let without = compliant.replace(missing, "renamed_away(");
        assert!(
            !violations(&without).is_empty(),
            "the scanner accepted a body with no `{missing}`, so the rule ordering against it \
             passes on an empty premise",
        );
    }
}

/// A commented-out gate is not a gate, and this file's own prose is not one either.
///
/// The classification has to happen before the judgment — the same reason
/// `scripts/check-action-pins.sh` classifies a workflow line before deciding about it. Without it,
/// somebody could satisfy this test by writing the call in a comment.
#[test]
fn a_gate_in_a_comment_does_not_count() {
    let commented = format!(
        "// we used to call {}self.deployment, &surface)?; here\n\
         let credentials = {}self.credentials.clone(), tenant)?;\n\
         let tool = {}entry, egress, credentials, settings)?;\n",
        rules::GATE,
        rules::BINDS_THE_CREDENTIAL_PORT,
        rules::SEAM,
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
            "`{}` does not call `{}`.\n\n\
             The deployment gate is what keeps a locally-executing runtime out of a process that \
             serves many tenants — `AGENTS.md` lists it as an invariant and there is deliberately \
             no override. It cannot be reached by any value a test can supply, because every \
             connector in the compiled-in catalogue declares `http`, so this ordering check is the \
             only thing that holds the call site. See `docs/designs/invoke.md` §4.",
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
            rules::SEAM,
            "there is no reason to project, or dispatch, a connector this deployment will not \
             execute",
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
                "`{}` calls `{marker}` before `{}`: {why}. See `docs/designs/invoke.md` §2 and §4 \
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
/// renamed or restructured, and the one outcome a tripwire must never have is passing quietly
/// because it could not find what it was looking at.
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
/// Every rule above is about what the code *does*, and this file's own documentation names the
/// markers it looks for. Whole-line only, and deliberately so — the same trade
/// `no_second_request_path.rs` records: stripping a trailing `//` correctly means knowing whether
/// the slashes are inside a string literal, and an over-eager strip creates blind spots. This one
/// can only produce a false *positive*, which is the direction a guard should fail in.
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
