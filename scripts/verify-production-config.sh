#!/usr/bin/env bash
set -euo pipefail

APP="${FLY_APP:-flux-exchange}"
OPERATOR_POLICY='FLUX_EXCHANGE_OPERATOR_SUBJECTS'

fail() {
  printf 'production configuration verification failed: %s\n' "$1" >&2
  exit 1
}

operator_policy_is_deployed() {
  local metadata="$1"

  jq -e --arg name "$OPERATOR_POLICY" '
    [.[] | select(.name == $name)] as $entries
    | (($entries | length) == 1 and $entries[0].status == "Deployed")
  ' "$metadata" >/dev/null 2>&1
}

self_test() {
  local work_dir
  work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-config.XXXXXX")"
  trap 'rm -rf "$work_dir"' RETURN

  printf '%s\n' \
    '[{"name":"FLUX_EXCHANGE_OPERATOR_SUBJECTS","status":"Deployed","digest":"redacted"}]' \
    >"$work_dir/deployed.json"
  printf '%s\n' '[]' >"$work_dir/absent.json"
  printf '%s\n' \
    '[{"name":"FLUX_EXCHANGE_OPERATOR_SUBJECTS","status":"Staged"}]' \
    >"$work_dir/staged.json"
  printf '%s\n' \
    '[{"name":"FLUX_EXCHANGE_OPERATOR_SUBJECTS","status":"Deployed"},' \
    ' {"name":"FLUX_EXCHANGE_OPERATOR_SUBJECTS","status":"Deployed"}]' \
    >"$work_dir/duplicate.json"

  operator_policy_is_deployed "$work_dir/deployed.json" ||
    fail 'self-test refused one deployed operator policy'
  for refused in absent staged duplicate; do
    if operator_policy_is_deployed "$work_dir/$refused.json"; then
      fail "self-test accepted $refused operator policy metadata"
    fi
  done

  printf 'production configuration verifier self-test passed\n'
}

if [ "${1:-}" = '--self-test' ]; then
  self_test
  exit 0
fi

work_dir="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-config.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT

# Fly returns only names, digests and deployment status. The value cannot be read through this API,
# and even that value-free metadata stays in the private temporary directory rather than a log or
# retained workflow artifact.
flyctl secrets list --app "$APP" --json >"$work_dir/secrets.json"
operator_policy_is_deployed "$work_dir/secrets.json" ||
  fail "$OPERATOR_POLICY is absent, duplicated or not deployed"

printf 'production operator policy is deployed\n'
