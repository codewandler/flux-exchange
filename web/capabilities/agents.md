---
capability: agents
---

# Agents

An **Agent** is a model plus an authored loop plus bounded operation and datasource capabilities. A
**Managed Agent** is that declaration hosted inside an installed Flux App.

An Agent is not a Service Account. The Service Account is a durable API principal that can
authenticate a piece of automation; it acquires no model, loop or capability merely by existing.
The Agent is the runtime that uses bounded capabilities, and may authenticate to Exchange through a
Service Account when it calls the remote surface.

## The implementation contract

[X-108, “Host installed Flux Apps and Managed Agents”](https://github.com/codewandler/flux-exchange/blob/main/docs/stories/X-108-host-installed-flux-apps.md)
owns the implementation: installed package revisions, reviewed bindings, lifecycle and the boundary
between an App's declaration and its hosted Agent runtime.

The status above comes from the build's onboarding descriptor. This page defines the intended
concept and names the tracked implementation; it does not turn that intent into an availability
claim.
