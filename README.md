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
| `crates/exchange-host` | The vocabulary and the rules, as ports. `Principal`/`Tenant`, `Grant`/`Selector`, `Runtime`/`Deployment`, `Lease`, the `Identity` trait, and `CredentialStore` — a file-backed credential store, bound but not yet wired into a binary. **Real and tested (32 tests).** |
| `crates/exchange-server` | A service with exactly one route: `GET /health` on loopback. It refuses to start on a reachable address with no identity provider. **Tested (13 tests).** |
| `console/` | A Vue 3 console reading the **live catalogue** from this service, reusing the framework-free explorer components from flux-connectors. An unreachable service renders an error naming the endpoint — never an empty catalogue. |

**Not built, despite being described in the design:** sign-in, every route but `/health`, `invoke`,
`subscribe`, the websocket, channels, leases-in-anger, stored workflows, execution records, and the
catalogue loader. The credential store has moved off this list and is described below — with the
caveat that it is a library binding no binary holds yet, which is a shorter distance from "not
built" than the section heading suggests. The design is ahead of the code on purpose; the gap is
stated here so nobody has to discover it.

### The credential store, and what does not protect it

`exchange_host::CredentialStore` binds the file-backed store from `connector-secrets` rather than
reimplementing one: a `0600` file in a `0700` directory, both modes set in the `open(2)`/`mkdir(2)`
call and **re-checked every time the store is opened**. A widened mode is refused, never quietly
tightened — the file already had that mode while it held values, so tightening it would hide the
exposure instead of reporting it. A path inside a working tree is refused outright, because a
credential under a checkout is one `git add -A` from being committed — and the path is resolved
through every symlink and every `..` before that check, so what is inspected is where the store
would land rather than how it was spelt. A write is a whole-file
rewrite through a sibling temporary, `fsync` and `rename(2)`, so a crash mid-write leaves the
previous file whole rather than truncated, and a delete rewrites immediately, so a revoked
credential does not come back on restart.

**What protects a value there is that file mode and nothing else.** There is no encryption at rest,
no passphrase, no OS keychain integration, and no protection from `root` or from a backup that
copies the file. That makes it the right store for a single-operator deployment and the wrong one
for a shared machine, where `connector-secrets`' Vault-backed store is the answer. Nothing ever
silently selects the in-memory store instead: a configuration naming no path is a **startup error**
naming what would have worked, because a host that fell back would start, serve every route
correctly, look exactly like a working one, and lose every credential on restart.

To decommission a store, remove the **directory**, not the file — a write interrupted between the
`fsync` and the `rename(2)` can leave a complete copy of every credential in a sibling temporary
that `rm` on the store file alone does not touch.

No binary binds it yet: the server serves `/health` without ever opening a store.

## Try it

```bash
cargo run                       # binds 127.0.0.1:8080, answers GET /health
cargo test --workspace          # 45 tests
cd console && npm install && npm run dev
```

Rust 1.87 or newer.

## Embedding it in your own product

`codewandler-flux-exchange-host` publishes the host as a **crate**, not only a binary. Identity, the
secret store, the transport and the runtime registry are all **ports**, so a product composes them
into its own service with its own identity provider and its own runtimes — without forking anything,
and without that product's concerns reaching the shared code.

One implementation ships behind one of those ports: `CredentialStore`, the file-backed store above.
It is a default, not a fixture — the port is `SecretStore`, a deployment that wants Vault or its own
backend binds that instead and never constructs the type, and the store it wraps comes from
`connector-secrets`, a flux-family crate, rather than from any product.

That is the point of the boundary, not a side effect of it: the public crate has no **downstream**
dependency to leak through. Traits are how that is kept true, and a default a composing binary can
decline does not spend it.

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
