# flux-exchange docs

Start here to find anything inside the repository. These are the internal contributor docs: vision,
roadmap, story status, design records, and notes. Work is managed through the installed **Flux Board**
CLI — see [AGENTS.md](../AGENTS.md) → **"Start here"** for its copyable JSON workflow.

## Map

| If you want… | Read |
|---|---|
| **The shared domain vocabulary** — Connector, App, Agent, Datasource, Trigger and Event Delivery | [concepts.md](concepts.md) |
| Why the project exists; the principles | [vision.md](vision.md) |
| Threat model, security controls, limitations and incident response | [security.md](security.md) |
| Status + what's next; the epics | [roadmap.md](roadmap.md) |
| **What to work on right now** | [stories/README.md](stories/README.md) — the backlog/status board |
| The detail of a specific story | `stories/<ID>-<slug>.md` |
| Design records for non-trivial work | [designs/](designs/) |
| Finished / superseded material | [archive/](archive/) |
| Released history | [../CHANGELOG.md](../CHANGELOG.md) |

## Working here

Every contributor — human or agent — starts at [AGENTS.md](../AGENTS.md) → **"Start here"**. Use
`flux board --root . next --limit 1 --output json` to select ready work, or inspect the item the user
named; use the guarded transition, evidence, done, check and sync commands documented there. New or
unscoped work is created through the Board CLI so the next agent inherits its context. The released
Exchange runtime and binary are Linux-only on `aarch64-unknown-linux-gnu` and
`x86_64-unknown-linux-gnu`; historical stories may describe superseded platform work.
