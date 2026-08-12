---
id: X-153
title: "Load the catalogue as a pack, including one newer than this binary"
status: ready
priority: 3
epic: catalog-artifact
areas: [exchange-host, exchange-server, deployment]
note: "the reader's `load` constructor is new capability, not a migration: a deployment can serve a catalogue newer than the binary it was built with, so a new provider stops requiring an Exchange release"
---

# Load the catalogue as a pack, including one newer than this binary

## Goal

Consume the connector catalogue as a versioned, digest-carrying pack through the dependency-free
reader, and gain the thing that unlocks: **a deployment can serve a catalogue newer than the binary
it was built with.**

## Why this is more than a migration

The compatible half is invisible — `docs/designs/catalog-artifact.md` says
`codewandler-connector-catalog` becomes *"a shim embedding the pack and re-exporting the reader
without breaking its public API"*, so `catalog::providers()` and friends keep working and Exchange
could adopt it by doing nothing.

The half worth a story is this:

> *Hosts that want the file at a path (Exchange loading a newer catalogue than it was built with) get
> a `load` constructor that verifies schema version and digest before serving a single record.*

Today a new provider, or a corrected vendor quirk, reaches this service only through a crates.io
release and an Exchange rebuild. Decision 0022 breaks that — *"adopting new connectors requires
neither a Flux release nor a consumer code change"* — but only for a host that actually loads a pack
from a path. That is this story.

## Acceptance

- [ ] Exchange reads the catalogue through the reader over the embedded pack, with the existing
      `catalog` API surface unchanged and no behavioural difference — proven by the X-152
      characterization output, not by inspection.
- [ ] A deployment may point Exchange at a catalogue pack on disk. Schema version and digest are
      verified **before a single record is served**, and a pack that fails either is refused at
      startup naming which check failed — never partially loaded, never silently ignored in favour of
      the embedded one. *Refuse; never repair.*
- [ ] A pack whose **major** schema version this build does not understand is refused, with the
      refusal naming both versions. Additive minor differences load. The design makes this the
      schema's own rule; Exchange enforces its side of it.
- [ ] The configured path is deployment configuration read at startup, never derived from a request
      — the same rule every other store binding in this service follows.
- [ ] The onboarding descriptor and `GET /api/catalogue` report **which** catalogue is being served —
      embedded or loaded, and its digest — so an operator debugging a missing operation can tell
      which catalogue answered without reading a log.
- [ ] Failing-first tests: a truncated pack, a digest mismatch, a major-version mismatch, and a path
      that does not exist. Each refuses distinguishably; none falls back.
- [ ] The `no_second_request_path` allow-list is unchanged, or the reader's entry carries a written
      sentence saying why a catalogue reader is not a transport.

## Progress

- 2026-08-12: Filed against Decision 0022 / C-537 (the pack and reader) and C-539 (Exchange's
  adoption). Blocked on the reader existing as a published release.

- 2026-08-12: Promoted to ready by the cross-repo coordinator: flux-connectors v0.23.0 published
  the complete surface this story consumes (documents, pack, reader, DocumentRehearsal, plan
  API) — the upstream blockers recorded above are closed. X-155 lands the pins first.

## Notes

- Read [[X-151]] for the epic's scope.
- The container format is upstream C-537's to finalise; the properties this story depends on are the
  ones the design fixes regardless — one file, versioned schema, embedded digest, deterministic
  bytes, no network and no filesystem walk at query time.
- Size is not a concern worth designing around: the design measures today's whole catalogue crate at
  ~186 KB compressed.
- This story is what makes the catalogue a **data** dependency rather than a code one. Worth stating
  in the release notes when it lands, because it changes what an operator has to do to get a new
  provider — and that is the user-visible half of Decision 0022.
