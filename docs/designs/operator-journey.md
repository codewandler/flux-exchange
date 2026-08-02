# Design: the operator journey

**Status:** accepted · **Story:** X-88 · **Release:** v0.13.0

## One journey, three server answers

The console presents Connect → Grant → Invoke as one progress strip on each of those screens. A step
is complete only from the latest `GET /api/connections` or `GET /api/grants` answer; a successful
write triggers those reads before the strip advances. Links carry the selected connector or
operation in fragment-local query state, never a tenant, address or credential.

Connection cards lead with an actionable state and hide address-level detail in a native
`<details>`. A searchable connector picker is shared by the Connect and Grants views and uses the
exchange catalogue's vendor, id and description plus the tenant's connection state. It remains a
form control with listbox semantics and full keyboard behavior, not a styled text filter followed by
an unrelated select.

Rotation is a per-held-credential form on the card. The input is deliberately uncontrolled, read
through `FormData`, submitted as `{ "value": "…" }` to the existing atomic `PUT` route and cleared
only after the refreshed connection listing proves success.

## Grants in operator language

Three modes produce the existing selector and nothing wider:

- **Read only:** `max_risk=low`, unbounded effects and idempotency.
- **No destructive effects:** no risk bound, every currently published effect except `delete` and
  `money`, any idempotency. Because `effects_within` is a subset bound, newly introduced effects are
  refused rather than silently admitted.
- **Custom:** the existing risk/effects/idempotency controls.

The server remains the only admission decision. Preview groups its returned operation facts by
service then risk and compares the admitted id set to the currently held grant for the same
connector: proper subset is narrower, equality unchanged, any added id wider. Save still waits for
the exact latest preview.

## Invocation and catalogue metadata

`GET /api/catalogue/connectors/{connector}/operations` additively publishes `input_schema`, copied
from `connector_pack::project(operation).input_schema`. That is the exact schema the runtime tool
uses and therefore is not a second parser. It is anonymous connector metadata and contains no
tenant, credential or configured endpoint.

The Invoke screen accepts one catalogue operation and its body as JSON. It starts from an object
containing every required top-level property with a type-appropriate empty value, validates JSON
syntax and the top-level required/type constraints the published schema states, and sends the value
verbatim—no envelope—to the existing invoke route. Results show elapsed client time, operation,
`is_error`, canonical content and optional view. Refusals preserve `sent`, `retryable`, message and
the service's `supply_at` remedy. No invocation result is presented as an execution record.

## Recovery, navigation and accessibility

App-owned reads expose retry callbacks to their failure views. Loading placeholders reserve the
same block shape as ready content. The shell's desktop rail remains; below 640px built links remain
visible and unavailable future surfaces move into a `details` disclosure carrying their reasons.

Finder matches wrap the exact case-insensitive substrings in `<mark>`, `/` focuses the one search
input unless the event starts inside an editable control, and operation pages link back to the
encoded prior finder state. Motion transitions are disabled under `prefers-reduced-motion`.

## Release

X-86, X-87 and X-88 are the v0.13.0 release contents. The implementation runs the repository's
Rust, console, web, security-audit and packaging dry-run gates; updates the story board and CHANGELOG;
commits the complete release tree; and creates/pushes `v0.13.0`. It never runs `cargo publish`.
