# Design: `invoke` — the caller names an operation, and nothing else is theirs

**Status:** written ahead of implementation, 2026-08-01 · **Epic:** invoke ·
**Story:** [X-12](../stories/X-12-invoke.md) · **Blocked on:** [X-11](../stories/X-11-align-the-engine-line.md) ·
**Answers:** [`vision.md`](../vision.md)'s north star ·
**Builds on:** flux's [`ecosystem.md`](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md)
§"The remote binding" and flux-connectors' [`connectors-api.md`](https://github.com/codewandler/flux-connectors/blob/main/docs/designs/connectors-api.md)
§"The confused deputy, answered again" · **Does not cover:** execution records, `subscribe` ·
**Amended by:** [X-13](../stories/X-13-grants-gate-invoke.md), which landed the grant gate this
document reserved a slot for — see §2 step 4 and §6

## This cannot be built today, and the reason is not a detail

`connector-pack` **cannot be linked from this repository**. `codewandler-connector-pack` 0.8.0
requires `codewandler-flux-runtime` at `^0.41` — which Cargo reads as `>=0.41.0, <0.42.0` for a `0.x`
crate — and the flux family is at 0.45.0. `connector_pack::pack` hands out
`Arc<dyn flux_runtime::Tool>`, and two versions of `flux-runtime` are two distinct traits, so the
registry cannot accept it. X-11 tracks the alignment, the work is upstream in flux-connectors, and a
bump is in flight.

**This document describes what is built when that clears.** Nothing here is implementable before it.

There is a workaround, and it is the one thing this design refuses. This host could build the request
itself — read the catalogue's `base_url`, interpolate the parameters, attach the credential, hand the
result to an HTTP client — and it would work, and it would be the credential-injecting proxy the
family already rejected ([`connectors-proxy.md`](https://github.com/codewandler/flux-connectors/blob/main/docs/designs/connectors-proxy.md)).
The whole confused-deputy answer is that *the caller cannot name the authority*, and that answer is
only true because the destination comes from the operation's own compiled Flux rather than from
anything this process composes. A second request path deletes the argument and leaves the service.
So: **wait for X-11.** A blocked epic is cheaper than a design that no longer holds.

The rest of this document is written in the present tense, describing the finished path. Read it as
conditional on X-11.

---

## 1. The route shape

```text
POST /v1/operations/{operation}/invoke
body: the operation's declared parameters, verbatim — no envelope
→ 200 { "operation": "...", "content": "...", "is_error": false }
→ 4xx { "refusal": "...", "address": "...", "retryable": false, "message": "..." }
```

`{operation}` is the catalogue's own spelling of the operation id — `zendesk-ticket-show`. It is the
only noun the caller supplies. The body is the parameter object and nothing else: it is deserialized
as an opaque JSON value and handed to `Tool::execute`, which is the same shape flux itself calls a
tool with.

### What a caller cannot name, and what structurally prevents it

Each of these is a claim about the *shape of the interface*, not about a validator. A validator is a
thing that can be relaxed by one person in one commit; the absence of a field is not.

**Not a host.** There is no request field in which a destination could arrive, because the body *is*
the parameter object — the pack projects a `ToolSpec` with an `input_schema` from the operation's
compiled Flux, and a member the operation does not declare has nowhere to land. Behind that, the URL
is produced by `Operation::execute` evaluating the operation's own `CompositeOpDecl` — the parsed
module, not a re-lowering of the IR — so the destination is the connector author's, fixed at
compile time. And behind *that*: the crate holding this path has no HTTP client in its dependencies
(§2, lock 1), so a URL that did arrive would have nothing to consume it. Three independent reasons,
and the third is the one that survives a refactor of the other two.

The honest residual: a **parameter value** can be interpolated into a path segment or a query, and
for the thirteen connectors with a templated `base_url` an operator's own **configuration** value can
reach the host segment. Neither is caller-named destination — the second is the tenant's own
connection setting — and the reshaping cases are refused by `Error::UnsafeConfig` (a `subdomain` of
`acme.zendesk.com@evil.example` resolves to `evil.example`, because the `@` makes everything before
it userinfo) and `Error::UnresolvedEndpoint` (a parameter whose text spells a `{placeholder}`). X-12's
failing-first test drives a parameter carrying `@evil.example`, `../` and `{subdomain}` and asserts
the **origin observed on the wire is unchanged** — asserted against the recorded URL, not against the
error message.

**Not a credential.** There is no field, and no lookup a caller can steer. The address
`tenants/<tenant>/<authority>/<service>/<credential>` is composed inside `connector-pack` from four
components: the tenant, `provider.authority`, the elided `default` service, and `credential.leaf`.
Three of the four are `&'static str` read out of a generated, compiled-in catalogue — they are
program text, not runtime input. The fourth is the tenant, below. **There is no code path from an
HTTP body to `CredentialRef::new`**, and that is the property to preserve; a route that took a
credential name "so an operator can pick which of two tokens to use" would create one.

A connector declaring no `authority` yields `Error::NoCredentialAddress` and the request is refused
rather than sent unauthenticated — fail-closed with a diagnostic naming the missing *fact*, instead
of a vendor's `401` that says nothing.

**Not a tenant.** The invoke entry point takes a `&Principal` and has no parameter of type `&Tenant`
or `&str` that could stand in for one. `Principal` carries its tenant and has no constructor taking a
tenant separately from an identity; the only producer of one on a request path is `Identity::resolve`,
whose whole input is `presented: &str` — the credential material. There is no tenant-shaped hole
anywhere between the socket and the credential address.

`Principal::new` is public, deliberately: `Identity` is a port and a composing binary must be able to
mint principals from its own IdP. The boundary is that **no route in this repository calls it.**
X-03's three-vector test (path segment, body field, header) is the assertion; this design's
contribution is only that `invoke` gives it nothing new to test.

**Not a runtime, and not the transport.** `Runtime` has no constructor taking caller input — stated
and kept in `crates/exchange-host/src/runtime.rs`. The transport is a constructor argument to the
host, supplied once at startup, and is not reachable from a request.

### Rejected route shapes

- **`POST /v1/invoke` with `{ "operation": …, "params": … }`.** Rejected because an envelope is a
  place to put fields, and the field that eventually gets added is `endpoint`, or `base_url`, or
  `credential`. The shape should have nowhere to put one. This is the same reasoning as "no
  constructor on `Runtime` that takes caller input": remove the slot, not the value.
- **`POST /v1/tenants/{tenant}/operations/{op}/invoke`, with the segment ignored.** Rejected, and
  more firmly than the first. A tenant segment that is *ignored* is worse than one that is honoured,
  because it reads as authoritative in every log line, every client SDK and every support
  conversation — and the first person who "fixes" the inconsistency by honouring it has broken the
  north star in a one-line diff that looks like a cleanup.
- **`POST /v1/connectors/{connector}/operations/{op}`.** The connector is derivable from the
  operation id — `catalog::operation(OperationKey::id(…))` is a global lookup and the entry carries
  `provider`. A redundant name is a name that can *disagree*, which needs a reconciliation rule, and
  a reconciliation rule is a decision procedure over caller input about which connector to use.
- **The dotted tool name (`zendesk.ticket.show`) in the path.** That spelling exists because a dotted
  name is not a legal Flux declaration and every flux tool is dotted; it is the *tool surface's*
  spelling. The catalogue's `zendesk-ticket-show` is the *addressing* spelling, and it is what X-06
  serves. One spelling on the wire; `dotted_name` derives the other internally.

---

## 2. The dispatch path

Ordered, and the order is load-bearing at three points.

1. **Resolve the principal.** `Identity::resolve`. `Ok(None)` is anonymous and is not an error;
   `IdentityError::Rejected` and `IdentityError::Unreachable` stay distinct end to end.
2. **Look the operation up in the catalogue**, then its provider. Unknown operation → terminal
   refusal naming the id.
3. **`Deployment::admits(surface.runtime)`.** §4.
4. **Consult the caller's grants** — `admit_grant`, refusing with `Error::NotGranted`. **Landed in
   X-13**, and it was the insertion this slot promised rather than a re-plumb: the facts come from
   `OperationFacts::of(entry)` and the grants from the [`Grants`] port at the resolved principal's
   tenant. It takes the `Admitted` step 3 produced and yields a `Granted`, which is what step 7 is a
   method on — so the two gates are one chain the compiler checks rather than two calls to remember.
5. **Construct both ports from the one tenant, at one call site** — `Credentials::new(store, tenant)`
   and `Configuration::new(settings, tenant)`. Building them from a single value in a single
   expression is what makes `Error::TenantMismatch` unreachable here rather than merely untriggered.
6. **A fresh `ToolContext`**, one per invocation. §3 of the redaction rules.
7. **`connector_pack::pack(&[provider.id], egress, credentials, configuration)`** into a fresh
   `ToolRegistry`. Fresh per request: projection is cheap, and it is what keeps one tenant's resolved
   configuration snapshot from outliving the request it was read for.
8. **`tool.execute(&ctx, params)`.** Inside this call and nowhere else, the pack resolves the
   credential, registers every travelling form with `ctx.redactor`, verifies the registration took,
   evaluates the operation's compiled Flux into `{method, url, headers, body}`, places the
   credential, and hands the whole thing to flux's own `http.request`.
9. **Render the response through the same `ctx.redactor`** that the credential was registered with.

Steps 1–7 are wiring. **Everything that makes a request correct and safe is inside step 8, and none
of it is ours.** That is the design.

### This host builds no request of its own — enforced, not intended

The rule is easy to state and easy to erode: the erosion is always a small, reasonable-looking
addition — a health-check pinger, a token-refresh call, an OAuth authorization-code exchange, a
webhook registration helper. Each needs an HTTP client, and once one exists the second request path
is a function call away.

**Three locks. The first is the gate; the others make its failures legible.**

**Lock 1 — the crate that dispatches has no transport, and its dependency list is an allow-list.**

The whole invoke path lives in `exchange-host`, the published crate. It receives the transport as a
constructor argument typed `connector_pack::Egress` — the same port shape `connector-pack` itself
uses, one level up. `exchange-server` is the only crate that names `flux_web` or constructs
`HttpRequestTool`, and it is the only crate with a server framework.

A test reads `crates/exchange-host/Cargo.toml` — its own manifest, no network, no `cargo` invocation
— and asserts its `[dependencies]` table is a **subset of an allow-list written in the test**, each
entry with a one-line reason. Not a deny-list: a deny-list only catches the transports somebody
thought of, and it passes for `ureq`, `isahc`, `attohttpc` and whatever ships next year. An
allow-list fails on *any* new dependency, and the person adding one has to write down why it is not
a transport. The complementary assertion is one line: **`exchange-server`'s sources never name
`connector_pack`.** The crate that can build a request cannot name the pack; the crate that names the
pack cannot build a request.

The manifest is the checked artifact rather than `cargo metadata`, deliberately. The resolved graph
unifies features across the workspace, so `connector-secrets`' optional `vault` feature (which pulls
`reqwest`) enabled anywhere would make a closure-based check either fail spuriously or need an
exception that swallows the real signal. A crate's own `[dependencies]` table is unaffected by
unification, and it is exactly the thing a second request path would have to change.

What lock 1 does *not* cover: `flux-system` is reachable transitively (it is where flux's real IO
lives) and `flux-runtime`'s `ToolContext` is how the pack is called at all. The claim is therefore
"no transport is a *direct* dependency", plus lock 2 for the reachable ones.

**Lock 2 — one seam, counted.** A scanner over `crates/exchange-host/src/**/*.rs` enforces ten
rules. Each is one string: some no source in that crate may write, some only a named file may. They
are listed below, under "What lock 2 is, and what it is not", with what each catches **and what it
cannot** — a list rather than a summary at this point, because the summary that used to sit here
described the three rules X-12 shipped and was still describing them once there were nine.

Source scanning is a blunt instrument and it is used here because the repository already runs one and
already knows how to keep it honest: `console/test/components.test.mjs` is guarded by a test that
runs the scanner against sources it *must reject* and sources it *must accept*. This scanner gets the
same treatment, and without it the scanner is worth nothing — a regex that matches nothing passes
every file. Since X-56 the same treatment runs one step further out:
`the_design_says_what_every_lock_2_rule_is` reads the rule list out of the test and fails if *this
document* has stopped naming one of them — which is the drift above, and it went unnoticed because
nothing was measuring the distance between the two.

**Lock 3 — a counting transport, for what the other two cannot see.** Tests construct the host with
an `Egress` wrapping a `Tool` that records every call and its URL. Then:

- a successful invoke calls it **exactly once**, and the recorded origin is the connector's declared
  one whatever the parameters were;
- **every refusal in §5 leaves the count at zero.** That is what "the request was never sent" means
  as a test rather than as a sentence in an error message, and it is the assertion `MissingCredential`
  being terminal actually rests on.

Lock 3's limit, stated because it is the one most likely to be over-read: it proves things about the
paths a test drives, never about paths it does not know exist. Locks 1 and 2 are what speak to
absence. Keep all three; they fail differently, and "Three mechanisms, and they fail differently"
below is where that is set out.

#### What lock 2 is, and what it is not

**Lock 2 checks names, not values.** Every rule is a string, and each refuses — or bounds to named
files — a source that writes that string. It cannot see a capability arriving under a name nobody
listed, and it cannot see what a value *is*: a host in a `const` is text like any other text. Two
independent review rounds each worked this out from the test because this document did not say it,
which is a cost paid per review rather than once, and it is stated before the table because it
decides how to read every row.

The rules as they stand, each with its edge. The scan is over code — whole-line comments are
stripped first, so documenting a rule is not a violation of it.

| Rule | Where it may appear | What it catches | What it cannot |
|---|---|---|---|
| `connector_pack::resolve` | exactly one file — and *exactly*, not *at most* | a second file resolving an operation, and equally the deletion of the only one, which would otherwise satisfy every other rule here | a second seam reaching the pack under an alias (`use connector_pack::resolve as go`) or through a re-export |
| `connector_pack::pack` | nowhere | the pack's **model-facing** entry point, which installs a whole provider's tools into a registry. An execute route wants `resolve`; `pack` would be a second way in, and would silently withhold every `expose = false` operation from a caller entitled to run it | the same, aliased |
| `connector_pack::Rehearsal` | `settings.rs` | the pack's **third** entry point turning up somewhere new. It takes no `Egress`, holds no transport and has no `execute`, so nothing reached through it can dispatch — this is a count, not a refusal, and the point of the count is that `resolve` and `pack` were once believed to be the whole list | a *fourth* entry point. The rule knows the three that exist; a new one is invisible until upstream ships it and somebody reads the changelog |
| `connector_pack::channel_plan` | `channel.rs` | the pack's zero-transport channel planner appearing anywhere except the tenant-owned channel seam. It resolves configuration and credentials into redacting wire wrappers but owns no client, socket or `Egress`; execution stays in the composing binary's selected Flux system | the same call reached through an alias or re-export, and anything the composing binary does with the returned value |
| `.tool()` | nowhere | unwrapping the transport out of its `Egress` — the second request path, in one line | the same unwrap through some other accessor, or a binding that never spells the call |
| `.execute(` | the seam only | a tool dispatched from anywhere but the file that resolved it | call syntax only: the same dispatch spelled `Tool::execute(tool, …)` is a different string |
| `Egress` | `invoke.rs`, `lib.rs` | the transport port travelling to a third file, from where it is one refactor away from `.tool()`. **Not** "exactly two occurrences", which is what this section claimed until X-56 and was never what the scanner counted — the rule bounds *which files* may name it, not how often | a transport that is neither an `Egress` nor named as one |
| `ToolContext` | `invoke.rs`, `lib.rs` | **possession** of the handle every guarded IO capability hangs off — process spawn, the workspace filesystem, the worktree ops. A file that cannot name the handle cannot take one, store one or return one, so it has nothing to call an accessor on, whatever the accessors are called this release | a context obtained without naming the type. That needs a public accessor for `Invoker`'s private `contexts` field and there is none; add one and this rule is back to being one accessor behind |
| `.system(` | nowhere | the shortest spelling of "give me the guarded `System`", caught cheaply | `.workspace_context().active()`, which is a different public accessor returning the same `Arc<System>` — see the demonstration below. Read this as the spelling somebody reaches for by accident, not as a boundary; the boundary is the row above |
| `flux_system`, `std::net`, `tokio::net`, `reqwest`, `hyper`, `ureq`, `isahc`, `attohttpc`, `TcpStream`, `UdpSocket` | nowhere | a transport or a socket named in a source, including the ones lock 1 cannot refuse because they arrive transitively — `flux-system` is reachable through `flux-runtime`, which is exactly why naming it is refused here rather than only in the manifest | a client whose crate nobody listed. This is the one deny-list among the three locks, and it is *why* lock 1 is an allow-list: this row can only ever name the clients that already exist |

**The demonstration, which is worth more than the argument.** X-48's second review round added one
file to the dispatching crate:

```rust
let handle = ctx.workspace_context().active();
handle.run(&argv, Duration::from_secs(5)).await
```

That reaches `System::run` — a process spawn — naming nothing on the forbidden row, no `.tool()`,
and not even `.system(`, which had been written believing it was the only spelling. The whole
workspace stayed green. **A rule that chases accessor spellings will always be one accessor behind**,
because the set of ways to get a value out of a public type is not a set a scanner can enumerate.

So the instrument changed rather than the string list growing: the `ToolContext` rule bounds
possession instead of use, which is the same shape as the `Egress` rule — a capability that travels
to a bounded, readable set of places. That is a real narrowing, and it is still a name check. Which
is the sentence this subsection opens with, and the reason it opens with it.

#### Three mechanisms, and they fail differently

None of them is the argument on its own. They are set out here once, and
`crates/exchange-host/tests/no_second_request_path.rs` points at this subsection rather than
restating it — the argument lived in both until X-56, and two copies is how the paragraph above
drifted in the first place.

- **Lock 1 is not a name check.** It fails on a name it has *never heard of*, which is the property
  a deny-list cannot have. Its scope is the dispatching crate's normal dependency tables —
  `[dependencies]`, `[dependencies.name]`, and both of those under `[target.…]`; `dev-` and
  `build-dependencies` are deliberately out, with the reasons on `header_of`. Its blind spot is the
  mirror of lock 2's: a capability reached *transitively*, through a crate already on the list,
  which is how `flux-system` is reachable at all.
- **Lock 2 is a name check over sources**, with each rule's edge in the table above. What it
  contributes that no test can is a statement about *absence* across the whole crate.
- **Lock 3 is behavioural** — a counting transport, one dispatch per invoke and zero for every
  refusal. It proves things about the paths its tests drive and nothing about paths nobody wrote a
  test for.

**Three, and not four.** The fourth this argument used to count was the deployed composition's
sandbox posture. X-55 struck it from the count — not because it is untrue, but because it lives
outside the boundary the locks bound, so it is not a property of the crate that ships. That is the
next subsection.

#### Where the locks stop

Two readings of the three locks above were live at the same time, and they are different claims:
**are the locks about the published crate, or about the deployed binary?** X-55 settled it, because
a boundary nobody has decided is one that moves by accident — and because several documents were
reading as the second while the code did the first.

**The locks bound the published crate, not the deployed binary.** `exchange-host` is what
`cargo publish` uploads and what a consumer composes into a process of its own. `exchange-server` is
`publish = false`: one composition of this library, and not the one anybody downstream runs. So lock
1 reads `crates/exchange-host/Cargo.toml`, lock 2 walks `crates/exchange-host/src`, and
`exchange-server` gets exactly one rule — its sources may not name `connector_pack`.

**The alternative, and why it was not taken.** Widening lock 2 to `crates/exchange-server/src` is
the honest other answer, and it fails on contact with what that crate is. `exchange-server`
legitimately holds a transport — that is what a composition is *for* — so `execution.rs` names
`flux_system`, constructs `HttpRequestTool` and holds a `ToolContext`, and every rule in lock 2 goes
red on correct code the day the scan reaches it. What answers that is an exception list with an
entry per file, extended by whoever is adding the thing the rule is meant to catch: the "one more
file on the list" drift these locks exist to prevent. Lock 1's allow-list works because a dependency
is a rare and deliberate addition; a per-file exception list over a crate under active development
is not the same instrument wearing the same clothes.

**What the decision costs, stated rather than left to be found.** A second request path added to
`exchange-server` — an OAuth code exchange in a route, a health-check pinger in `main.rs` — is
caught by nothing structural. That crate holds `flux_web` and binds the credential store, so it
could build one, and the one rule that reaches it bounds *naming the pack* rather than *building a
request*. This is a known residual and a review matter. If it ever needs closing, close it with a
rule shaped for a crate that is supposed to hold a transport, not by pointing lock 2 at it.

**What the decision forbids.** No document may present a control from outside
`crates/exchange-host/src` as covering a gap inside it. The concrete case is the one X-48's second
review round left open: `crates/exchange-server/src/execution.rs` builds this composition's `System`
with `SandboxMode::Require`, so `System::run`/`run_with_env` are confined or refuse. That is true,
it is useful, and it is a property of **this repository's deployment**. For a consumer of
`codewandler-flux-exchange-host` it does not exist — a downstream binary implements `Contexts` and
supplies whatever `System` it built, quite possibly `System::new`, whose sandbox is disabled. The
argument therefore counts **three** mechanisms rather than four, and the composition's posture is
described where it lives instead of counted here.

Both halves are checked rather than reviewed, in
`crates/exchange-host/tests/no_second_request_path.rs`:
`no_document_claims_more_than_the_locks_reach` requires every document that says what the locks
reach to carry the sentence above verbatim, and refuses a paragraph that names a control from
outside the boundary alongside the vocabulary of leaning on one;
`the_locks_bound_the_published_crate` pins the boundary itself, so widening it fails a test with the
argument attached rather than passing as a quiet edit to a path string.

**Rejected:** relying on review, and relying on a grep for `reqwest` in the sources. The first is
what X-12 exists to replace. The second checks a name rather than a capability, passes for any crate
nobody listed, and would have to be updated by the same person who is adding the thing it is meant to
catch.

---

## 3. Credential resolution and redaction ordering

The ordering rule and its verification belong to `connector-pack` and are not reimplemented here.
`Credentials::resolve_mechanism` registers each credential with `ctx.redactor` **before the request
exists and before the next fallible step**, then *asks the redactor whether it took* — because
flux's `Redactor::add_secret` silently ignores a value under six trimmed characters, so registration
can succeed and protect nothing. A value the redactor does not end up holding is
`Error::UnredactableCredential`, and **it is not sent**.

This host's job is to not defeat that. Four obligations:

**One `ToolContext` per invocation.** The redactor is per context, so a credential registered for one
call must not outlive it into another tenant's. A pooled or long-lived context would also break the
pack's idempotence check, which asks the redactor *in hand* rather than consulting a memo — precisely
because a remembered registration against a redactor that never received the value is a credential
travelling unheld.

**The same `ctx` into `execute` and into the response rendering.** The vendor's response is rendered
through `ctx.redactor` before it reaches the route. Several vendors echo a token back in an error
body, and at the pinned flux-web `http.request` returns one flat string
(`HTTP {status}\n{headers}\n{body}`) returned whole — so the difference between "the pack kept it off
the wire" and "it stayed off every surface" is this one call.

**The host resolves no credential itself.** Its only contact with the store outside the pack is a
presence check for the connections surface, and that check returns `bool`. No method on this host's
API returns a `Secret`. `StoreError::NotFound` answers "no"; anything else is reported as unknown
rather than collapsed into "no", because "unreachable" and "not connected" want opposite responses
from an operator.

**A credential too short to redact is refused, not sent.** `UnredactableCredential` names the tenant,
the authority and the credential's name — the address, minus the value. It names neither the value
nor its **length**: a length is a fingerprint. This is worth writing down because the refusal reads
like an incomplete diagnostic and the obvious "improvement" is to add the length.

### How "the registration took" is verified from this side

The pack verifies it internally; X-12 asks this host to assert the observable. Two tests:

- **A sentinel of six or more characters**, stored at a tenant's address, one invoke against a
  loopback vendor that echoes the `Authorization` header back in its body, and the assertion that the
  route's response carries **no substring of the sentinel**.
- **A five-character sentinel** → refusal, and the counting transport at zero.

Both sentinels must carry none of flux's known credential prefixes (`sk-ant-`, `xoxb-`, `ghp_`).
flux's redactor runs a second, shape-based pass over tokens it was never told about, so a sentinel
that looks like a credential would be scrubbed whether or not anybody registered it — and the test
would pass while asserting nothing. `connector-pack`'s own `tests/credentials.rs` keeps a sentinel
for exactly this reason; copy the reason, not just the constant.

---

## 4. Where `Deployment::admits` sits

**After the catalogue lookup, before anything touches the credential store.**

After, because the check needs the connector's declared runtime, which comes from the catalogue.
Before the store, because a refusal that happens *after* a secret has been read has already moved
that secret into this process's memory for a connector it was never going to run. "Refuse; never
repair" is cheapest at the earliest point where the answer is knowable, and this is that point.

Before `pack(…)` too: installation is where projection and the tenant-mismatch check happen, and
there is no reason to project a connector this deployment will not execute.

Before the grant check, on the reasoning that the runtime refusal is a property of the
*deployment* and is the same answer for every principal — cheaper, and it leaks nothing, because the
catalogue X-06 serves is unfiltered and already publishes what a connector declares. That ordering
was called reversible with X-13 owning the final word; **X-13 kept it**, and made it structural: the
runtime gate is what mints the `Admitted` the grant gate consumes, so swapping the two is now a
change to two signatures rather than a reordering of two lines.

`RuntimeRefusal` already carries the message and the tested requirement that it *names the way out* —
"run this connector in a single-tenant deployment, or isolate it per tenant at the OS or pod level".
Route it through unchanged. Do not add an override, a force flag, or a per-connector exception: the
refusal exists because isolating a locally-executing runtime is an OS or pod concern that cannot be
done from inside one Rust process, and an override is a lie about that.

**A measured gap, which the test must account for.** `catalog::Provider` has **no runtime field**;
every connector in the shipped catalogue is HTTP. So `ConnectorSurface::runtime` cannot be *read*
from the catalogue and must be derived — `Runtime::Http` for every catalogue connector today — and
the derivation must be documented as a derivation, on the same terms X-06 sets for `effects`. The
consequence for X-12: **no shipped connector exercises the refusal**, so the test must construct a
`ConnectorSurface` declaring a locally-executing runtime. That makes the path *more* worth testing,
not less — it is a fail-closed guarantee with no accidental coverage, which is the same situation
`connector-pack` records for its query-placement branch.

---

## 5. Failure taxonomy

Two questions, and only the second one is usually asked: *was the request sent?* and *will retrying
help?* An agent needs the first and infers the second, and the HTTP status space cannot express it —
`502` says nothing about whether the effect happened.

**So `retryable` is a field in the refusal body, not something a client derives from the status
code.** It is set to `true` only when the failure **provably precedes dispatch**, or when it follows
dispatch on an operation the catalogue declares `Idempotency::Idempotent`.

`Idempotency::Conditional` is **not** marked retryable. The condition a conditional write depends on
is stated in prose the host cannot evaluate, and a host that guesses turns "safe to repeat if you
pass the same key" into a duplicated write.

| Failure | Sent? | Retryable | Status | Why |
|---|---|---|---|---|
| `IdentityError::Rejected` | no | no | 401 | The credential was presented and is bad. |
| `IdentityError::Unreachable` | no | **yes** | 503 | The IdP is down. Opposite operator response; never collapse the two. |
| unknown operation | no | no | 404 | Nothing in the catalogue spells it. |
| `RuntimeRefusal` | no | no | 409 | This deployment will not serve that runtime, ever, for anyone. |
| `NotGranted` | no | no | 403 | No grant this caller's tenant holds admits it. `403` and not `404`: the catalogue is anonymous and publishes the operation to strangers, so hiding it here would be a fiction the surface next door disproves. |
| `MissingCredential` | no | no | 422 | See below. |
| `MissingConfig`, `MissingCredentialConfig` | no | no | 422 | The tenant's connection is incomplete. Names the field and the service. |
| `NoCredentialAddress`, `UndeclaredCredential`, `InboundCredential`, `EmptyMechanism` | no | no | 422 | The connector cannot be addressed or authenticated as declared. |
| `UnredactableCredential` | no | no | 422 | The value cannot be kept off a surface, so it does not go on the wire. |
| `UnsafeConfig`, `UnresolvedEndpoint` | no | no | 422 | The composed request is not the one the gate was shown. |
| `TenantMismatch` | no | no | 500 | Unreachable by construction (§2 step 5); if it fires, this host is wrong. |
| credential store unreachable | no | **yes** | 503 | A transport failure at the store. Distinct from `NotFound`, which is the row above. |
| transport failure reaching the vendor | **maybe** | only if declared idempotent | 502 / 504 | The one genuinely ambiguous row. |
| vendor `4xx` / `5xx` / `429` | **yes** | n/a | 200 | Not a failure of this host. Returned as a result with `is_error`, unshaped. |

That last row is the one most likely to be got wrong in a comfortable direction. A `404` from Zendesk
is an *answer*, and flattening it into a host error destroys the distinction between "the vendor said
no" and "we could not ask".

**This host does not retry.** It labels. Retry policy belongs to the caller, which knows its own
deadline and its own idempotency requirements; a host that retried a `429` on the caller's behalf
would be spending someone else's rate limit against a budget it cannot see.

### Why `MissingCredential` is terminal

Three reasons, in increasing order of how much they cost when ignored.

1. **The request was never sent.** There is no ambiguity about whether the effect happened, so retry
   buys nothing but the same answer.
2. **Nothing that a retry can change is inside this system.** The refusal names an address a human has
   to go and put a value at. Time does not do that.
3. **A retrying agent against a credential-less connector is a loop.** `connector-pack` requires its
   credential port rather than accepting an `Option` for exactly this reason — a pack without one
   would send unauthenticated requests, get a fail-closed `401`, and a host treating `401` as
   retryable would loop on it forever. Marking `MissingCredential` retryable reintroduces that loop
   one layer up, and it does so against a vendor's rate limit.

**Every refusal names the address and never the value.** That is one rule with three edges: no
credential value on any surface including errors; no *length*, which is a fingerprint; and, for
connection settings, no value either unless the value is the thing the operator has to see in order
to fix what they pasted — which is the one deliberate exception `UnsafeConfig` takes, and it takes it
for a non-secret.

---

## 6. What this design does not cover

- **Grants.** ~~X-13.~~ **Landed**, in the slot §2 step 4 reserved. What this document said until
  then — *"`invoke` is gated by identity alone, and that is a stated gap rather than a position"* —
  is no longer true of the code, and the two things it left undecided were decided as follows.

  **Whose grants.** A **tenant's**, not a principal's. Every principal this host resolves for a
  tenant is decided against the same set, which is the shape `agent-access.md` already describes.
  Per-principal grants are a narrowing this build does not make; the refusal names the principal
  because that is who was refused, not because the grant was theirs.

  **What the absence of a grant store means.** *Nothing runs.* An unset `FLUX_EXCHANGE_GRANTS`
  builds no invoker, and the route answers `503` naming the setting. The alternative reading — no
  grants configured, so admit everything — is exactly the exposure this story closed, reintroduced
  as a default: the safe state is the one an operator gets by doing nothing.

  **The ordering §4 said was reversible** — runtime refusal first, grant second — was kept. The
  runtime refusal is a property of the deployment and is the same answer for every principal, it
  leaks nothing the anonymous catalogue does not already publish, and it is what mints the
  `Admitted` the grant gate consumes.

  **A surface for editing a grant** — this paragraph used to say there was none, and that the store
  was a file an operator writes. **X-62 closed it**, because a gate nothing can configure is a gate
  that runs nothing: `GET`/`PUT /api/grants` read and replace a tenant's set, and
  `POST /api/grants/preview` answers what a proposed grant *would* admit before it is saved. Three
  decisions came with it, and each is here rather than in `agent-access.md` because each is a
  question about *what a grant is*:

  - **It expresses a selector and refuses an operation id.** The wire body is a connector plus
    `max_risk`, `effects_within` and `idempotency`; a request naming `allow_ids`, `deny_ids` or any
    other id-shaped field is refused with `422` and the argument. `Selector`'s exception lists stay
    for the file an operator edits by hand — a route that deserialised `Selector` verbatim would let
    a console write names back into a model whose whole point is that it does not read them.
  - **The preview is derived from the projection the gate decides on**, `OperationFacts::of` through
    `ConnectorSurface::admitted`, and not from a second copy of `Selector::admits` beside the screen.
    `routes::tests::a_grant_written_through_the_surface_admits_exactly_what_the_gate_admits` asserts
    the two agree against `admit_grant` itself.
  - **Both verbs, and the preview, admit a `User` and nothing else.** Editing decides which
    operations run at all, which is more authority than supplying a credential (X-54). The *read* is
    gated too, which is the half that is easy to get wrong: `admit_grant` deliberately withholds a
    tenant's policy from a refused caller so that an agent cannot enumerate it one call at a time,
    and a read open to every kind would hand the whole of it over in one request.

  What is still a file-only decision is an **id exception**: a hand-written grant carrying
  `allow_ids` or `deny_ids` is *shown* by the read, marked as one this surface cannot express, and a
  `PUT` that would replace it is refused with `409` rather than dropping it silently.
- **Execution records.** Nothing here writes an audit trail. `vision.md` requires that every execution
  be explainable — who asked, which grant admitted it, what was called, what came back — and X-13
  made three of those four facts exist. *Which grant* admitted it is the one still missing: the gate
  answers yes or no, and a `Granted` carries the operation rather than the grant that produced it.
  A record wants the grant, so that is the first thing the story after this one has to add.
- **`subscribe`.** The inbound half of the binding. It needs the confused-deputy argument made in the
  inbound direction ("a subscriber cannot name a binding it has not been granted"), and that argument
  is sound only once an authenticated principal exists.
- **Leases and streaming.** The `Lease` type is tested and nothing holds one. It needs a runtime that
  keeps state open, which means the runtime axis beyond `http`, which a multi-tenant deployment
  refuses.
- **Response shaping.** Whatever `http.request` returns is returned, redacted and otherwise whole.
- **Idempotency keys, rate limiting, concurrency limits, and retry policy.** Caller-side or later.

## 7. Risks

- **The response shape will have changed by the time this is built.** `connectors-api`'s `exec.rs` is
  the prior art for this path and it handles `http.request`'s flat string, because flux-connectors
  pins flux-web 0.41. Since flux-web **0.43.0** the canonical `ToolResult.content` is the record
  `{status, headers, body}` and the flat block is only the model-facing view. X-11 will land the
  *current* line, so invoke is likely built against the record. **Re-measure before copying
  `exec.rs`**; do not port the flat-string handling on faith.
- **The allow-list test is annoying by design, and the failure mode is deletion.** Somebody adding a
  dependency meets a red test that is not about their change. Mitigation: the test's failure message
  states the rule and cites this section, so the cheapest way out is to add the entry with its reason
  rather than to delete the test. Worth watching in review; a diff that removes it is a blocker.
- **`connector-catalog` and `connector-pack` have to move into `exchange-host`.** Today the catalogue
  is a dependency of `exchange-server`. Invoke needs both in the host crate, which means the
  allow-list gains two entries in the same change that introduces it. This overlaps X-02's live work
  on the server crate; the route *handler* stays in `exchange-server` and remains a thin adapter,
  while the invoke *function* lives in `exchange-host`. Coordinate rather than assume.
- **A future OAuth refresh, webhook registration or health-check pinger is the real threat to lock 1,**
  and each will arrive with a good reason. The answer is not to refuse them but to place them: a
  token refresh is a *credential-store* concern with its own design and its own crate boundary, and
  if it needs a transport it takes one as a port, exactly as this does. What it must never do is give
  `exchange-host` an HTTP client.
- **This document runs ahead of the code, which is the failure mode `connectors-api.md` records
  about itself.** Nothing described here exists. When it does, §2's ordering and §5's table are to be
  re-measured against the implementation rather than re-read.
