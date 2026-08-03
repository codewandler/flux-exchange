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

## Consumers

The console renders only the versioned rows and targets the service returns. It keeps entered values
in DOM controls until submission and holds only the value-free result afterwards. `secret` rows are
password controls, closed rows are selects, and all other rows are text-like inputs derived from
`input`. Existing labels are selectable and the selected `name` remains editable.

Flux consumes the same JSON contract and target identities. Scriptable aliases such as `--site`,
`--domain`, or `--endpoint` may resolve to a published field identity, but are not schema. A vendor
secret has no argv representation: a non-interactive client must use secure stdin/prompt or an
Exchange-owned browser handoff. The committed contract fixture is intentionally vendor-neutral. The
server projection and browser client exercise it here. X-125 is not complete until the actual Flux
CLI consumer also exercises that same artifact; a future consumer is not evidence that the present
acceptance passed.
