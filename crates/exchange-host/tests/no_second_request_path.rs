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
//! - **Lock 1** — the dispatching crate's own **normal** dependency tables are an **allow-list**.
//!   Not a deny-list: a deny-list only catches the transports somebody thought of, and passes for
//!   `ureq`, `isahc`, `attohttpc` and whatever ships next year. An allow-list fails on any new
//!   dependency *in the tables it reads* — see [`header_of`] for which those are and why the rest
//!   are out — and whoever adds one has to write down why it is not a transport.
//! - **Lock 2** — one seam, counted, over this crate's sources.
//!
//! # What lock 2 is, and what it is not
//!
//! Stated here rather than discovered later, because a guard that overstates its reach is worse
//! than one that admits its edge — this repository has had to correct exactly that more than once.
//!
//! **Lock 2 checks names, not values.** Every rule below is a string, and it refuses a source that
//! writes that string. It cannot see a capability that arrives under a name nobody listed, and a
//! previous attempt at this section got that wrong twice in the same paragraph, so the corrections
//! are recorded rather than quietly applied.
//!
//! The demonstration, which is worth more than the argument. X-48's review added one file to this
//! crate:
//!
//! ```ignore
//! let handle = ctx.workspace_context().active();
//! handle.run(&argv, Duration::from_secs(5)).await
//! ```
//!
//! That reaches `System::run` — a process spawn — naming nothing in [`rules::FORBIDDEN`], no
//! `.tool()`, and not even [`rules::REACHES_THE_SYSTEM`], which was written believing `.system(`
//! was the only spelling. It is not: `ToolContext::workspace_context` is public, and
//! `WorkspaceContext::active` returns the same `Arc<System>` under a different name. The whole
//! workspace stayed green. **A rule that chases accessor spellings will always be one accessor
//! behind**, because the set of ways to get a value out of a public type is not a set this file
//! can enumerate.
//!
//! So the instrument changed rather than the string list growing. [`rules::HOLDS_A_TOOL_CONTEXT`]
//! bounds **possession** instead of use: a `ToolContext` is the handle every IO capability hangs
//! off, and only the seam and the crate root may hold one — the same two files that may hold an
//! `Egress`, and the same shape of rule. A file that cannot name the handle cannot call anything on
//! it, whatever the accessor is called this release. [`rules::REACHES_THE_SYSTEM`] stays underneath
//! it as a second, narrower net.
//!
//! That is a real narrowing and it is still a name check. What it does not catch: a `ToolContext`
//! obtained without naming the type — which today requires a public accessor for `Invoker`'s
//! private `contexts` field, and there is none. If one is ever added, this rule is back to being
//! one accessor behind.
//!
//! So what covers the rest, in the order it bites:
//!
//! - **Lock 1**, above, is not a name check — it fails on a *name it has never heard of*. Its scope
//!   is this crate's normal dependency tables: `[dependencies]`, `[dependencies.name]`, and both
//!   under `[target.…]`. That list is narrower than the "any entry" this section used to claim: the
//!   review appended `[dependencies.reqwest]` and lock 1 reported `5 passed`, because the parser
//!   matched one spelling of the header. [`the_manifest_parser_reads_every_shape_cargo_allows`] is
//!   the test that was missing, and `dev-`/`build-dependencies` remain deliberately out of scope
//!   with reasons on [`header_of`]. Its standing blind spot is the mirror of lock 2's: a capability
//!   reached *transitively*, through a crate already on the list — which is exactly how
//!   `flux-system` is reachable at all.
//! - **Lock 3** (`tests/invoke.rs`) is behavioural: a counting transport, one dispatch per invoke,
//!   and zero for every refusal. It proves things about the paths its tests drive and nothing about
//!   paths nobody wrote a test for.
//!
//! Three mechanisms, and they fail differently. None of them is the argument on its own, and all
//! three are inside the crate that ships — which was a claim worth checking rather than assuming,
//! and is the next section.
//!
//! # Where the locks stop (X-55)
//!
//! **The locks bound the published crate, not the deployed binary.** `exchange-host` is what
//! `cargo publish` uploads and what a consumer composes into a process of its own;
//! `exchange-server` is `publish = false`, one composition of this library among the ones nobody
//! here writes. So lock 1 reads this crate's manifest, lock 2 walks [`claim::SCANNED`] — the
//! boundary as one value rather than as a path typed at a call site — and `exchange-server` gets
//! exactly one rule: [`the_crate_that_holds_a_transport_never_names_the_pack`].
//!
//! What that costs is written down rather than left to be found. **A second request path added to
//! `exchange-server` is caught by nothing here**: that crate holds `flux_web` and binds the
//! credential store, so it could build one, and the single rule reaching it bounds *naming the
//! pack* rather than *building a request*. The scan does not follow, because holding a transport is
//! what a composition is *for* — point lock 2 at that crate and every rule goes red on correct
//! code, and the per-file exception list which answers that is the "one more file on the list"
//! drift these locks exist to prevent. The argument in full, with the alternative and why it was
//! not taken, is in `docs/designs/invoke.md` under "Where the locks stop".
//!
//! The consequence for prose is the half this file has to carry. A control outside the boundary may
//! be **described** and may not be **leaned on**. `exchange-server`'s `execution::guarded_system`
//! builds its `System` with `SandboxMode::Require`, so `System::run`/`run_with_env` are confined or
//! refuse — a true and useful fact about *this repository's deployment*, and not a fact about
//! `codewandler-flux-exchange-host`, since a downstream binary implements `Contexts` itself and
//! supplies whatever `System` it built, quite possibly `System::new`, whose sandbox is disabled.
//! It is narrower than "a spawn" too: `build_command` consults `Sandbox::ensure_available` only for
//! `Confinement::Sandboxed`, so the `Exempt` paths (`run_exempt`, `spawn_debug_pipe`) skip it.
//! [`no_document_claims_more_than_the_locks_reach`] is what keeps the three documents that say any
//! of this from saying more.
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

    /// **The pack's third entry point** (X-47). `Rehearsal` parses an operation's Flux and reports
    /// what it would need and what request it would build.
    ///
    /// It is **not** a bypass today and is not being treated as one: it takes no `Egress`, holds no
    /// transport and has no `execute`, so nothing reached through it can dispatch. It is counted
    /// because it is a *pack entry point this file did not know about* — [`SEAM`] and
    /// [`MODEL_FACING_SEAM`] were the whole list, and a third one appearing without the scanner
    /// noticing is exactly the erosion this file exists to make impossible. If upstream ever gives
    /// it a transport, the failure should be a red test here rather than a discovery in production.
    pub const REHEARSAL: &str = "connector_pack::Rehearsal";

    /// Files allowed to name [`REHEARSAL`]: the one that derives a connector's configuration
    /// surface from its operations' own Flux.
    pub const MAY_NAME_REHEARSAL: &[&str] = &["settings.rs"];

    /// Dispatching a tool. Only the seam may, and only on the operation the pack resolved.
    pub const DISPATCH: &str = ".execute(";

    /// **Holding the handle every IO capability hangs off** (X-48, round 2).
    ///
    /// `ToolContext` is what a tool is called with, and upstream builds it over a
    /// `flux_system::System`. Everything reachable from one — process spawn, the workspace
    /// filesystem, the worktree ops — is reachable *through some accessor or other*, and this file
    /// cannot enumerate accessors: the round-1 rule below picked `.system(` and the review walked
    /// past it with `ctx.workspace_context().active()`, which is a different public accessor
    /// returning the same `Arc<System>`.
    ///
    /// So this rule bounds **possession** rather than use. A file that never names `ToolContext`
    /// cannot take one as a parameter, store one, or return one, so it has nothing to call an
    /// accessor on — and that holds however many accessors upstream adds. It is the same shape as
    /// [`MAY_NAME_EGRESS`]: a capability that travels to a bounded, readable set of places.
    ///
    /// Still a name check, and the residual is named rather than left to be found: a context
    /// obtained purely by inference, without the type appearing. That needs a public accessor for
    /// `Invoker`'s private `contexts` field, and there is none — add one and this rule is back to
    /// being one accessor behind.
    pub const HOLDS_A_TOOL_CONTEXT: &str = "ToolContext";

    /// Files allowed to name [`HOLDS_A_TOOL_CONTEXT`]: the seam that dispatches with one, and the
    /// crate root that re-exports the type so a composition can implement `Contexts` without
    /// naming `flux-runtime` and guessing an engine version. The same two files as
    /// [`MAY_NAME_EGRESS`], which is not a coincidence — they are the two halves of one invocation.
    pub const MAY_HOLD_A_TOOL_CONTEXT: &[&str] = &["invoke.rs", "lib.rs"];

    /// **The narrower net under [`HOLDS_A_TOOL_CONTEXT`]**: the shortest spelling of "give me the
    /// guarded `System`".
    ///
    /// Kept, and deliberately not trusted. It catches `ctx.system()` and `WorkspaceContext`'s own
    /// accessor if it is ever spelled that way; it did **not** catch `.workspace_context().active()`,
    /// which is how round 1's claim that this "closes the door" was disproved. Read it as the
    /// spelling somebody would reach for by accident, caught cheaply — not as a boundary.
    pub const REACHES_THE_SYSTEM: &str = ".system(";

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

/// **Where the locks reach, and what no document may say they reach** (X-55).
///
/// The rules above are about sources. These are about *claims*, and they exist because the failure
/// this repository keeps having to correct is not a weak lock — it is a document describing a
/// stronger one than the code enforces.
mod claim {
    /// **Lock 2's scan boundary, as a value rather than as a literal at a call site.**
    ///
    /// The real scan walks this and [`crate::the_locks_bound_the_published_crate`] asserts what it
    /// is, so widening the boundary is one edit that fails one test with the argument attached,
    /// rather than a quiet change to a path string.
    pub const SCANNED: &str = "crates/exchange-host/src";

    /// The prefix a scanned path must have: the crate that is actually published.
    pub const THE_PUBLISHED_CRATE: &str = "crates/exchange-host/";

    /// **The sentence X-55 settled the boundary question with**, carried verbatim by every document
    /// in [`DOCUMENTS`].
    ///
    /// One sentence rather than a list of accepted paraphrases, and that is the whole reason it
    /// works: a paraphrase is where a claim drifts back to the wider one it was narrowed from, and
    /// a document that has to carry this exact string is a document somebody read before changing.
    pub const THE_DECISION: &str = "The locks bound the published crate, not the deployed binary.";

    /// The documents that say what the locks reach, and therefore the ones that can overstate it.
    ///
    /// `docs/vision.md` is deliberately out: it states the *principle* — this host constructs no
    /// request of its own — and a principle is not a claim about enforcement. What drifts is the
    /// sentence next to the principle that says which mechanism holds it, and that sentence lives
    /// in these three.
    pub const DOCUMENTS: &[&str] = &[
        "AGENTS.md",
        "docs/designs/invoke.md",
        "crates/exchange-host/tests/no_second_request_path.rs",
    ];

    /// **Controls this repository really has, which live outside [`SCANNED`].**
    ///
    /// Naming one is not a fault — the composition's posture is real and worth describing. Reading
    /// it as though it held something up inside the published crate is, because for a consumer of
    /// `codewandler-flux-exchange-host` it is not there at all: a downstream binary implements
    /// `Contexts` itself and supplies whatever `System` it built.
    pub const OUTSIDE_THE_BOUNDARY: &[&str] = &["guarded_system", "SandboxMode::Require"];

    /// **Vocabulary that turns naming one of those into leaning on it.**
    ///
    /// A cheap net and openly one: the ways to overstate a guarantee in English are not a set this
    /// file can enumerate, exactly as [`crate::rules::REACHES_THE_SYSTEM`] cannot enumerate the
    /// accessors that reach a `System`. What actually carries the claim is [`THE_DECISION`], which
    /// a reader of any of these three documents meets; this list only catches the phrasing
    /// somebody reaches for by accident.
    ///
    /// It refuses a sentence that *denies* coverage too — "this is not a backstop" — and that is
    /// intended rather than tolerated. A passage that has to say so is still organised around the
    /// idea that it might be; describe where the control lives and stop.
    pub const CLAIMS_TO_COVER: &[&str] = &["backstop", "compensat", "makes up for"];
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

/// **The parser, proved over every shape Cargo allows — the check lock 1 never had.**
///
/// X-48's review appended six lines to this crate's manifest:
///
/// ```toml
/// [dependencies.reqwest]
/// version = "0.13"
/// features = ["json"]
/// ```
///
/// and lock 1 reported `5 passed`. An HTTP client had become a direct dependency of the crate whose
/// entire safety argument is that it holds no transport, and the allow-list said nothing — because
/// [`dependencies_of`] tested `line == "[dependencies]"` and a per-dependency table is not that
/// string.
///
/// The lesson is not "fix the parser". It is that **the parser had no test**, so nothing measured
/// the distance between what it read and what Cargo accepts. The rules of lock 2 have had a
/// self-test since X-12 for exactly this reason; lock 1's parser is the older half and never got
/// one. This is it, and it is driven in both directions — a fixture whose every entry must be
/// found, and a fixture whose every entry must be ignored — because a parser that returned
/// everything would pass a one-directional test while making the allow-list refuse the world.
#[test]
fn the_manifest_parser_reads_every_shape_cargo_allows() {
    let declares = "\
[package]
name = \"whatever\"

[dependencies]
serde.workspace = true
thiserror = \"1\"
tokio = { version = \"1\", features = [\"rt\"] }

[dependencies.reqwest]
version = \"0.13\"
features = [\"json\"]

[dependencies.\"quoted-name\"]
version = \"2\"

[target.'cfg(unix)'.dependencies]
nix = \"0.29\"

[target.'cfg(unix)'.dependencies.libc]
version = \"0.2\"
";

    let found = dependencies_of(declares);
    for expected in [
        "serde",
        "thiserror",
        "tokio",
        "reqwest",
        "quoted-name",
        "nix",
        "libc",
    ] {
        assert!(
            found.iter().any(|name| name == expected),
            "`{expected}` is a normal dependency in this manifest and the parser did not see it, \
             so the allow-list would pass while it sat in a consumer's graph. Found: {found:?}",
        );
    }

    // The other direction. `version`/`features` are a per-dependency table's *keys*, and a parser
    // that reported them would make the allow-list fail on noise until somebody added them to
    // `ALLOWED` — which is how a guard gets weakened by the person maintaining it.
    let ignores = "\
[package]
name = \"whatever\"
description = \"not a dependency\"

[lib]
name = \"whatever\"

[features]
default = []

[dev-dependencies]
criterion = \"0.5\"

[dev-dependencies.proptest]
version = \"1\"

[target.'cfg(unix)'.dev-dependencies]
tempfile = \"3\"

[build-dependencies]
cc = \"1\"
";

    let found = dependencies_of(ignores);
    assert!(
        found.is_empty(),
        "the parser reported {found:?} from a manifest with no normal dependencies at all; \
         `dev-`/`build-dependencies` and every non-dependency table are out of scope on purpose \
         — see `header_of`",
    );

    // And the keys under a per-dependency table are not themselves dependencies.
    let entry_keys = dependencies_of("[dependencies.reqwest]\nversion = \"0.13\"\n");
    assert_eq!(
        entry_keys,
        vec!["reqwest".to_owned()],
        "a per-dependency table names one dependency; its keys are not more of them",
    );
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
    let sources = sources_under(&workspace_path(claim::SCANNED));

    assert!(
        sources.len() > 3,
        "only {} source files were found under `{}`; the walk is broken",
        sources.len(),
        claim::SCANNED,
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
        // The one `FORBIDDEN` could not see: a `System` reached through the re-exported
        // `ToolContext`, naming nothing on the crate-name list.
        (
            "the guarded system behind a tool context",
            "let out = ctx.system().run(&argv, timeout).await?;",
        ),
        // **The review's proof-of-concept, verbatim in shape.** This is the one that walked past
        // round 1's `.system(` rule: a different public accessor, the same `Arc<System>`, a real
        // process spawn. It is caught now by possession — the file has to name `ToolContext` to
        // receive one — and it is kept here as the case that decides whether that rule is alive.
        (
            "a spawn reached through a second accessor",
            "async fn ping(ctx: &ToolContext) { ctx.workspace_context().active().run(&argv, t); }",
        ),
    ];

    for (what, line) in must_reject {
        let found = violations(&[seam(), ("second.rs".to_owned(), (*line).to_owned())]);
        assert!(
            !found.is_empty(),
            "the scanner accepted {what} (`{line}`), so the rule that should catch it is dead",
        );
    }

    // Possession, driven in both directions — a rule that rejected the seam's own context would be
    // one somebody deletes rather than one that guards anything.
    let holding = |name: &str| {
        (
            name.to_owned(),
            format!(
                "fn fresh(&self) -> {} {{ todo!() }}",
                rules::HOLDS_A_TOOL_CONTEXT
            ),
        )
    };
    assert!(
        violations(&[seam(), holding("lib.rs")]).is_empty(),
        "the scanner rejects the crate root re-exporting the context type a composition needs",
    );
    assert!(
        !violations(&[seam(), holding("helper.rs")]).is_empty(),
        "the scanner accepted a third file holding a `{}`, which is the handle every guarded IO \
         capability hangs off",
        rules::HOLDS_A_TOOL_CONTEXT,
    );

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

    // The pack's third entry point, outside the one file that derives a configuration surface with
    // it. Driven in both directions, because a rule that rejected `settings.rs` too would be one
    // somebody deletes rather than one that guards anything.
    let rehearsing = |name: &str| {
        (
            name.to_owned(),
            format!(
                "let r = {}::of(id, provider, service, flux)?;",
                rules::REHEARSAL
            ),
        )
    };
    assert!(
        violations(&[seam(), rehearsing("settings.rs")]).is_empty(),
        "the scanner rejects the one file that may derive a configuration surface",
    );
    assert!(
        !violations(&[seam(), rehearsing("elsewhere.rs")]).is_empty(),
        "the scanner accepted the pack's third entry point in a file that may not name it",
    );
}

// ---------------------------------------------------------------------------------------------
// The boundary — the locks bound the published crate, and no document may say otherwise (X-55)
// ---------------------------------------------------------------------------------------------

/// **The scan boundary is inside the published crate, and that is the decision — not an accident
/// of which path somebody typed.**
///
/// X-55 settled it: `exchange-host` is what ships, `exchange-server` is `publish = false`, and a
/// consumer of `codewandler-flux-exchange-host` gets locks 1–2 and nothing else. Widening
/// [`claim::SCANNED`] to the composing crate is a legitimate change to make — it is the other half
/// of the question — but it changes what the locks *mean*, so it fails here first and the failure
/// says where the argument has to be rewritten.
#[test]
fn the_locks_bound_the_published_crate() {
    assert!(
        claim::SCANNED.starts_with(claim::THE_PUBLISHED_CRATE),
        "lock 2 now scans `{}`, which is outside `{}`.\n\n\
         That is a change of claim, not of coverage. The locks are about the crate this repository \
         publishes; a scan that reaches the composing binary makes them about a deployment, and \
         `exchange-server` legitimately holds a transport, so every rule in `rules` would need an \
         exception list with an entry per file — the drift these locks exist to prevent.\n\n\
         If that is the trade you mean to make, make it in `docs/designs/invoke.md` first and \
         change `claim::THE_DECISION` with it.",
        claim::SCANNED,
        claim::THE_PUBLISHED_CRATE,
    );
}

/// **No document claims more than the locks reach.**
///
/// The other half of X-55, and the half that is easy to skip: deciding the boundary is worth
/// nothing if a document goes on describing the wider one. Every document in [`claim::DOCUMENTS`]
/// carries [`claim::THE_DECISION`] verbatim, and none of them leans on a control that lives outside
/// the boundary to hold something up inside it.
#[test]
fn no_document_claims_more_than_the_locks_reach() {
    let documents: Vec<(String, String)> = claim::DOCUMENTS
        .iter()
        .map(|relative| {
            let path = workspace_path(relative);
            let text = fs::read_to_string(&path).unwrap_or_else(|error| {
                panic!(
                    "`{relative}` states what the locks reach and must be readable: {error} \
                     (looked at `{}`)",
                    path.display()
                )
            });
            ((*relative).to_owned(), text)
        })
        .collect();

    assert_eq!(
        documents.len(),
        claim::DOCUMENTS.len(),
        "not every document was read, so this test is asserting less than it says",
    );

    let found = overclaims(&documents);
    assert!(found.is_empty(), "{}", found.join("\n\n"));
}

/// **The claim scanner, proved against prose it must reject and prose it must accept.**
///
/// Same treatment as [`the_scanner_catches_what_it_claims_to`], for the same reason: a rule whose
/// marker stopped matching would pass every document forever while reading as though it guarded
/// something.
#[test]
fn the_claim_scanner_catches_what_it_claims_to() {
    let compliant = |name: &str| {
        (
            name.to_owned(),
            format!("Where the locks stop.\n\n{}\n", claim::THE_DECISION),
        )
    };

    assert!(
        overclaims(&[compliant("docs/designs/invoke.md")]).is_empty(),
        "the scanner rejects the shape it exists to permit",
    );

    // The decision wrapped at column 100, which is how it actually appears in all three documents.
    // A raw `contains` would refuse every one of them.
    let (head, tail) = claim::THE_DECISION.split_at(20);
    let wrapped = (
        "AGENTS.md".to_owned(),
        format!("- **This host constructs no request of its own.**\n\n{head}\n {tail}\n"),
    );
    assert!(
        overclaims(&[wrapped]).is_empty(),
        "the scanner refuses a document that states the decision across a line break, which is \
         every document that states it",
    );

    // A document that never states the decision is the drift this rule exists to catch: the
    // boundary was settled once, and a document that does not carry it is one that never met it.
    assert!(
        !overclaims(&[(
            "AGENTS.md".to_owned(),
            "The locks are structural.".to_owned()
        )])
        .is_empty(),
        "the scanner accepted a document that never states the boundary",
    );

    // A control from outside the boundary, presented as holding something up.
    let leaning = (
        "docs/designs/invoke.md".to_owned(),
        format!(
            "{}\n\nThe posture in `guarded_system` is the backstop for what lock 2 cannot see.\n",
            claim::THE_DECISION
        ),
    );
    assert!(
        !overclaims(&[leaning]).is_empty(),
        "the scanner accepted a composition-level control presented as covering a crate-level gap",
    );

    // And the same control, merely described. Naming it is the honest thing to do; the rule is
    // about what the sentence claims for it.
    let describing = (
        "docs/designs/invoke.md".to_owned(),
        format!(
            "{}\n\n`exchange-server`'s `guarded_system` builds its `System` with \
             `SandboxMode::Require`. A downstream binary supplies its own.\n",
            claim::THE_DECISION
        ),
    );
    assert!(
        overclaims(&[describing]).is_empty(),
        "the scanner rejects a document for describing where a control lives, which is the thing \
         it wants documents to do",
    );

    // Prose only, and this direction is load-bearing rather than tidy: `claim`'s own tables hold
    // both a control name and a coverage word as *code*, three lines apart. A scanner that read
    // code would refuse this file for declaring the rule it enforces.
    let tables = (
        "crates/exchange-host/tests/no_second_request_path.rs".to_owned(),
        format!(
            "//! {}\nconst OUTSIDE: &[&str] = &[\"guarded_system\"];\n\
             const COVER: &[&str] = &[\"backstop\"];\n",
            claim::THE_DECISION
        ),
    );
    assert!(
        overclaims(&[tables]).is_empty(),
        "the scanner read Rust code as a claim, so this file cannot name the strings it refuses",
    );

    // The same two strings in one doc comment of the same file, which *is* a claim.
    let doc_comment = (
        "crates/exchange-host/tests/no_second_request_path.rs".to_owned(),
        format!(
            "//! {}\n//!\n//! `guarded_system` is the backstop under lock 2.\n",
            claim::THE_DECISION
        ),
    );
    assert!(
        !overclaims(&[doc_comment]).is_empty(),
        "the scanner accepted a doc comment leaning on a control from outside the boundary",
    );

    // The residual, driven so it is a recorded limit rather than a discovered one: the rule is
    // per paragraph, so a control named in one paragraph and leaned on two paragraphs later is
    // not caught. Widening it to whole documents would refuse `docs/designs/invoke.md` for
    // discussing lock 3 and the composition in the same file, which is most of what it is for.
    let far_apart = (
        "docs/designs/invoke.md".to_owned(),
        format!(
            "{}\n\n`guarded_system` sets the posture.\n\nIt is the backstop under lock 2.\n",
            claim::THE_DECISION
        ),
    );
    assert!(
        overclaims(&[far_apart]).is_empty(),
        "this is the rule's known limit and the assertion records it; if it now fails, the rule \
         got stronger and this comment is what needs rewriting",
    );
}

/// Every claim rule applied to `documents`, as human-readable violations.
///
/// A pure function over `(path, text)` pairs, so [`the_claim_scanner_catches_what_it_claims_to`]
/// can drive the identical code over fixtures rather than over the documents it guards.
fn overclaims(documents: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();

    for (path, text) in documents {
        // Paragraphs rather than raw text, and for the sentence check too: every document here
        // wraps at column 100, so the decision arrives split across two lines and a `contains` over
        // the raw text would refuse a document that carries it perfectly well.
        let paragraphs = paragraphs(&prose_of(path, text));

        if !paragraphs
            .iter()
            .any(|paragraph| paragraph.contains(claim::THE_DECISION))
        {
            found.push(format!(
                "`{path}` says what the locks reach and does not carry the sentence that says how \
                 far.\n\n\
                 Write it verbatim, in prose rather than in a string literal:\n\n    {}\n\n\
                 It is one sentence and not a list of accepted paraphrases on purpose — a \
                 paraphrase is where the claim drifts back to the wider one. The argument behind \
                 it is in `docs/designs/invoke.md`, under \"Where the locks stop\".",
                claim::THE_DECISION,
            ));
        }

        for paragraph in &paragraphs {
            let Some(control) = claim::OUTSIDE_THE_BOUNDARY
                .iter()
                .find(|marker| paragraph.contains(**marker))
            else {
                continue;
            };
            let Some(coverage) = claim::CLAIMS_TO_COVER
                .iter()
                .find(|marker| paragraph.contains(**marker))
            else {
                continue;
            };

            found.push(format!(
                "`{path}` names `{control}` and `{coverage}` in one paragraph:\n\n{paragraph}\n\n\
                 `{control}` lives outside `{}`, so it is a property of this repository's \
                 composition and not of the published crate — a consumer implements `Contexts` \
                 itself and supplies whatever `System` it built. Presenting it as holding \
                 something up is a composition-level control covering a crate-level gap, which is \
                 the overstatement X-55 removed.\n\n\
                 Describe where the control lives and stop. Denying the coverage counts too: a \
                 passage that has to say \"this is not a {coverage}\" is still organised around \
                 the idea that it might be.",
                claim::SCANNED,
            ));
        }
    }

    found
}

/// `text` with everything that is not a claim removed, and paragraph breaks preserved.
///
/// The mirror image of [`code_of`], and the reason is the mirror image too. `code_of` throws prose
/// away because the rules above are about what a source *does*; this throws code away because the
/// rules below are about what a document *says*. In a `.rs` file the claims are the comments and
/// the code is noise — `claim`'s own tables hold a control name and a coverage word three lines
/// apart, and a scanner that read them would refuse this file for declaring its own rule.
///
/// A dropped line becomes an empty one rather than disappearing, so two comment blocks either side
/// of a function do not merge into one paragraph.
fn prose_of(path: &str, text: &str) -> String {
    if !path.ends_with(".rs") {
        return text.to_owned();
    }

    text.lines()
        .map(|line| {
            line.trim_start()
                .strip_prefix("//")
                .map(|rest| rest.trim_start_matches(['!', '/']).trim())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `prose` split on blank lines, with each paragraph's own line breaks flattened to spaces.
///
/// Flattened because a sentence wrapped at column 100 puts "the" and "backstop" on different lines,
/// and a rule that matched line by line would miss every marker unlucky enough to straddle a wrap.
fn paragraphs(prose: &str) -> Vec<String> {
    prose
        .split("\n\n")
        .map(|paragraph| paragraph.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|paragraph| !paragraph.is_empty())
        .collect()
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
        (
            rules::REACHES_THE_SYSTEM,
            "that hands back flux's guarded `System` — process spawn through `run`/`run_with_env` \
             and the filesystem through `read_file`/`write_file`. It reaches all of that without \
             naming `flux_system`, which is why it is refused by call syntax rather than by crate \
             name",
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

    for path in naming(rules::HOLDS_A_TOOL_CONTEXT) {
        if !rules::MAY_HOLD_A_TOOL_CONTEXT
            .iter()
            .any(|allowed| path.ends_with(allowed))
        {
            found.push(format!(
                "`{path}` names `{}`, which is the handle every guarded IO capability hangs off — \
                 process spawn, the workspace filesystem, the worktree ops, each through some \
                 accessor this file cannot enumerate (`ctx.system()`, \
                 `ctx.workspace_context().active()`, and whatever upstream adds next).\n\n\
                 The rule is possession, not use: the seam dispatches with one and the crate root \
                 re-exports the type, and no third file in this crate has a reason to hold one. If \
                 yours does, that is a design decision — take it deliberately and add the file to \
                 `MAY_HOLD_A_TOOL_CONTEXT` with a sentence, exactly as `Egress` works.",
                rules::HOLDS_A_TOOL_CONTEXT,
            ));
        }
    }

    for path in naming(rules::REHEARSAL) {
        if !rules::MAY_NAME_REHEARSAL
            .iter()
            .any(|allowed| path.ends_with(allowed))
        {
            found.push(format!(
                "`{path}` names `{}`, the pack's third entry point. It cannot dispatch today — no \
                 `Egress`, no `execute` — so this is a count rather than a refusal: the list of \
                 ways into `connector-pack` from this crate is bounded on purpose, and a new one \
                 belongs in `MAY_NAME_REHEARSAL` with a reason.",
                rules::REHEARSAL,
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

/// What a `[…]` header in a manifest is, as far as lock 1 cares.
///
/// Classifying the header before reading anything under it is the whole fix for X-48's second
/// finding: the previous parser tested `line == "[dependencies]"` and therefore saw **only** that
/// one spelling, so `[dependencies.reqwest]` made `reqwest` a direct dependency of the dispatching
/// crate with lock 1 reporting `5 passed`.
#[derive(Debug, PartialEq, Eq)]
enum Header<'a> {
    /// `[dependencies]` or `[target.…​.dependencies]` — the entries are the lines that follow.
    Table,
    /// `[dependencies.serde]` or `[target.…​.dependencies.serde]` — the header **is** the entry,
    /// and the lines under it (`version`, `features`, …) are that entry's keys rather than more
    /// dependencies.
    Entry(&'a str),
    /// `[package]`, `[lib]`, `[features]`, `[dev-dependencies]`, `[build-dependencies]`, and their
    /// `target`-scoped and per-entry forms. Nothing lock 1 reads.
    Other,
}

/// Classify one `[…]` header line.
///
/// `dev-dependencies` and `build-dependencies` are [`Header::Other`] deliberately, and the reasons
/// differ. A dev-dependency is absent from a published crate's requirements, so it reaches no
/// consumer's graph — the manifest says so where the table is declared. A build-dependency is
/// reachable only from a build script, and **this crate has no `build.rs`**; adding one would make
/// that reasoning false and is the moment to widen this function rather than to argue about it.
fn header_of(line: &str) -> Header<'_> {
    let header = line
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim();

    if header == "dependencies" || header.ends_with(".dependencies") {
        return Header::Table;
    }

    let named = header
        .strip_prefix("dependencies.")
        .or_else(|| header.find(".dependencies.").map(|at| &header[at + 14..]));

    match named {
        // A per-dependency table's name may be quoted (`[dependencies."my-crate"]`), which is legal
        // TOML and rare enough that only the trimming is worth carrying.
        Some(name) => Header::Entry(name.trim().trim_matches('"').trim_matches('\'')),
        None => Header::Other,
    }
}

/// The names of every **normal** dependency one manifest declares.
///
/// Covers `[dependencies]`, `[dependencies.name]`, and both of those under a
/// `[target.'cfg(…)'.…]` scope — every table whose entries land in a consumer's graph. See
/// [`header_of`] for what is deliberately out of scope.
///
/// Deliberately not a TOML parse: reading these lines needs no dependency, and the shapes it accepts
/// are the shapes Cargo actually allows (`name.workspace = true`, `name = { … }`, `name = "…"`, and
/// the per-dependency table). Two things keep a hand-rolled parser from failing the way hand-rolled
/// parsers fail: `the_dispatching_crate_…` asserts it found *some* entries, so a table it cannot
/// read is a failure rather than a silent pass; and [`the_manifest_parser_reads_every_shape_cargo_allows`]
/// drives it over a fixture holding every shape, in both directions.
fn dependencies_of(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut inside = false;

    for line in manifest.lines() {
        let line = line.trim();

        if line.starts_with('[') {
            match header_of(line) {
                Header::Table => inside = true,
                Header::Entry(name) => {
                    inside = false;
                    if !name.is_empty() {
                        names.push(name.to_owned());
                    }
                }
                Header::Other => inside = false,
            }
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
