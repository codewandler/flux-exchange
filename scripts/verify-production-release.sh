#!/usr/bin/env bash
set -euo pipefail

APP="${FLY_APP:-flux-exchange}"
BASE_URL="${PRODUCTION_URL:-https://flux-exchange.fly.dev}"
EXPECTED_VERSION="${EXPECTED_VERSION:?EXPECTED_VERSION is required}"
EXPECTED_SOURCE_SHA="${EXPECTED_SOURCE_SHA:?EXPECTED_SOURCE_SHA is required}"
EXPECTED_IMAGE_DIGEST="${EXPECTED_IMAGE_DIGEST:?EXPECTED_IMAGE_DIGEST is required}"
EVIDENCE_FILE="${EVIDENCE_FILE:-production-evidence.json}"

fail() {
  printf 'production verification failed: %s\n' "$1" >&2
  exit 1
}

require_header() {
  local file="$1"
  local name="$2"
  local expected="$3"
  tr -d '\r' <"$file" | rg -qi "^${name}:[[:space:]]*${expected}([[:space:]]*|;.*)$" ||
    fail "$name is absent or weaker than expected"
}

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-release.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

curl --fail --silent --show-error --location --dump-header "$work_dir/health.headers" \
  --output "$work_dir/health.json" "$BASE_URL/health"
jq -e --arg version "$EXPECTED_VERSION" '.status == "ok" and .version == $version' \
  "$work_dir/health.json" >/dev/null || fail '/health does not report the expected version'

curl --fail --silent --show-error --location --dump-header "$work_dir/console.headers" \
  --output /dev/null "$BASE_URL/"
curl --fail --silent --show-error --location --dump-header "$work_dir/api.headers" \
  --output /dev/null "$BASE_URL/api/onboarding"

for headers in "$work_dir/console.headers" "$work_dir/api.headers"; do
  require_header "$headers" content-security-policy "default-src 'self'"
  require_header "$headers" strict-transport-security 'max-age=31536000'
  require_header "$headers" x-content-type-options 'nosniff'
  require_header "$headers" referrer-policy 'no-referrer'
  require_header "$headers" permissions-policy 'camera=\(\), microphone=\(\), geolocation=\(\), payment=\(\), usb=\(\)'
done
require_header "$work_dir/api.headers" cache-control 'no-store'

flyctl releases --app "$APP" --json >"$work_dir/releases.json"
flyctl machines list --app "$APP" --json >"$work_dir/machines.json"

machine_count="$(jq '[.[] | select(.state == "started")] | length' "$work_dir/machines.json")"
[ "$machine_count" = 1 ] || fail "expected one started machine, found $machine_count"
actual_digest="$(jq -r '[.[] | select(.state == "started")][0].image_ref.digest // empty' "$work_dir/machines.json")"
[ "$actual_digest" = "$EXPECTED_IMAGE_DIGEST" ] || fail 'the running machine does not use the scanned image digest'

release_id="$(jq -r '.[0].ID // empty' "$work_dir/releases.json")"
machine_id="$(jq -r '[.[] | select(.state == "started")][0].id // empty' "$work_dir/machines.json")"
[ -n "$release_id" ] || fail 'Fly did not report a release identifier'
[ -n "$machine_id" ] || fail 'Fly did not report a machine identifier'

jq -n \
  --arg verified_at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg source_sha "$EXPECTED_SOURCE_SHA" \
  --arg image_digest "$EXPECTED_IMAGE_DIGEST" \
  --arg version "$EXPECTED_VERSION" \
  --arg fly_release "$release_id" \
  --arg fly_machine "$machine_id" \
  '{verified_at: $verified_at, source_sha: $source_sha, image_digest: $image_digest,
    application_version: $version, fly_release: $fly_release, fly_machine: $fly_machine,
    health: "ok", security_headers: "verified", api_cache_control: "no-store"}' >"$EVIDENCE_FILE"

jq . "$EVIDENCE_FILE"
