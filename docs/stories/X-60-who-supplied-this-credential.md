---
id: X-60
title: "An operator can find out who supplied a credential"
status: ready
priority: 2
epic: connections
areas: [exchange-server, exchange-host]
note: "found by X-54, 2026-08-01: nothing records who supplied a credential, which is half the reason X-54's kind gate was needed — and it means an operator cannot audit a substitution after the fact, including one another human made"
---

# An operator can find out who supplied a credential

## Goal
A connection can answer who put the credential there, and when.

## Why this surfaced

X-54 gated `POST /api/connections/{connector}` and `PUT .../credentials/{credential}` to humans,
because a caller who decides *which vendor account* a tenant's operations reach has been granted the
credential position whether or not it ever sees a value.

**Two of the three properties in that argument are absences, not defences:**

- **Nothing records who supplied a credential.** `GET /api/connections` answers `held: true`
  identically for a value the operator typed and a value somebody else substituted.
- **Revoking a principal's token does not take the value back out**, or point an operator at the
  address to check.

So the gate narrows *who can do it* and leaves *nobody can tell that it was done*. X-54 was right to
stop there — an audit record is a different thing from an access rule — but the gap it names is real
and it did not go away when the gate landed. X-54 closed the agent case. **It does not help at all
for the human case**, which `connection-settings.md` § *What this does not close* already names: a
signed-in human of the tenant who did not supply the credential is indistinguishable from one who
did.

## The design constraint this runs straight into

`docs/designs/connections.md` states, deliberately:

> A connection exists exactly when the store holds a value at one of the addresses derived for that
> tenant and connector. **There is no second source of truth to disagree with the credentials.**

An audit record *is* a second thing beside the store. [[X-14]]'s design already had to solve a
version of this for connection labels, and its resolution is the shape to reuse: **split what each
thing is authoritative for.** The store stays authoritative for *whether a credential exists*; a
record can only ever say *who last wrote one*, can never make a connection appear or disappear, and
its absence degrades to "unknown" rather than to "none".

The test that proves it is the same one: delete the whole record and assert every connection is still
listed and still usable, merely unattributed.

## Acceptance
- [ ] A connection reports who last supplied or rotated each credential, and when.
- [ ] **Failing-first test** — deleting the entire record leaves every connection listed and usable,
      attribution reading "unknown" rather than the connection vanishing.
- [ ] It records a **principal**, not a value, and never anything derived from the credential itself.
- [ ] Nothing tenant-specific leaks to an anonymous surface, and the attribution is visible only
      within the tenant it belongs to.
- [ ] `docs/designs/connections.md`'s "no second source of truth" claim is amended in place to say
      what the record is authoritative for and what it is not — the way §4 of
      `connection-settings.md` carries its own correction.

## Notes
- This is also what would make [[X-54]]'s DELETE decision reviewable: an agent deleting a connection
  is currently permitted on the argument that an operator sees it in `GET /api/connections`. That is
  true only if the operator was already looking.
- Bounds and file mode follow the credential store's, not the settings store's — X-47's review found
  `SettingsStore::bind` does not refuse a pre-existing widened mode where `CredentialStore` does.
