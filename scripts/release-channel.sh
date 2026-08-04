#!/usr/bin/env bash
# Allocate one serialized stable generation and sign it with the root-delegated channel role.
set -euo pipefail

fail() { printf 'release-channel: %s\n' "$*" >&2; exit 1; }
[ "$#" = 10 ] || fail 'usage: release-channel.sh <existing-or-dash> <manifest> <release-key-id> <channel-key-id> <issued-at> <expires-at> <output-dir> <trust-dir> <root-policy> <now>'
existing="$1"
manifest="$2"
release_key_id="$3"
channel_key_id="$4"
issued_at="$5"
expires_at="$6"
output_dir="$7"
trust_dir="$8"
root_policy="$9"
now="${10}"
channel="$output_dir/flux-exchange-release-channel.json"
entry="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-entry.XXXXXX")"
trap 'rm -f -- "$entry"' EXIT
mkdir -p "$output_dir"

manifest_sha="$(sha256sum "$manifest" | awk '{print $1}')"
jq -cS --arg digest "$manifest_sha" --arg release_key "$release_key_id" \
  '{tag,version,source_commit,build_id,manifest_sha256:$digest,release_key_ids:[$release_key],protocols}' \
  "$manifest" >"$entry"
cargo run --locked -p flux-exchange-release -- update-channel \
  --existing "$existing" --entry "$entry" --issued-at "$issued_at" --expires-at "$expires_at" \
  --signing-key-id "$channel_key_id" --output "$channel"
if [ "$existing" != - ] && cmp -s "$existing" "$channel"; then
  existing_dir="$(dirname "$existing")"
  jq -er '.signing_key_ids[]' "$channel" | while IFS= read -r key_id; do
    signature="flux-exchange-release-channel.json.${key_id}.minisig"
    [ -f "$existing_dir/$signature" ] || fail "idempotent channel retry is missing existing signature $signature"
    cp "$existing_dir/$signature" "$output_dir/$signature"
  done
  exit 0
fi
cargo run --locked -p flux-exchange-release -- sign "$channel" \
  --secret-key-env FLUX_EXCHANGE_CHANNEL_SIGNING_KEY_B64 --role channel \
  --trust-directory "$trust_dir" --root-policy "$root_policy" --now "$now" \
  --output-directory "$output_dir"
