#!/usr/bin/env bash
#
# Publish this workspace's crates.io artifact.
#
# Modelled on ../flux-connectors' script of the same name, deliberately much smaller. That workspace
# publishes a four-crate closure and has to derive a topological order; this one publishes exactly
# one crate — `codewandler-flux-exchange-host` — so there is no order to get wrong. `flux-exchange`
# is `publish = false`: it is one composition of the host, and a product embedding the host writes
# its own binary.
#
# The properties worth keeping from the sibling, because they are about recoverability rather than
# about scale:
#
#   - Idempotent: a crate@version already on crates.io is treated as done and skipped, so a re-run
#     after a failed or partial publish resumes rather than erroring. A published version cannot be
#     withdrawn, so "already up" has to read as success.
#   - Rate-limit aware: crates.io answers a burst of new crates with 429 and a "try again after"
#     hint. One crate rarely trips it, but a first publish can; the wait is parsed and honoured so an
#     unattended run grinds through instead of failing.
#   - Needs a token: `CARGO_REGISTRY_TOKEN` in the environment (CI) or a prior `cargo login`
#     (local). Publishing is CI-only by contract — see AGENTS.md — so the local path exists for
#     `--dry-run`, not for releasing.
#
# Usage:
#   scripts/publish-crates-io.sh             # publish (needs a token)
#   scripts/publish-crates-io.sh --dry-run   # package + verify only, uploads nothing
#
set -uo pipefail

MODE="publish"
case "${1:-}" in
  --dry-run) MODE="dry-run" ;;
  "") ;;
  *) echo "usage: $0 [--dry-run]" >&2; exit 2 ;;
esac

cd "$(dirname "$0")/.."

# The one crate this workspace publishes. If a second ever joins it, take ../flux-connectors'
# derived-order script rather than hand-listing them here — a list goes stale, a sort cannot.
CRATE="codewandler-flux-exchange-host"

VERSION="$(
  cargo metadata --format-version 1 --no-deps --offline --manifest-path Cargo.toml 2>/dev/null \
    | CRATE="$CRATE" python3 -c "import json,os,sys
for pkg in json.load(sys.stdin)['packages']:
    if pkg['name'] == os.environ['CRATE']:
        print(pkg['version'])
        break"
)"

if [ -z "$VERSION" ]; then
  echo "!! could not read $CRATE's version from the workspace manifests" >&2
  exit 1
fi

if [ "$MODE" = "dry-run" ]; then
  echo "==> cargo publish --dry-run -p $CRATE   ($VERSION)"
  cargo publish --dry-run -p "$CRATE" || exit 1
  echo "== $CRATE@$VERSION packages and verifies (nothing uploaded) =="
  exit 0
fi

# Is crate@version already on crates.io? Answering from the index costs one HTTP GET; letting
# `cargo publish` discover it costs a full package + verify build. Any doubt — network error,
# unparseable answer — falls through to the publish path, which is idempotent anyway. This is an
# optimization, never the correctness boundary.
already_published() {
  curl -sS --max-time 20 -H "User-Agent: flux-exchange-release (codewandler/flux-exchange)" \
    "https://crates.io/api/v1/crates/$CRATE/$VERSION" 2>/dev/null \
    | grep -q "\"num\":\"$VERSION\"" || return 1
  return 0
}

if already_published; then
  echo "==> $CRATE@$VERSION already on crates.io — nothing to do"
  exit 0
fi

while true; do
  echo "==> cargo publish -p $CRATE   ($VERSION)"
  if out=$(cargo publish -p "$CRATE" 2>&1); then
    echo "== $CRATE@$VERSION published =="
    exit 0
  fi

  # Already-published is success for our purposes — it is what makes a re-run resumable.
  if echo "$out" | grep -qiE "already (exists|uploaded)|already been (uploaded|published)|crate version .* is already"; then
    echo "== $CRATE@$VERSION was already on crates.io =="
    exit 0
  fi

  if echo "$out" | grep -qiE "429 Too Many Requests|too many (new )?crates"; then
    retry_at=$(echo "$out" | grep -oiE "try again after [^.]*GMT" | sed -E "s/try again after //I" | head -1)
    now=$(date -u +%s)
    target=$(date -u -d "$retry_at" +%s 2>/dev/null || echo $((now + 600)))
    wait=$(( target - now + 20 ))
    [ "$wait" -lt 20 ] && wait=20
    echo "    rate-limited (429); waiting ${wait}s (until ${retry_at:-~10m}) then retrying..."
    sleep "$wait"
    continue
  fi

  echo "$out" | tail -25
  echo "!! publish failed (fix, then re-run — an already-published version is skipped)" >&2
  exit 1
done
