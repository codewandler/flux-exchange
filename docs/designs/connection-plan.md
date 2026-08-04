---
story: X-125
status: accepted
---

# One declaration-driven labelled connection plan

The connection surface already has three correct but separate write models: X-14 owns labels and
host-minted instance UUIDs, X-47 owns non-secret settings, and X-10 owns credentials. A first-time
client cannot safely infer how those models compose. This design adds one projection and one
orchestrator; it does not add another connection identity, store, or vendor schema.

## The declaration is the form

`GET /api/connections/{connector}/plan` is an authenticated `User` read. A configured operator is
still a `User` and receives the same plan; a Service Account is denied rather than receiving either
the plan or an authority proposal. The plan is value-free: it may report a selected label, whether a
field is set, and a custom-origin lifecycle state and revision, but no stored setting, credential or
origin. Exact custom-origin inspection is a separate configured-operator-only read described below.

The plan projects `connector_catalog::Provider::config` and `Provider::auth` in declaration order.
The response is versioned as `exchange.connection-plan.v1` and starts its `fields` array with the
synthetic `name` field. Every remaining configuration row comes from one catalogue `ConfigField`,
with its stable service-qualified identity, human label, requiredness, input kind, declared binding,
additional bindings, closed choices, submission target, and a `set` boolean. No Exchange-owned
connector or vendor field list participates.

Catalogue 0.18 still carries some credentials only in `Provider::auth`, with no `ConfigField` form
metadata. Dropping them would make Slack, OpenAI, Intercom and other connectors look complete while
their declared credential address is empty. The projection therefore synthesizes a conservative
secret row for every auth target no routable config row binds. Its label falls back to the declared
credential name and its provenance states that richer form metadata was absent. Requiredness is
derived generically from operation authentication alternatives: a credential is required when at
least one declared operation has no mechanism that omits it. This keeps a lone OpenAI token required
without making Slack's inbound-only signing secret a prerequisite for bot operations. This is
generic over connector declarations, not a list of affected connectors. A catalogue census keeps
every auth target represented if upstream adds another metadata-poor credential.

The declaration's `binds` value decides the target generically:

- `credential.<name>` addresses the existing credential write surface and is a secret input;
- every other binding accepted by `DeclaredSetting` addresses the existing service-scoped settings
  surface; and
- a field that cannot be parsed, matched, or admitted remains in the response as `routable: false`
  with a reason. A required unroutable field makes the plan incomplete. It is never omitted.

Closed choices come from `Provider::choices_for`, keyed by the declared service and binding. A
choice field is therefore a closed select for every consumer. Fields without a closed set publish
no `choices` member.

Credential declarations that several service-local form rows bind to one provider credential have
one submission target. The rows retain their declaration identities and point at the same target;
the composite request accepts the target identity once, so a shared Zendesk token is never requested
or written several times merely because several services use it.

`also_binds` remains ordered declaration metadata on its one row. It creates neither another input,
another submitted value nor another settings-store address: the connector applies those additional
bindings internally from the value stored for the primary `binds` target.

### CLI aliases are a projection, not a second schema

Every field publishes its complete `aliases` array, including an empty array when no command-line
spelling is safe. `connection.name` owns `--name`. A non-secret catalogue form row receives exactly
one alias by turning its declared field `name` from lower snake case into a long kebab-case option
(`site` becomes `--site`, `account_email` becomes `--account-email`). A connector that declares its
field as `endpoint` therefore publishes `--endpoint`; Exchange never substitutes that spelling based
on the connector or the field's semantics. Secret rows receive no alias because a secret must never
travel on argv. The rule uses only stable field identity; it does not inspect connector id, vendor,
label, help, URL template, policy or target. The committed shared fixture materializes every array,
so neither server nor console can treat an omitted alias member as an implicit empty list.

The server validates the resulting alias set over the whole plan. An alias outside the closed
`--lower-kebab-case` grammar, repeated within one field, or claimed by two field identities refuses
the projection; it is never silently reassigned or disambiguated with a vendor-specific spelling.
Consumers read this array and do not repeat the derivation. The console parser requires the member
on every field, checks that it is an array of unique grammar-valid strings, and refuses the complete
plan when it is malformed. This leaves an upstream declaration free to grow explicit alias metadata
in a later protocol version without baking today's convenience rule into a client.

## Selecting and naming a connection

The read lists the tenant's existing labels and accepts an optional `name` query solely to select a
label for editing. A label is operator-owned metadata and is safe in a URL; no setting or credential
value is. Selection resolves through X-14's registry and the credential inventory. The response may
name the label and whether a field is set, but never exposes the host-minted instance UUID.

The composite write accepts `name` first and an optional `current_name` when renaming. A new name
uses X-14's existing instance create path. An existing name edits that connection. A changed name is
renamed only after its value writes finish, so every earlier target remains stable during the
operation. X-14's registry rename preserves the instance UUID, credential addresses, and scoped
settings by construction.

## One write contract, honest partial state

`POST /api/connections/{connector}/plan` accepts the plan version, `name`, optional `current_name`,
and a map keyed by published target identities. The body type deliberately has no `Debug` formatter.
Secret values appear only in this request body and are wrapped or consumed immediately by the
existing credential handlers. They never enter a response, URL, query, activity record, Flux
argument, navigation state, or log.

The credential store, settings store, and label registry cannot commit one cross-store transaction.
The API says so rather than pretending otherwise. It executes and reports an ordered apply plan:

1. create the labelled credential set atomically, or rotate only submitted credentials on an
   existing connection;
2. write submitted settings in declaration order, one checked store operation per target; and
3. rename an existing label last, if requested.

Every step reports `applied`, `unchanged`, `refused`, or `skipped`, its target identity, and a
value-free reason when it did not apply. Execution stops at the first refusal. The response embeds a
fresh projection of what survived and classifies the attempt as `complete`, `incomplete`, `refused`,
or `partial`. `207 Multi-Status` denotes a partial attempt; successful complete or incomplete
attempts use `200`. Retrying the same `name` is safe: once creation landed, the next attempt edits
that named connection and only submitted targets are replaced. Compensation is explicit in the
plan: settings can be unset and the whole labelled connection can be removed through the existing
routes; Exchange never claims to have rolled back a value that another store already committed.

Unknown target identities, duplicate aliases for one target, and malformed selection are refused
before the first write. Missing inputs are omissions, not empty values, and keep required fields
visibly incomplete.

## A custom origin is proposed before it is authority

Most endpoint settings fill a path, select from a connector-declared closed set, or remain below a
literal vendor suffix. A custom-origin field is different: its value becomes the destination
authority that receives this connection's credential. Supplying a credential or setting proves
only that a human controlled an input. It does not prove that an operator reviewed the resulting
authority, so neither the composite write nor the ordinary setting write may approve one as a side
effect.

The released connector declaration supplies a typed custom-origin rule. That connector-shared rule
parses, validates and normalizes the operator's proposed HTTPS origin and produces both the stored
setting value and the exact normalized authority that request construction and permission subjects
must share. Exchange does not infer this property from a connector id, field name, input kind or a
local `CustomOriginPolicy` boolean, and it does not maintain another URL parser. An unsupported
scheme, malformed whole origin or declaration that has no released typed rule refuses before any
authority mutation.

The settings port persists an authority lifecycle beside the normalized value, under the same
tenant, connector, instance, service and declared field address:

- `unset` has no proposed value;
- `proposed` has a value and a non-reusable store-wide proposal revision, but the runtime cannot
  read it;
- `approved` records that an operator explicitly approved that exact revision, so the runtime may
  read it; and
- `revoked` keeps the proposed value for repair but makes it unreadable to the runtime again.

An initial custom-origin submission creates a new revision in `proposed`; it never carries approval
from any earlier value. Replacing a `proposed`, `approved` or `revoked` value is a distinct proposal
transition and must carry the current `expected_revision`. The store compares that revision while
holding its write lock, revalidates the released typed rule, normalizes the new origin, and commits a
strictly higher `proposed` revision. A stale or concurrent replacement loses the compare-and-swap and
mutates neither the value nor the revision high-water mark. Supplying the same origin bytes is still
a replacement and still receives a new revision. Clearing the setting removes the value and visible
authority state but retains the durable revision high-water mark, so recreating a setting or label
cannot turn a delayed request into authority for a different origin.

Approval is exactly `proposed -> approved` at the expected revision. It is not an idempotent command
over every value-bearing state: replay after approval refuses, and `revoked -> approved` refuses even
at the same revision. Reactivation requires a replacement proposal with a strictly higher revision,
followed by approval of that new proposal. Revocation accepts only the states for which the plan
publishes it and checks the expected revision in the same store transaction. Every persistence
failure restores both the in-memory lifecycle record and revision high-water mark. All states and
transitions survive restart.

The value-free plan marks custom-origin fields generically and publishes only lifecycle state,
revision, and actions valid for that state: `unset` can be proposed; `proposed` can be approved,
replaced or revoked; `approved` can be replaced or revoked; and `revoked` can only be replaced. The
initial proposal and replacement use the field's existing plan submission target; a replacement
must carry the plan-published `expected_revision` alongside the new value. The configured operator
check is repeated server-side for replacement and every activation transition. The console renders
only the published actions and therefore never offers Approve for `revoked`.

The authority address is
`/api/connections/{connector}/instances/{label}/settings/{service}/{field}/authority`. A separate
`GET` at that address is guarded by the deployment's configured operator authority and returns the
exact normalized origin for the current proposal so the approving operator can inspect what would
become active. An ordinary eligible connection owner receives only the value-free plan, and a
Service Account receives neither surface. `PUT` at the authority address approves and `DELETE`
revokes; each names `exchange.connection-plan.v1` and the canonical decimal expected revision. The
route derives the tenant from the resolved principal and treats connector, label, service and
declared field only as catalogue/registry keys. Approval and revocation accept no origin value.

For a custom origin, the field's existing `set` flag means runtime-effective and is true only in
`approved`. A required proposed or revoked field therefore keeps the overall plan incomplete. The
composite apply step reports that the proposal persisted and explicit approval remains, rather than
calling the connection complete.

The field's `authority` object is closed: `state` is `unset`, `proposed`, `approved` or `revoked`;
`revision` is `null` only for `unset`; and its action map contains only the state-specific methods
above. A revision is a canonical decimal string rather than a JSON number, so a JavaScript client
cannot silently round the store's `u64`: it starts at `"1"`, contains ASCII digits only, has no sign
or leading zero, and must parse within `u64`. A successful value-free mutation response repeats only
version, connector, label, service and field plus the action and
`authority: { state, revision }`. A stale revision, ineligible declaration or invalid state is a
value-free refusal and does not mutate anything.

Every authority transition has a value-free audit protocol. Exchange prepares an audit event naming
the action and revision before entering the durable mutation; if audit begin fails, it mutates
nothing. The begun and finalized record and all related logs contain the value-free action and
revision, but never origin, label, credential, submitted value, authorization material or another
credential-shaped value. After durable mutation, failure to finalize audit is not a generic
service-unavailable refusal: the response is an explicit partial/may-have-happened outcome naming
the value-free action and revision so a client knows which state to re-read. Runtime convergence is
still attempted after a committed mutation when audit finalization fails; an audit backend failure
cannot be allowed to preserve authority the durable state has revoked.

The existing settings file is a legacy unversioned map of plain strings. Binding accepts that exact
shape without rewriting it. The first explicit mutation persists
`exchange.connection-settings.v2`, its next origin revision and its values; ordinary leaves remain
strings while custom origins are strictly tagged records. A legacy ordinary string at an address
the current typed catalogue marks as custom-origin refuses startup and names the derived address,
never the value: an old value is not inferred approved, and explicit cleanup/resubmission is safer
than a silent migration.

There is one deliberate legacy tagged-record migration for the already-written pre-normalization
X-125 shape: a `custom_origin` record with value, state and revision but no normalized-origin member
is accepted only when the current released connector rule recognizes its derived address and can
validate and normalize its value. The normalized result is persisted by the next explicit mutation;
an absent rule or invalid legacy value refuses without naming the value. This exact migration shape
is tested and is not a general compatibility fallback. The v2 reader is otherwise forward closed:
unknown schema versions, root fields, record kinds, record fields, authority states, malformed or
non-increasing revision high-water marks and unknown high-water shapes all refuse startup rather
than being treated as legacy, dropped or repaired.

Invocation reads a custom-origin value only in `approved`, after revalidating it through the current
connector-shared typed rule. Proposed and revoked values are indistinguishable from missing
configuration at the runtime port, so request construction and permission subjects observe the same
normalized snapshot; a declaration change never demotes a tagged record into an executable ordinary
string.

Proposal/replacement, approval, revocation and clear invalidate long-lived runtime projections. A
cancellation signal or a status change is not termination proof. Before a successful response, the
coordinator awaits an acknowledgment that the old connector runtime has actually terminated, only
then starts or projects the replacement state, and awaits acknowledgment that this final projection
has landed. The replacement may be a newly approved normalized origin or the intentionally inactive
projection for proposed, revoked, cleared or unset state. If durable authority mutation succeeded
but termination or final projection cannot be confirmed, the route returns the same explicit
partial/may-have-happened outcome naming action and revision; it never returns generic `503` or
success. Already-dispatched one-shot work may complete, but no successful transition leaves an old
long-lived runtime retaining authority.

`exchange.connection-plan.v1` remains the only accepted request version. `GET` defaults an omitted
version to v1 and refuses an explicitly unsupported one. Composite and authority writes naming
another version refuse before semantic processing or any write, with a dedicated `422`
carrying `unsupported_connection_plan_version`, `requested` and `supported` — never an embedded v1
plan that could be mistaken for the requested contract. A consumer receiving another response
version refuses it before rendering or submitting; the console tests that closed-version check.

Catalogue 0.18 does not yet publish the typed custom-origin rule delivered by upstream C-87. The
dependency-independent lifecycle and wire contract can be developed against a vendor-neutral test
rule, but the production projection stays fail closed until connector 0.19.0 is published and
Exchange consumes that released declaration. No connector id, vendor field, local boolean, sibling
path dependency or git dependency substitutes for that release seam. The connector-shared rule owns
value grammar, normalization, API-path composition and HTTPS requirements; the egress
private-network/DNS guard remains a separate dispatch control.

That release boundary changes Exchange's connector pins and compatible Flux engine line together as
required by the repository dependency contract. It does not create the inverse coupling: Flux pins
the connection-plan protocol version it understands, not an Exchange or connector release number,
and caller input never selects a compatibility or runtime line.

## Consumers

The console renders only the versioned rows and targets the service returns. It keeps entered values
in DOM controls until submission and holds only the value-free result afterwards. `secret` rows are
password controls, closed rows are selects, and all other rows are text-like inputs derived from
`input`. Existing labels are selectable and the selected `name` remains editable.

Flux can consume the same JSON contract and target identities. Scriptable aliases such as `--site`,
`--domain`, or `--endpoint` may resolve to a published field identity, but are not schema. A vendor
secret has no argv representation: a non-interactive client must use secure stdin/prompt or an
Exchange-owned browser handoff. The committed contract fixture is intentionally vendor-neutral and
is exercised by the production server projection and browser client here. The cross-repository Flux
CLI proof is scheduled under its own consumer story so neither repository claims a future client as
evidence for its current completion.

## Evidence at the release seam

The dependency-independent tests prove tenant derivation from two resolved principals, rollback of
both state and revision on persistence failure, the ordinary human's value-free plan, Service
Account denial, the configured operator's exact normalized proposal, approval replay refusal after
revocation, stale and concurrent replacement refusal, and the termination-before-projection
barrier. Persistence fixtures separately prove the one deliberate tagged-record migration and that
unknown v2 state, kind, field and high-water inputs refuse.

The shared `exchange.connection-plan.v1` fixture carries the complete alias arrays and adversarial
duplicates. The server projection and console parser consume that same artifact: missing, malformed
or duplicate aliases are refusals, not client defaults.

The final production proof waits for published connector 0.19 rather than a sibling checkout. It
exercises unsupported, malformed and normalized origins through the released typed rule and the
real `connector_pack`: proposed and revoked values are inert and absent from permission subjects,
intents and dispatch evidence, while only the approved normalized value becomes active. Only after
that proof moves the connector and compatible Flux engine pins together does the repository run its
full gate. This evidence does not make Flux depend on an Exchange or connector release number.
