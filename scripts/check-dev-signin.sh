#!/usr/bin/env bash
# Exercise the documented development command as a browser would, including the process boundary.

set -euo pipefail

fail() {
  printf 'check-dev-signin: %s\n' "$*" >&2
  exit 1
}

check_release_v2_evidence() {
  local script="$1" evidence legacy_plan
  evidence="$(sed -n '/^# BEGIN X-134 RELEASE-V2 EVIDENCE$/,$p' "$script")"
  test -n "$evidence" || fail 'release-v2 evidence boundary is missing'
  legacy_plan="exchange.connection-plan.""v1"
  if grep -Fq "$legacy_plan" <<<"$evidence"; then
    fail 'development smoke still admits a v1 connection-plan ratchet'
  fi
  grep -Fq 'exchange.connection-plan.v2' <<<"$evidence" \
    || fail 'development smoke does not query the authoritative v2 connection plan'
  grep -Fq 'secret_json_forbidden' <<<"$evidence" \
    || fail 'development smoke does not prove the secret JSON refusal'
  grep -Fq 'application/vnd.flux-exchange.service-account-handoff-v1' <<<"$evidence" \
    || fail 'development smoke does not bind the FXSA response media type'
  grep -Fq 'FXSA\x01\x01\x00\x00' <<<"$evidence" \
    || fail 'development smoke does not decode the closed FXSA header'
}

if [[ "${1:-}" = --self-test ]]; then
  scratch="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-dev-signin.XXXXXX")"
  trap 'rm -f -- "$scratch"' EXIT
  cp "${BASH_SOURCE[0]}" "$scratch"
  check_release_v2_evidence "$scratch"

  python3 - "$scratch" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
source = path.read_text()
current = (
    "$origin/api/connections/intercom/plan?version="
    + "exchange.connection-plan.v2"
)
source = source.replace(
    current,
    current.removesuffix("v2") + "v1",
    1,
)
path.write_text(source)
PY
  if (check_release_v2_evidence "$scratch" >/dev/null 2>&1); then
    fail 'self-test accepted the legacy connection-plan identity'
  fi

  cp "${BASH_SOURCE[0]}" "$scratch"
  python3 - "$scratch" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
source = path.read_text()
media_type = (
    "content-type: application/vnd.flux-exchange.service-account-handoff-" + "v1"
)
source = source.replace(
    media_type,
    "content-type: application/octet-stream",
    1,
)
path.write_text(source)
PY
  if (check_release_v2_evidence "$scratch" >/dev/null 2>&1); then
    fail 'self-test accepted removal of the FXSA response media type'
  fi

  printf 'PASS check-dev-signin release-v2 self-test\n'
  exit 0
fi

check_release_v2_evidence "${BASH_SOURCE[0]}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
run_dir="$(mktemp -d)"
server_log="$run_dir/server.log"
first_server_log="$run_dir/server-first.log"
cookie_jar="$run_dir/cookies"
secret_refusal="$run_dir/secret-json-refusal.json"
handoff_headers="$run_dir/service-account-handoff.headers"
handoff_frame="$run_dir/service-account-handoff.fxsa"
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

# Build once, then own the actual server process on both Unix and Windows. `cargo run` can replace
# itself on Unix, but on Windows Cargo remains the parent waiting for the `.exe`; killing `$!` there
# would stop Cargo without proving the Exchange child released its listener and store handles.
cargo build --locked --bin flux-exchange
server_binary="${CARGO_TARGET_DIR:-$repo_root/target}/debug/flux-exchange"
case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) server_binary="$server_binary.exe" ;;
esac
test -x "$server_binary" || {
  echo "development server binary was not built at $server_binary" >&2
  exit 1
}

# Unset every competing identity choice. This test is specifically the zero-configuration shorthand,
# not the explicit development roster or a federated composition. The CI gate forces Cargo colour
# globally; this log is a machine-readable process boundary, and ANSI between a structured field
# name and `=` makes a healthy ephemeral listener undiscoverable.
env \
  -u FLUX_EXCHANGE_DEV_IDENTITY \
  -u FLUX_EXCHANGE_LOCAL_USERS \
  -u FLUX_EXCHANGE_TENANT \
  -u FLUX_EXCHANGE_OIDC_ISSUER \
  -u FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT \
  -u FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT \
  -u FLUX_EXCHANGE_OIDC_JWKS_URI \
  -u FLUX_EXCHANGE_OIDC_CLIENT_ID \
  -u FLUX_EXCHANGE_OIDC_CLIENT_SECRET \
  -u FLUX_EXCHANGE_OIDC_REDIRECT_URI \
  -u FLUX_EXCHANGE_OIDC_TENANT \
  -u FLUX_EXCHANGE_OIDC_HOSTED_DOMAIN \
  -u FLUX_EXCHANGE_STATE \
  -u FLUX_EXCHANGE_CREDENTIALS \
  -u FLUX_EXCHANGE_SETTINGS \
  -u FLUX_EXCHANGE_GRANTS \
  -u FLUX_EXCHANGE_CONNECTIONS \
  -u FLUX_EXCHANGE_CHANNELS \
  -u FLUX_EXCHANGE_WORKFLOWS \
  -u FLUX_EXCHANGE_AUDIT \
  -u FLUX_EXCHANGE_SERVICE_ACCOUNTS \
  -u FLUX_EXCHANGE_APPS \
  NO_COLOR=1 \
  CARGO_TERM_COLOR=never \
  XDG_STATE_HOME="$run_dir/state-home" \
  LOCALAPPDATA="$run_dir/state-home" \
  USERPROFILE="$run_dir/home" \
  FLUX_EXCHANGE_BIND=127.0.0.1:0 \
  USER=flux-dev-e2e \
  "$server_binary" --dev >"$server_log" 2>&1 &
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

if grep -Fq 'store is bound' "$server_log"; then
  sed -n '1,240p' "$server_log" >&2
  echo "development composition left a durable store unbound" >&2
  exit 1
fi

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) state_root="$run_dir/state-home/Flux/Exchange" ;;
  *) state_root="$run_dir/state-home/flux-exchange" ;;
esac
for path in \
  credentials/store.txt \
  audit/events.sqlite3; do
  test -f "$state_root/$path" || {
    echo "development composition did not create $path" >&2
    exit 1
  }
done
for path in settings grants connections channels workflows service-accounts; do
  test -d "$state_root/$path" || {
    echo "development composition did not bind $path" >&2
    exit 1
  }
done

# BEGIN X-134 RELEASE-V2 EVIDENCE
# The ordinary HTTP connection surface is now projection-only. It returns the complete v2 plan,
# while the former JSON mutation path refuses before decoding or writing a credential. Native FXLM
# owns the secret-bearing ceremony; this browser-shaped smoke must never reintroduce it.
credential_sentinel="X127-SENTINEL-NOT-A-REAL-SECRET"
connection_plan="$(curl --fail --silent --show-error \
  --cookie "$cookie_jar" \
  "$origin/api/connections/intercom/plan?version=exchange.connection-plan.v2")"
grep -Fq '"version":"exchange.connection-plan.v2"' <<<"$connection_plan"
grep -Fq '"selection":null' <<<"$connection_plan"
grep -Fq '"credential_revision":null' <<<"$connection_plan"

secret_status="$(curl --silent --show-error \
  --cookie "$cookie_jar" \
  --header 'content-type: application/json' \
  --output "$secret_refusal" \
  --write-out '%{http_code}' \
  --data "{\"version\":\"exchange.connection-plan.v2\",\"name\":\"restart-proof\",\"values\":{\"credential.intercom.access_token\":\"$credential_sentinel\",\"setting.default.endpoint.host\":\"api.intercom.io\"}}" \
  "$origin/api/connections/intercom/plan")"
test "$secret_status" = 415 || {
  echo "legacy connection secret JSON returned HTTP $secret_status instead of 415" >&2
  exit 1
}
test "$(tr -d '\r\n' <"$secret_refusal")" = '{"code":"secret_json_forbidden"}' || {
  echo "legacy connection secret JSON did not return the closed refusal" >&2
  exit 1
}
if grep -Fq "$credential_sentinel" <<<"$connection_plan" || \
   grep -R -Fq -- "$credential_sentinel" "$state_root"; then
  echo "a refused connection credential entered an Exchange response or durable state" >&2
  exit 1
fi

# Mint one durable bearer identity into the exact one-frame FXSA response, restart the real process
# over the same root, and prove the second process accepts it. The frame remains in this private
# scratch directory and is never printed; the store itself contains only the verifier.
expires_at="$(( $(date +%s) + 3600 ))"
mint_status="$(curl --silent --show-error \
  --cookie "$cookie_jar" \
  --header 'content-type: application/json' \
  --dump-header "$handoff_headers" \
  --output "$handoff_frame" \
  --write-out '%{http_code}' \
  --data "{\"id\":\"restart-proof\",\"expires_at\":$expires_at}" \
  "$origin/api/service-accounts")"
test "$mint_status" = 201 || {
  echo "Service Account handoff returned HTTP $mint_status instead of 201" >&2
  exit 1
}
grep -Fqi 'content-type: application/vnd.flux-exchange.service-account-handoff-v1' "$handoff_headers"
grep -Fqi 'cache-control: no-store' "$handoff_headers"
service_account_token="$(python3 - "$handoff_frame" <<'PY'
import pathlib
import sys

frame = pathlib.Path(sys.argv[1]).read_bytes()
if len(frame) < 13 or frame[:8] != b"FXSA\x01\x01\x00\x00":
    raise SystemExit("Service Account response is not a v1 Exchange-to-writer FXSA frame")
declared = int.from_bytes(frame[8:12], "big")
if not 1 <= declared <= 512 or len(frame) != 12 + declared:
    raise SystemExit("Service Account response is not exactly one bounded FXSA frame")
token = frame[12:]
try:
    sys.stdout.write(token.decode("ascii"))
except UnicodeDecodeError as error:
    raise SystemExit("current bearer token is not ASCII") from error
PY
)"
test -n "$service_account_token" || {
  echo "development composition returned an empty FXSA token" >&2
  exit 1
}

kill "$server_pid"
wait "$server_pid" || true
server_pid=""
if grep -Fq "$credential_sentinel" "$server_log"; then
  echo "a connection credential entered server stdout or stderr" >&2
  exit 1
fi
mv "$server_log" "$first_server_log"

env \
  -u FLUX_EXCHANGE_DEV_IDENTITY \
  -u FLUX_EXCHANGE_LOCAL_USERS \
  -u FLUX_EXCHANGE_TENANT \
  -u FLUX_EXCHANGE_STATE \
  -u FLUX_EXCHANGE_CREDENTIALS \
  -u FLUX_EXCHANGE_SETTINGS \
  -u FLUX_EXCHANGE_GRANTS \
  -u FLUX_EXCHANGE_CONNECTIONS \
  -u FLUX_EXCHANGE_CHANNELS \
  -u FLUX_EXCHANGE_WORKFLOWS \
  -u FLUX_EXCHANGE_AUDIT \
  -u FLUX_EXCHANGE_SERVICE_ACCOUNTS \
  -u FLUX_EXCHANGE_APPS \
  NO_COLOR=1 \
  CARGO_TERM_COLOR=never \
  XDG_STATE_HOME="$run_dir/state-home" \
  LOCALAPPDATA="$run_dir/state-home" \
  USERPROFILE="$run_dir/home" \
  FLUX_EXCHANGE_BIND=127.0.0.1:0 \
  USER=flux-dev-e2e \
  "$server_binary" --dev >"$server_log" 2>&1 &
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
    echo "restarted development server exited before listening" >&2
    exit 1
  fi
  sleep 0.05
done

test -n "$port" || {
  echo "restarted development server did not listen" >&2
  exit 1
}
curl --fail --silent --show-error \
  --header "Authorization: Bearer $service_account_token" \
  "http://127.0.0.1:$port/api/catalogue/effective" >/dev/null

restarted_session="$(curl --fail --silent --show-error \
  --header "Authorization: Bearer $service_account_token" \
  "http://127.0.0.1:$port/api/session")"
grep -Fq '"id":"restart-proof"' <<<"$restarted_session"
grep -Fq '"kind":"service_account"' <<<"$restarted_session"
grep -Fq '"tenant":"dev"' <<<"$restarted_session"
if grep -Fq "$credential_sentinel" <<<"$restarted_session" || \
   grep -Fq "$credential_sentinel" "$first_server_log" "$server_log" || \
   grep -R -Fq -- "$credential_sentinel" "$state_root"; then
  echo "a refused connection credential entered server output, durable state or a response" >&2
  exit 1
fi
