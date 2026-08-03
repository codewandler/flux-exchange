# Design: durable audit evidence

**Status:** accepted · **Story:** X-95

## Decision

Exchange writes a typed audit journal to SQLite on operator-selected durable storage. The server
composition reads `FLUX_EXCHANGE_AUDIT`; the Fly deployment binds it below the existing encrypted
volume at `/data/audit/events.sqlite3`. A reachable bind with no journal refuses before opening its
socket. Loopback remains usable without durable evidence and says that it is doing so at startup.

This is an application audit journal, not a copy of ordinary logs and not a future execution-record
model. The latter may retain a connector invocation's declared inputs, outputs or trace vocabulary;
this journal records only who exercised or was refused authority, what non-secret address was
affected, and how the host answered.

### One closed record vocabulary

Every row serialises as one JSON object with these fields:

- `schema_version`, fixed at `1` until a deliberate migration;
- server-generated `event_id` and `request_id`;
- an RFC 3339 UTC `timestamp`;
- closed `action` and `outcome` spellings;
- optional `actor` containing separate `tenant`, `kind` and stable `id` fields;
- a closed, action-specific `target` object; and
- an optional alert `count` and `window_seconds` for threshold events.

There is no catch-all metadata map, message, request body, header, token, OIDC value, credential or
setting value field. Route adapters receive narrow constructors for their action and target. A new
kind of evidence therefore changes this type and its sentinel scan before it can reach the journal.

The actor is absent only when authentication did not resolve one. A refusal after identity
resolution retains the actor. Caller-facing responses do not gain any audit detail; the
server-generated request id is returned only as `x-request-id`, so an operator and caller can name
the same request without turning a refusal into an oracle.

### Correlation surrounds the guards

An outer middleware generates one request id before authentication and puts it in request
extensions. The identity guard records successful and refused authentication against that id.
Handlers create a distinct event id for each administrative or invocation outcome. Nested events
from one request share the request id and never reuse an event id.

For an authority-changing operation, the journal first inserts an `attempted` event before the
underlying store or runtime is touched, then transitions that same event to `succeeded` or
`refused`. If the initial insert fails, the action refuses without being exercised. If the final
transition fails after the action, the durable `attempted` row remains as explicit evidence that
requires investigation; the caller receives an audit-unavailable response rather than a false
success. SQLite transactions make each journal transition atomic, while deliberately making no
claim that two independent stores share a transaction.

### Storage, retention and query

The journal directory is created at `0700` and the database at `0600`; an existing path with wider
permissions refuses and is never repaired. SQLite uses WAL mode so the running server and an
operator query can coexist. The process deletes records only when their timestamp is more than 30
days old, at open and on the first write after each daily maintenance boundary. Thirty days is the
minimum, not a configurable shorter value.

`flux-exchange audit-query` opens the configured database read-only and emits newline-delimited
JSON. It accepts exactly one of `--event-id`, `--actor <tenant/kind/id>` or `--target <kind/value>`,
plus a bounded result count. The implementation API carries the same three query shapes for tests.
It has no HTTP route: in the Fly composition, readers authenticate to Fly and run it through `fly
ssh console`; tenant callers do not gain an audit-enumeration surface.

The `exchange` uid can append, transition and age out records. A Fly organization member with SSH
access can query them. Only that runtime uid or a Fly organization administrator able to replace or
destroy the volume can delete evidence before retention. The deployment runbook names both powers.

### Alerts are audit records, not a vendor adapter

The journal evaluates three fixed rolling-window policies after each matching event:

- authentication refusals: 20 in five minutes;
- authorization refusals: 10 in five minutes for one resolved actor; and
- any credential or grant change: one event immediately.

Crossing a threshold appends a separate `alert_raised` record carrying the triggering event id,
policy, count and window, then emits that record at `warn` through tracing. A policy is re-armed only
after its window has passed, preventing one flood from producing one alert per request. Alert
records contain identifiers and counts only.

This host does not send email, Slack or a webhook. Such a sender would be a second Exchange-owned
outbound adapter, contrary to the connector/runtime boundary. Fly already captures stdout; the
retained journal is the 30-day source of truth and the warning stream is the operator notification
surface. Deployment verification proves the alert record and warning exist without claiming that
Fly's shorter searchable-log retention is the journal retention.

## Failure policy

- An unusable configured journal refuses startup and names the path, never a row.
- A reachable deployment with no journal refuses before binding.
- An initial audit write failure refuses the authority-changing operation before its store/runtime.
- A failed final transition leaves `attempted`, logs the event id, and does not answer success.
- Retention failure is visible and does not delete additional rows or stop new writes.
- Query failure names the database and query kind, never record values.

## Verification

Failing-first tests:

1. parse emitted JSON and require every schema field and stable spelling;
2. carry one request id across authentication and an invocation success/refusal while event ids stay
   distinct;
3. submit one sentinel through a token, cookie, OIDC code, credential value, setting value and body,
   then scan every JSON field name and value for it;
4. restart the journal, query by event, actor and target, and retain a 30-day boundary row while
   removing an older one;
5. refuse widened directory/database modes and injected write failures; and
6. trigger each alert policy and prove its record contains only identifiers, counts and the fixed
   policy spelling.

Live verification deploys a versioned build, performs one safe refused request, queries its event by
id through Fly SSH, restarts the machine and queries it again. No credential-shaped fixture is sent
to production.

## Rejected alternatives

- **Ordinary Fly logs as the journal.** Fly currently retains searchable logs for seven days, below
  the story's 30-day contract, and log retention is deployment state rather than this process's
  fail-closed evidence path.
- **An unrelated Loki, Prometheus or company account.** Flux family repositories do not couple to a
  downstream company's infrastructure, and no such endpoint is an authority to store this service's
  principal activity.
- **A new hosted logging vendor.** It adds an account, credential, bill and deletion authority when
  the deployment already has encrypted durable storage. The typed local journal satisfies the
  retention and query contract without moving evidence across another boundary.
- **A generic JSON metadata map.** It makes request bodies and credential values representable and
  reduces “never material” to review discipline.
- **An authenticated audit HTTP endpoint.** A tenant principal is not an operator, and X-91 has not
  yet established an operator role. Fly SSH is the existing operator boundary.
