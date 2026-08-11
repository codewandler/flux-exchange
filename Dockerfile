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
# Node 22/trixie-slim, pinned to the 2026-08-03 OCI index digest. The readable tag says what to
# update; the digest says exactly what production builds. `npm ci` makes the lockfile authoritative.
FROM node:22-trixie-slim@sha256:517aa41d78545cb1b8c67b13655b4c13ede1ee9df1da8aab54cd7434aefbcaf8 AS console
WORKDIR /console
COPY console/package.json console/package-lock.json ./
RUN npm ci
COPY console/ ./
RUN npm run build

# ─── the binary ──────────────────────────────────────────────────────────────────────────────────
# 1.88 is the MSRV in `Cargo.toml`'s `rust-version`, and X-33's CI job builds against whatever that
# says. Do not raise this to make a build pass: the number is observed, not chosen, and raising it is
# a compatibility break for consumers of the published crate that belongs in the CHANGELOG.
# rust:1.88-alpine3.22, pinned to the 2026-08-03 OCI index digest. Building on musl makes the final
# binary static, so production does not need to ship an operating-system userland it never invokes.
FROM rust:1.88-alpine3.22@sha256:9dfaae478ecd298b6b5a039e1f2cc4fc040fc818a2de9aa78fa714dea036574d AS build
WORKDIR /src

# These are build inputs only. The final scratch stage receives the CA bundle, not apk or any of the
# compilers and interpreters used to build native dependencies.
RUN apk add --no-cache ca-certificates cmake make musl-dev perl

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
    && cargo build --release --locked --bin flux-exchange \
    && mkdir -p /image-root/data /image-root/home/exchange /image-root/tmp \
    && chown -R 10001:10001 /image-root/data /image-root/home/exchange \
    && chmod 1777 /image-root/tmp

# ─── what runs ───────────────────────────────────────────────────────────────────────────────────
# The runtime is intentionally scratch. A full Debian runtime carried more than a hundred known
# vulnerabilities in packages this service never executes; waiving them would make the scan
# ceremonial. The musl binary is static, so only its trust roots, console and writable directories
# need to cross this boundary.
FROM scratch AS runtime

# The CA bundle is not optional: this host completes OIDC and connector TLS exchanges. Without it
# every sign-in fails at the token endpoint, which reads as "the provider refused us" — the exact
# confusion X-17 split apart.
COPY --from=build /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt

COPY --from=build /src/target/release/flux-exchange /usr/local/bin/flux-exchange
COPY --from=console /console/dist /srv/console

# A fixed uid matters more than a passwd entry. The credential store refuses widened modes and the
# existing Fly volume is owned by 10001; changing the uid would make a sound store refuse startup.
# The builder supplies empty directories with that numeric ownership without adding a shell or user
# database to the final image.
COPY --from=build --chown=10001:10001 /image-root/home/exchange /home/exchange
COPY --from=build --chown=10001:10001 /image-root/data /data
COPY --from=build --chmod=1777 /image-root/tmp /tmp
VOLUME /data

USER 10001:10001
ENV HOME=/home/exchange
ENV SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt
ENV FLUX_EXCHANGE_CONSOLE=/srv/console
EXPOSE 8080

# No shell form: `exec` semantics mean the binary is pid 1 and receives fly's SIGINT/SIGTERM
# directly, which is what `with_graceful_shutdown` in `main.rs` is waiting for. Wrapped in a shell,
# the signal reaches the shell and the server is killed mid-write — on a store that rewrites and
# fsyncs the whole file under one mutex.
ENTRYPOINT ["/usr/local/bin/flux-exchange"]
