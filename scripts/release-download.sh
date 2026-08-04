#!/usr/bin/env bash
# Fetch one fixed-origin GitHub release asset through exactly one admitted 302.
set -euo pipefail

fail() { printf 'release-download: %s\n' "$*" >&2; exit 1; }
[ "$#" = 3 ] || fail 'usage: release-download.sh <release-tag> <basename> <destination>'
release_tag="$1"
basename="$2"
destination="$3"
root="$(git rev-parse --show-toplevel)"

case "$release_tag" in
  exchange-trust-v1|exchange-stable-v1) ;;
  v*) printf '%s' "${release_tag#v}" | grep -Eq '^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$' || fail 'invalid immutable release tag' ;;
  *) fail 'release tag is outside the closed transport policy' ;;
esac
printf '%s' "$basename" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9._-]{0,126}[A-Za-z0-9])?$' || fail 'unsafe asset basename'
case "$basename" in *..*) fail 'unsafe asset basename' ;; esac

url="https://github.com/codewandler/flux-exchange/releases/download/${release_tag}/${basename}"
headers="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-headers.XXXXXX")"
body="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-body.XXXXXX")"
trap 'rm -f -- "$headers" "$body"' EXIT
status="$(curl --silent --show-error --noproxy '*' --proxy '' --proxy-header 'Proxy-Authorization:' \
  --header 'Authorization:' --header 'Cookie:' --cookie '' --max-redirs 0 --dump-header "$headers" \
  --output /dev/null --write-out '%{http_code}' "$url")"
[ "$status" = 302 ] || fail "initial GitHub response was HTTP $status, expected 302"
location="$(sed -nE 's/^[Ll]ocation:[[:space:]]*([^\r]*)\r?$/\1/p' "$headers")"
[ "$(printf '%s\n' "$location" | grep -c .)" = 1 ] || fail 'GitHub response did not contain exactly one Location'

# Validate before making the CDN request. The verifier repeats this check over the retained evidence;
# this guard is what prevents the network client itself from following an inadmissible location.
python3 "$root/scripts/release-validate-redirect.py" "$location"

final_status="$(curl --silent --show-error --noproxy '*' --proxy '' --proxy-header 'Proxy-Authorization:' \
  --header 'Authorization:' --header 'Cookie:' --cookie '' --max-redirs 0 --output "$body" \
  --write-out '%{http_code}' "$location")"
[ "$final_status" = 200 ] || fail "release CDN response was HTTP $final_status, expected 200"
mv -- "$body" "$destination"
body="$destination"

fixture="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-transport.XXXXXX")"
trap 'rm -f -- "$headers" "$fixture"' EXIT
jq -cSn --arg location "$location" \
  '{status:302,location:$location,forwarded_credentials:false,final_status:200,second_redirect:false}' >"$fixture"
cargo run --quiet --locked -p flux-exchange-release -- verify-transport-fixture "$fixture"
