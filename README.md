# flux-exchange

The platform layer of the [flux](https://github.com/codewandler/flux) family: a service that holds
credentials, terminates channels, runs operations for many callers, and records what happened.

Its primary caller is an **agent**, not a human. People sign in to wire things up and to see what
happened; agents are what call operations all day. That inverts the usual assumption and shapes
everything below.

> [!WARNING]
> **Status: v0.0.1 — a charter, a type system, and an HTTP surface with exactly one route.**
>
> `cargo run` now binds `127.0.0.1:8080` and answers `GET /health`. It refuses to start on a
> reachable address while no identity provider is configured. Nothing else is served: there is no
> sign-in, no catalogue route, no connection, and no `invoke`. It holds no credential — the store
> exists as a library binding and no binary holds one yet. See
> [What exists today](#what-exists-today) for the honest inventory before planning around any of
> this.

## Why it exists

flux runs on your machine with credentials in your environment. That stops working the moment you
want a team to share an integration, an agent to use it unattended, or an auditor to ask what
happened.

[flux-connectors](https://github.com/codewandler/flux-connectors) describes what vendors can do.
flux runs it. Neither of them holds a credential on anybody's behalf, and neither should — that is
a third job, and this is it.

The boundary, one question each:

- Does it change what happens when an effect executes? → **flux**
- Is it true of the vendor regardless of who runs it? → **flux-connectors**
- Does it require holding a credential or knowing a tenant? → **flux-exchange**

## The rules this repository enforces

Three claims from the [ecosystem design](https://github.com/codewandler/flux/blob/main/docs/designs/ecosystem.md)
are executed here rather than merely written down. Each has tests.

**The credential never crosses the boundary; the authority does.** Outbound, a caller names an
operation and gets a result — it cannot name a host (the URL comes from the operation's own compiled
Flux), cannot name a credential (the address is derived from the session's tenant and the
connector's declared authority), and cannot name a tenant (that is read from the resolved principal
and from nothing a caller controls). Inbound, a vendor's signed payload is verified here and the
caller receives a typed, declared event. In neither direction does a caller come to hold a value it
did not already have.

**The runtime is declared by the connector, never chosen by the caller.** A caller who can pick the
runtime is a caller who can pick an effect. There is deliberately no constructor on `Runtime` that
takes caller input.

**A locally-executing runtime cannot be safely multi-tenant in one process.** HTTP is shareable
because the effect leaves the machine; process spawning, container exec and raw sockets consume this
host's own identity, network position and filesystem. So a shared deployment **refuses** them, and
the refusal names what would have worked:

```
SingleTenant
  serves:  http, socket, process, container, plugin, remote
  refuses: nothing
MultiTenant
  serves:  http, remote
  refuses: socket, process, container, plugin  (they execute on this host)
```

**A grant selects operations by declared metadata, not by name.** A grant written as a list of ids
is a list somebody maintains, and it stops covering a connector the moment that connector gains an
operation. `risk <= low` covers the new one correctly on the day it lands. An agent's token grants
access to *operations*, never to credentials — so a stolen token yields a bounded operation set
against one tenant's connections, not a vendor secret.

## What exists today

| | |
|---|---|
| `crates/exchange-host` | The vocabulary and the rules, as ports. `Principal`/`Tenant`, `Grant`/`Selector`, `Runtime`/`Deployment`, `Lease`, and the `Identity` trait. **Real and tested (19 tests).** |
| `crates/exchange-server` | A binary that reports what it would serve and exits. **Not a service.** |
| `console/` | A Vue 3 console rendering **fixture data**, reusing the framework-free explorer components from flux-connectors. **No backend.** |

**Not built, despite being described in the design:** sign-in, any HTTP route, the credential store,
`invoke`, `subscribe`, the websocket, channels, leases-in-anger, stored workflows, execution
records, and the catalogue loader. The design is ahead of the code on purpose; the gap is stated
here so nobody has to discover it.

## Try it

```bash
cargo run                       # prints the deployment matrix above
cargo test --workspace          # 19 tests
cd console && npm install && npm run dev
```

Rust 1.87 or newer.

## Embedding it in your own product

`codewandler-flux-exchange-host` publishes the host as a **crate**, not only a binary. Identity, the
secret store, the transport and the runtime registry are all traits, so a product composes them into
its own service with its own identity provider and its own runtimes — without forking anything, and
without that product's concerns reaching the shared code.

That is the point of the trait boundary, not a side effect of it: the public crate has no downstream
dependency to leak through, because it has only traits.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
