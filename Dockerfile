# flux-exchange — the image a reachable deployment runs.
#
# Three stages, and the third carries no toolchain: a credential-holding service should not ship a
# compiler and a package manager it will never use.
#
# The console is built here rather than expected on disk. X-83 made the server serve it from a
# directory, and `FLUX_EXCHANGE_CONSOLE` below is what points at the copy this image carries — the
# console must be same-origin with the API, because the session cookie is `SameSite=Strict` and a
# browser never attaches one cross-origin.

# ─── the console ─────────────────────────────────────────────────────────────────────────────────
# Node 22/bookworm-slim, pinned to the 2026-08-03 OCI index digest. The readable tag says what to
# update; the digest says exactly what production builds. `npm ci` makes the lockfile authoritative.
FROM node:22-bookworm-slim@sha256:f32b81066cde10a75dbac96646099533316d94bac4150c55da1636e1f0ffdc46 AS console
WORKDIR /console
COPY console/package.json console/package-lock.json ./
RUN npm ci
COPY console/ ./
RUN npm run build

# ─── the binary ──────────────────────────────────────────────────────────────────────────────────
# 1.88 is the MSRV in `Cargo.toml`'s `rust-version`, and X-33's CI job builds against whatever that
# says. Do not raise this to make a build pass: the number is observed, not chosen, and raising it is
# a compatibility break for consumers of the published crate that belongs in the CHANGELOG.
# rust:1.88-bookworm, pinned to the 2026-08-03 OCI index digest.
FROM rust:1.88-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS build
WORKDIR /src

# The manifests first, so a source-only change does not re-resolve the dependency graph. Every
# member needs a manifest and a stub before `cargo build` will accept the workspace.
COPY Cargo.toml Cargo.lock ./
COPY crates/exchange-host/Cargo.toml crates/exchange-host/
COPY crates/exchange-server/Cargo.toml crates/exchange-server/
RUN mkdir -p crates/exchange-host/src crates/exchange-server/src \
    && echo '' > crates/exchange-host/src/lib.rs \
    && echo 'fn main() {}' > crates/exchange-server/src/main.rs \
    && cargo build --release --locked --bin flux-exchange \
    && rm -rf crates/exchange-host/src crates/exchange-server/src

COPY crates/ crates/
# The stub's artifacts are newer than the real sources on a cached layer, so touch the entry points
# to force a rebuild. Without this the image ships a binary whose `main` is empty — and it starts,
# exits 0, and looks like a crash-looping app with no error anywhere.
RUN touch crates/exchange-host/src/lib.rs crates/exchange-server/src/main.rs \
    && cargo build --release --locked --bin flux-exchange

# ─── what runs ───────────────────────────────────────────────────────────────────────────────────
# debian:bookworm-slim, pinned to the 2026-08-03 OCI index digest.
FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818 AS runtime

# `ca-certificates` is not optional: this host completes an OIDC token exchange over https and
# validates the provider's chain. Without it every sign-in fails at the token endpoint, which reads
# as "the provider refused us" — the exact confusion X-17 split apart.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# A fixed uid, and it matters more than it looks. The credential store **refuses a file whose mode is
# wider than it requires** rather than tightening it (X-09), and it creates its directory `0700` and
# its file `0600` — owned by whoever ran the process. A deploy that changed uid would find a store it
# cannot read and refuse to start, so this number is part of the deployment contract, not a detail.
RUN useradd --system --uid 10001 --create-home --home-dir /home/exchange exchange

COPY --from=build /src/target/release/flux-exchange /usr/local/bin/flux-exchange
COPY --from=console /console/dist /srv/console

# The volume mount point, owned by the uid that will write into it. fly attaches the volume over this
# path; the ownership set here is what the mounted filesystem inherits.
RUN mkdir -p /data && chown exchange:exchange /data
VOLUME /data

USER exchange
ENV FLUX_EXCHANGE_CONSOLE=/srv/console
EXPOSE 8080

# No shell form: `exec` semantics mean the binary is pid 1 and receives fly's SIGINT/SIGTERM
# directly, which is what `with_graceful_shutdown` in `main.rs` is waiting for. Wrapped in a shell,
# the signal reaches the shell and the server is killed mid-write — on a store that rewrites and
# fsyncs the whole file under one mutex.
ENTRYPOINT ["/usr/local/bin/flux-exchange"]
