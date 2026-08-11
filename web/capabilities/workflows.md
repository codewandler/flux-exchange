---
capability: workflows
---

# Workflows, and where they live

What an operator calls a workflow—a flow of operations with conditions, triggers or schedules—is a
stored, versioned, tenant-local **Flux Program**. `GET /api/workflows` is the collection entry point
for its drafts and immutable published versions.

## Exchange stores the Program; Flux gives it meaning

flux-exchange does not run triggers or schedules. It stores, validates and publishes the Program,
resolves its tenant-bound requirements, and asks Flux to execute it through the same operation and
grant boundary as a vendor operation. Trigger, condition and schedule semantics belong to the
Program and the Flux runtime, not to a second orchestration language in this service.

That boundary preserves one model for determinism, typing, replay, approval and risk derivation. A
visual editor is a projection over the Program; it does not create another executable format.

## Publication is the durable seam

A draft may change. Publishing freezes an immutable version together with the requirements derived
from it. Runs name that version, and structural activity records node identity and lifecycle without
recording invocation arguments, results or credentials.

To a caller, a published composed operation remains an operation: it is selected through the
catalogue, admitted through grants and invoked through [`invoke`](/capabilities/invoke). The caller
does not need to know whether the implementation came from a Connector declaration or a composed
Flux Program.
