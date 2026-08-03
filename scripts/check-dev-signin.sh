#!/usr/bin/env bash
# Exercise the documented development command as a browser would, including the process boundary.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_dir="$(mktemp -d)"
server_log="$run_dir/server.log"
cookie_jar="$run_dir/cookies"
server_pid=""

cleanup() {
  if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  rm -rf -- "$run_dir"
}
trap cleanup EXIT

cd "$repo_root"

# Unset every competing identity choice. This test is specifically the zero-configuration shorthand,
# not the explicit development roster or a federated composition. The CI gate forces Cargo colour
# globally; this log is a machine-readable process boundary, and ANSI between a structured field
# name and `=` makes a healthy ephemeral listener undiscoverable.
env \
  -u FLUX_EXCHANGE_DEV_IDENTITY \
  -u FLUX_EXCHANGE_OIDC_ISSUER \
  -u FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT \
  -u FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT \
  -u FLUX_EXCHANGE_OIDC_JWKS_URI \
  -u FLUX_EXCHANGE_OIDC_CLIENT_ID \
  -u FLUX_EXCHANGE_OIDC_CLIENT_SECRET \
  -u FLUX_EXCHANGE_OIDC_REDIRECT_URI \
  -u FLUX_EXCHANGE_OIDC_TENANT \
  NO_COLOR=1 \
  CARGO_TERM_COLOR=never \
  FLUX_EXCHANGE_BIND=127.0.0.1:0 \
  USER=flux-dev-e2e \
  cargo run --locked -- --dev >"$server_log" 2>&1 &
server_pid=$!

port=""
for _ in {1..400}; do
  port="$(sed -nE 's/.*local=127\.0\.0\.1:([0-9]+).*/\1/p' "$server_log" | tail -n1)"
  if [[ -n "$port" ]]; then
    break
  fi
  if ! kill -0 "$server_pid" 2>/dev/null; then
    wait "$server_pid" || true
    sed -n '1,240p' "$server_log" >&2
    echo "development server exited before listening" >&2
    exit 1
  fi
  sleep 0.05
done

if [[ -z "$port" ]]; then
  sed -n '1,240p' "$server_log" >&2
  echo "development server did not announce its ephemeral loopback port" >&2
  exit 1
fi

origin="http://127.0.0.1:$port"
signin_page="$(curl --fail --silent --show-error "$origin/api/signin")"
grep -Fq '<form method="post" action="/api/signin">' <<<"$signin_page"
grep -Fq 'Continue as the local development user' <<<"$signin_page"

signed_in_page="$(curl --fail --silent --show-error \
  --request POST \
  --cookie-jar "$cookie_jar" \
  "$origin/api/signin")"
grep -Fq 'You are signed in as the local development user' <<<"$signed_in_page"
grep -Fq '#HttpOnly_127.0.0.1' "$cookie_jar"

session="$(curl --fail --silent --show-error \
  --cookie "$cookie_jar" \
  "$origin/api/session")"
grep -Fq '"id":"flux-dev-e2e"' <<<"$session"
grep -Fq '"kind":"user"' <<<"$session"
grep -Fq '"tenant":"dev"' <<<"$session"
