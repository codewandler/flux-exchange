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

`GET /api/connections/{connector}/plan` is an operator-only read. It projects
`connector_catalog::Provider::config` and `Provider::auth` in declaration order. The response is versioned as
`exchange.connection-plan.v1` and starts its `fields` array with the synthetic `name` field. Every
remaining configuration row comes from one catalogue `ConfigField`, with its stable
service-qualified identity, human label, requiredness, input kind, declared binding, additional
bindings, closed choices, submission target, and a `set` boolean. No Exchange-owned connector or
vendor field list participates.

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

The settings port persists an authority lifecycle beside the value, under the same tenant,
connector, instance, service and declared field address:

- `unset` has no proposed value;
- `proposed` has a value and a non-reusable store-wide proposal revision, but the runtime cannot
  read it;
- `approved` records that an operator explicitly approved that exact revision, so the runtime may
  read it; and
- `revoked` keeps the proposed value for repair but makes it unreadable to the runtime again.

Every custom-origin setting write creates a new revision in `proposed`, even when the submitted bytes
match the prior value; a write never carries approval forward.
Clearing the setting removes the value and visible authority state but retains the store's durable
revision high-water mark. Recreating a setting or label therefore cannot reuse an old revision and
turn a delayed approval request into authority for a different origin. Approval is a
compare-and-set transition inside the settings store's existing write lock: it revalidates that the
current typed declaration requires approval and that the current proposal has the expected
revision, changes the state, persists, and rolls memory back if persistence fails. Revocation is
checked the same way. Both transitions survive restart.

The plan marks these fields generically and publishes their value-free state, revision, and
operator-only approve/revoke actions. On
`/api/connections/{connector}/instances/{label}/settings/{service}/{field}/authority`, `PUT`
approves and `DELETE` revokes the selected label's current matching proposal. Each action carries
only the plan version and proposal revision. The route remains under the deployment's existing
`OperatorPolicy`, derives the tenant from the principal, and treats connector, label, service and
declared field only as catalogue/registry keys. It accepts no origin value. Audit records name that
derived setting address and the transition, never the proposed value.

For a custom origin, the field's existing `set` flag means runtime-effective and is true only in
`approved`. A required proposed or revoked field therefore keeps the overall plan incomplete. The
composite apply step reports that the proposal persisted and explicit approval remains, rather than
calling the connection complete.

The field's `authority` object is closed: `state` is `unset`, `proposed`, `approved` or `revoked`;
`revision` is `null` only for `unset`; and value-bearing states publish the same approve and revoke
targets. A revision is a canonical decimal string rather than a JSON number, so a JavaScript client
cannot silently round the store's `u64`: it starts at `"1"`, contains ASCII digits only, has no sign
or leading zero, and must parse within `u64`. Both action bodies are exactly the plan `version` plus
that revision. A success response repeats only version, connector, label, service and field plus
`authority: { state, revision }`. A stale revision or ineligible declaration is a value-free refusal
and does not mutate anything.

The existing settings file is a legacy unversioned map of plain strings. Binding accepts that exact
shape without rewriting it. The first explicit mutation persists
`exchange.connection-settings.v2`, its next origin revision and its values; ordinary leaves remain
strings while custom origins are strictly tagged records. A legacy string at an address the current
typed catalogue marks as custom-origin refuses startup and names the derived address, never the
value: an old value is not inferred approved, and explicit cleanup/resubmission is safer than a
silent migration. Unknown root versions, record shapes, record fields and authority states refuse
startup rather than falling back to legacy, being dropped or being repaired.

Invocation reads a custom-origin value only in `approved`, after revalidating that the current typed
declaration still classifies that address as a custom origin. Proposed and revoked values are
indistinguishable from missing configuration at the runtime port, so request construction and
permission subjects observe the same snapshot; a policy change never demotes a tagged record into an
executable ordinary string. Proposal, approval, revocation and clear each restart the tenant's
generated channels for that connector, cancelling before a replacement plan can read settings. A
successful response waits for that cancellation/restart transition, so a long-lived channel cannot
retain revoked authority after success. Already-dispatched one-shot work may complete; revocation
governs later projections.

`exchange.connection-plan.v1` remains the only accepted request version. `GET` defaults an omitted
version to v1 and refuses an explicitly unsupported one. Composite and authority writes naming
another version refuse before semantic processing or any write, with a dedicated `422`
carrying `unsupported_connection_plan_version`, `requested` and `supported` — never an embedded v1
plan that could be mistaken for the requested contract. A consumer receiving another response
version refuses it before rendering or submitting; the console tests that closed-version check.

Catalogue 0.18 does not yet publish the typed custom-origin policy delivered by upstream C-87. It
can carry and test the generic lifecycle behind a fail-closed policy seam, but it does not activate
today's inferred whole-authority connectors as a substitute. The released projection activates the
lifecycle only after connector 0.19.0 publishes and Exchange consumes that typed declaration. No
connector id, vendor field, path dependency or git dependency substitutes for that release seam.
The declaration owns value grammar, API-path composition and HTTPS requirements. Exchange persists
and approves the exact submitted bytes; it invents no URL normalization or host allow-list, and the
egress private-network/DNS guard remains a separate dispatch control.

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
