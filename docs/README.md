# flux-exchange docs

Start here to find anything inside the repository. These are the internal contributor docs: vision,
roadmap, story status, design records, and notes. Work is tracked with the **track** framework — see
[AGENTS.md](../AGENTS.md) → **"Start here"** for the working loop.

## Map

| If you want… | Read |
|---|---|
| Why the project exists; the principles | [vision.md](vision.md) |
| Threat model, security controls, limitations and incident response | [security.md](security.md) |
| Status + what's next; the epics | [roadmap.md](roadmap.md) |
| **What to work on right now** | [stories/README.md](stories/README.md) — the backlog/status board |
| The detail of a specific story | `stories/<ID>-<slug>.md` |
| Design records for non-trivial work | [designs/](designs/) |
| Finished / superseded material | [archive/](archive/) |
| Released history | [../CHANGELOG.md](../CHANGELOG.md) |

## Working here

Every contributor — human or agent — starts at [AGENTS.md](../AGENTS.md) → **"Start here"**: open the
[board](stories/README.md), take the top `ready` story by priority, follow the loop, keep the gate
green. New or unscoped work? Create a story first (`/track:story`) so the next agent inherits the
context. After any change to a story's status/priority/title/epic/note, run `/track:board`. Optional
story `areas` are query-only subsystem tags for selection, e.g. `/track:next flux-lang`.
