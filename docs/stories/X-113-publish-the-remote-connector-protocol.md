---
id: X-113
title: "Publish the effective Service Account catalogue and HTTP invoke contract"
status: done
priority: 0
epic: rich-connector-runtimes
design: docs/designs/rich-connector-runtimes.md
note: "Milestone 1 — authenticated connected-and-granted operation projection with stable generation identity beside the existing one-shot HTTP invoke; lifecycle remains X-117/X-118"
---

# Publish the effective Service Account catalogue and HTTP invoke contract

## Goal

Define the independently shippable HTTP contract that lets Flux discover and invoke the operations
one resolved Exchange Service Account can actually use. It covers the authenticated effective
Service Account catalogue and existing one-shot HTTP invocation only, without exposing a credential,
tenant, endpoint, runtime placement or other caller-selected authority.

The milestone surface is the authenticated effective Service Account catalogue plus the existing
one-shot invoke route.

## Acceptance

- [x] An authenticated HTTP catalogue returns exactly the connected and granted operations effective
      for the resolved Service Account; it is neither the anonymous full catalogue nor an
      operator-management surface.
- [x] The projection carries a stable generation identity: unchanged effective operations and
      declarations retain it, while a relevant connector, connection or grant change replaces it.
- [x] The existing one-shot HTTP invocation accepts only an operation id, operation arguments and
      the existing tenant-local connection label; tenant and grants come from the resolved Service
      Account, with no credential or caller-selected authority.
- [x] Unknown, disconnected, not-granted, ambiguous-connection, refused, unreachable and
      runtime-failed outcomes remain distinct, bounded HTTP responses.
- [x] Contract tests consumable by Flux C-503 prove authenticated discovery, generation changes,
      read invocation, approved-write invocation and malformed or unauthorized requests failing
      closed. Streams, cancellation and terminal outcomes remain X-117; leases remain X-118.

## Progress

- 2026-08-03: X-124 reduced this story to the first useful HTTP milestone. Long-lived protocol work
  moved back to X-117 and X-118 instead of blocking effective discovery and one-shot invocation.
- 2026-08-03: Added canonical Service Account discovery at `GET /api/catalogue/effective`, a stable
  content generation over usable operation/connection bindings, exact credential/configuration and
  grant intersection, and a distinct disconnected refusal on the existing invoke path. The
  assembled HTTP contract suite covers discovery, refresh, read/write invocation and fail-closed
  malformed and unauthorized calls; all Cargo, console and public-site gates pass.

## Notes

- Depends on X-124. Flux C-503 consumes this contract through its embedded native Exchange binding.
- X-129 binds stable local-release protocol identities to these delivered routes and production wire
  types. That follow-up versions the contract; it does not reopen or replace X-113's behavior.
- X-101…X-105's `/api/subscribe` framing remains the delivered starting point for X-117, not part of
  this story's acceptance.
