#!/usr/bin/env bash
set -euo pipefail

APP="${FLY_APP:-flux-exchange}"
VOLUME_NAME="${FLY_VOLUME_NAME:-flux_exchange_data}"
MAX_AGE_SECONDS="${MAX_SNAPSHOT_AGE_SECONDS:-86400}"
EXPECTED_RETENTION="${EXPECTED_SNAPSHOT_RETENTION_DAYS:-14}"

fail() {
  printf 'snapshot verification failed: %s\n' "$1" >&2
  exit 1
}

verify() {
  local volumes_json="$1"
  local snapshots_json="$2"
  local now_epoch="$3"
  local volume_count encrypted retention scheduled newest newest_epoch age

  volume_count="$(jq --arg name "$VOLUME_NAME" '[.[] | select(.name == $name and .state == "created")] | length' "$volumes_json")"
  [ "$volume_count" = 1 ] || fail "expected exactly one created production volume, found $volume_count"

  encrypted="$(jq -r --arg name "$VOLUME_NAME" '.[] | select(.name == $name and .state == "created") | .encrypted' "$volumes_json")"
  [ "$encrypted" = true ] || fail 'the production volume does not report encryption enabled'

  retention="$(jq -r --arg name "$VOLUME_NAME" '.[] | select(.name == $name and .state == "created") | .snapshot_retention' "$volumes_json")"
  [ "$retention" = "$EXPECTED_RETENTION" ] ||
    fail "production retention is $retention days, expected $EXPECTED_RETENTION"

  scheduled="$(jq -r --arg name "$VOLUME_NAME" '.[] | select(.name == $name and .state == "created") | .auto_backup_enabled' "$volumes_json")"
  [ "$scheduled" = true ] || fail 'automatic daily snapshots are disabled'

  newest="$(jq -r '[.[] | select(.status == "created") | .created_at] | sort | last // empty' "$snapshots_json")"
  [ -n "$newest" ] || fail 'no completed snapshot exists'
  newest_epoch="$(date -u -d "$newest" +%s)" || fail 'newest snapshot time is invalid'
  age="$((now_epoch - newest_epoch))"
  [ "$age" -ge 0 ] || fail 'newest snapshot is dated in the future'
  [ "$age" -le "$MAX_AGE_SECONDS" ] ||
    fail "newest completed snapshot is $age seconds old, over the $MAX_AGE_SECONDS-second RPO"

  # Snapshot and volume identifiers are deliberately absent. Treating them as operational secrets
  # keeps an alert or public Actions log from becoming a map to credential-bearing recovery state.
  jq -n \
    --arg app "$APP" \
    --arg checked_at "$(date -u -d "@$now_epoch" +%Y-%m-%dT%H:%M:%SZ)" \
    --arg newest_snapshot_at "$newest" \
    --argjson snapshot_age_seconds "$age" \
    --argjson retention_days "$retention" \
    '{app: $app, checked_at: $checked_at, newest_snapshot_at: $newest_snapshot_at,
      snapshot_age_seconds: $snapshot_age_seconds, retention_days: $retention_days,
      encrypted: true, scheduled_snapshots: true}'
}

self_test() {
  local fixture_dir now
  fixture_dir="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-snapshot.XXXXXX")"
  trap 'rm -rf "$fixture_dir"' RETURN
  now=1785772800

  printf '%s\n' '[{"id":"credential-shaped-volume-id","name":"flux_exchange_data","state":"created","encrypted":true,"snapshot_retention":14,"auto_backup_enabled":true}]' >"$fixture_dir/volumes.json"
  printf '%s\n' '[{"id":"credential-shaped-snapshot-id","status":"created","created_at":"2026-08-03T12:00:00Z","retention_days":14}]' >"$fixture_dir/snapshots.json"
  verify "$fixture_dir/volumes.json" "$fixture_dir/snapshots.json" "$now" |
    jq -e '.encrypted and .scheduled_snapshots and .retention_days == 14' >/dev/null

  printf '%s\n' '[{"id":"hidden","status":"created","created_at":"2026-08-01T00:00:00Z","retention_days":14}]' >"$fixture_dir/stale.json"
  if (verify "$fixture_dir/volumes.json" "$fixture_dir/stale.json" "$now") >/dev/null 2>&1; then
    fail 'self-test accepted a stale snapshot'
  fi

  if (verify "$fixture_dir/volumes.json" "$fixture_dir/snapshots.json" "$now") | rg -q 'credential-shaped'; then
    fail 'self-test found an identifier in public evidence'
  fi
  printf 'snapshot verifier self-test passed\n'
}

if [ "${1:-}" = '--self-test' ]; then
  self_test
  exit 0
fi

command -v flyctl >/dev/null || fail 'flyctl is required'
command -v jq >/dev/null || fail 'jq is required'

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-snapshot.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
flyctl volumes list --app "$APP" --json >"$work_dir/volumes.json"
volume_id="$(jq -r --arg name "$VOLUME_NAME" '[.[] | select(.name == $name and .state == "created")][0].id // empty' "$work_dir/volumes.json")"
[ -n "$volume_id" ] || fail 'the production volume is absent'
flyctl volumes snapshots list "$volume_id" --app "$APP" --json >"$work_dir/snapshots.json"
verify "$work_dir/volumes.json" "$work_dir/snapshots.json" "$(date -u +%s)"
