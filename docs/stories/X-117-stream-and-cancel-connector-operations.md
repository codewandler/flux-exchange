---
id: X-117
title: "Stream and cancel connector operations over the connector WebSocket"
status: backlog
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "extend the bounded subscribe connection with request-correlated logs/stdout/socket output, cancellation and terminal status; no second WebSocket"
---

# Stream and cancel connector operations

## Goal

Carry long-running connector output and cancellation over the same authenticated connection used for
inbound events, without unbounded queues, implicit replay or orphaned runtime work.

## Acceptance

- [ ] X-113 frames start, chunk, cancel and terminate a stream with closed request correlation and
      bounded payload/queue/in-flight limits.
- [ ] Slow consumers are isolated; overflow cancels or disconnects according to the declared policy
      and never blocks the connector supervisor globally.
- [ ] Client disconnect, grant revocation, connection rotation and explicit cancel reach the runtime
      and produce one terminal outcome.
- [ ] Secret-bearing stdout/diagnostics are redacted at the worker and again at Exchange before logs,
      activity or frames.
- [ ] Failing-first tests cover `logs -f`, process stdout and a socket read loop, including cancellation
      races and no replay after reconnect.

## Progress

- (not started)
