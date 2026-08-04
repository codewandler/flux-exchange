#!/usr/bin/env bash
# Describe the five native archives once, then let the production verifier compute signed facts.
set -euo pipefail

fail() { printf 'release-stage: %s\n' "$*" >&2; exit 1; }
[ "$#" = 6 ] || fail 'usage: release-stage.sh <assets-dir> <version> <source-sha> <build-id> <release-key-id> <manifest>'
assets_dir="$1"
version="$2"
source_sha="$3"
build_id="$4"
release_key_id="$5"
manifest="$6"
root="$(git rev-parse --show-toplevel)"
spec="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-stage.XXXXXX")"
trap 'rm -f -- "$spec"' EXIT

"$root/scripts/release-check-assets.sh" "$assets_dir" "$version"

jq -cn \
  --arg origin 'https://github.com/codewandler/flux-exchange' \
  --arg version "$version" --arg source "$source_sha" --arg build "$build_id" \
  --arg release_key "$release_key_id" \
  --slurpfile targets <(jq -s 'sort_by(.target)' "$assets_dir"/asset-*.json) \
  '{schema:"exchange.release-manifest.v1",origin:$origin,tag:("refs/tags/v"+$version),version:$version,source_commit:$source,build_id:$build,
    protocols:{exchange_api:"exchange.api.v1",effective_catalogue_response:"exchange.effective-catalogue-response.v1",invoke_request:"exchange.invoke-request.v1",invoke_response:"exchange.invoke-response.v1",connection_plan:"exchange.connection-plan.v1",supervisor:"exchange.supervisor-ready.v1"},
    signing_key_ids:[$release_key],assets:$targets[0]}' >"$spec"

cargo run --locked -p flux-exchange-release -- stage-release \
  --spec "$spec" --directory "$assets_dir" --output "$manifest"
