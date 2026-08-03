---
id: X-128
title: "Emit one trusted readiness record for a supervised Exchange"
status: ready
priority: 0
epic: remote-deployment
areas: [exchange-server, lifecycle, protocol, windows]
note: "Milestone 1 — Flux owns a child only after that exact process reports its bound address and compiled identity over a dedicated one-shot channel"
---

# Emit one trusted readiness record for a supervised Exchange

## Goal

Give Flux's local supervisor one versioned, bounded proof that the exact Exchange child it spawned
has bound its loopback listener and is compatible. The proof travels over a dedicated inherited
one-shot pipe or handle; neither a PID nor an HTTP health response can substitute for it.

## Why this is Milestone 1 work

`flux exchange local start` is short-lived, while the Exchange process has to survive it. Decision
0004 assigns lifetime ownership to a detached Flux supervisor and requires that supervisor to
commit ownership only after the child identifies itself. Polling `/health` can answer from a foreign
listener, and a PID can be stale or reused. Mixing readiness with stdout also lets ordinary logs
become lifecycle protocol by accident.

Exchange therefore owns a narrow supervised launch mode and readiness writer. Flux C-510 owns the
supervisor, authenticated control channel and later start/status/stop behavior.

## Acceptance

- [ ] The server has one documented supervised launch mode distinct from ordinary interactive and
      deployment startup. It requires a loopback bind with port `0`, lets the operating system
      choose the port, and refuses a non-loopback or preselected nonzero port before listening. The
      published address is read from the bound socket; there is no reserve-then-rebind port race.
- [ ] Supervised mode requires one dedicated inherited one-shot channel supplied by the parent: a
      pipe file descriptor on Unix and an inheritable pipe handle on Windows. It refuses an absent,
      malformed, non-inherited or wrong-kind channel. It never opens a caller-named filesystem path
      or a shared named pipe as a readiness fallback.
- [ ] After every required store and safety check succeeds and the socket has bound, Exchange writes
      exactly one UTF-8 JSON record of at most 16 KiB and closes the channel. The record has schema
      identity `exchange.supervisor-ready.v1` and contains only the actual loopback socket address,
      the child's OS process-start identity, and the compiled release/build and protocol identities.
      EOF before one complete record, a second record, trailing bytes, an unknown field required for
      interpretation or a record over the bound is a refusal for the supervisor.
- [ ] The process identity contains both the PID as a diagnostic and an OS-derived start identity
      that distinguishes PID reuse. The readiness contract never claims a PID alone proves
      ownership, and Exchange emits no PID file for Flux to trust. Native Unix and Windows tests
      compare the reported start identity with the handle/process the parent actually spawned.
- [ ] The release/build and protocol portion is produced from the same typed source as
      `flux-exchange compatibility --json` and agrees exactly for Exchange API, effective-catalogue,
      invoke request/response and `exchange.connection-plan` versions. Supervised startup does not
      infer a protocol from the package version, and the side-effect-free compatibility command
      still binds no listener and opens no store.
- [ ] The one-shot channel carries no log line, progress event, HTTP byte or later control traffic.
      Application stdout/stderr remain ordinary log streams, the bound socket carries application
      traffic, and Flux C-510's owner-only control channel is separate. Closing or losing the
      readiness reader cannot redirect the record to either stream.
- [ ] No credential, setting, grant body, Service Account token/verifier, session value or control
      credential is serializable in the readiness type. A sentinel test drives success and every
      startup refusal and proves the readiness bytes and captured stdout/stderr contain none of
      them.
- [ ] **Failing-first process tests:** a successful child emits one record only after bind; a store
      refusal or bind failure emits none; a foreign process answering `/health`, a planted PID file
      and a reused PID cannot satisfy the parent-side fixture; corrupt schema, address,
      process-start, release or protocol identity is rejected before lifecycle ownership is
      committed.
- [ ] Native Windows and Unix tests spawn the real server through the inherited handle/descriptor,
      validate the one-shot record against `compatibility --json`, connect to its reported address,
      then terminate it through the owned process handle. No test treats `/health` as readiness,
      though health may remain an independent application liveness endpoint after ownership exists.

## Progress

- 2026-08-04: Filed from the supervision amendment to cross-repository Decision 0004. The audit
  separated Exchange's launch/readiness protocol from Flux C-510's process supervisor and control
  channel.

## Notes

- Cross-repository authority:
  `../flux-roadmap/decisions/0004-flux-manages-a-verified-local-exchange.md` at `71fea6c`.
- X-126 depends on this story so the released executable and signed manifest carry the same
  compatibility identity the supervisor validates. Flux C-510 consumes the record and owns the
  long-lived supervisor; this story does not add a downloader, daemon manager or stop command.
- `/health` remains useful after identity is established. It is deliberately not process identity
  and cannot select which process Flux owns.
