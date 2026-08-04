#!/usr/bin/env bash
# Fetch one fixed-origin GitHub release asset through exactly one admitted 302.
set -euo pipefail

fail() { printf 'release-download: %s\n' "$*" >&2; exit 1; }
root="$(git rev-parse --show-toplevel)"

valid_key_id() {
  printf '%s' "$1" | grep -Eq '^[a-z0-9]([a-z0-9-]{0,62}[a-z0-9])?$' && [[ "$1" != *--* ]]
}

byte_limit_for() {
  local release_tag="$1" basename="$2" key_id version target _runner format _executable suffix
  case "$release_tag:$basename" in
    exchange-trust-v1:flux-exchange-release-trust.json) printf '65536\n'; return ;;
    exchange-trust-v1:flux-exchange-release-trust.json.*.minisig)
      key_id="${basename#flux-exchange-release-trust.json.}"
      key_id="${key_id%.minisig}"
      valid_key_id "$key_id" || fail 'unsafe root key id before signature path construction'
      printf '4096\n'; return ;;
    exchange-stable-v1:flux-exchange-release-channel.json) printf '262144\n'; return ;;
    exchange-stable-v1:flux-exchange-release-channel.json.*.minisig)
      key_id="${basename#flux-exchange-release-channel.json.}"
      key_id="${key_id%.minisig}"
      valid_key_id "$key_id" || fail 'unsafe channel key id before signature path construction'
      printf '4096\n'; return ;;
  esac
  case "$release_tag" in
    v*) version="${release_tag#v}" ;;
    *) fail 'release tag is outside the closed transport policy' ;;
  esac
  case "$basename" in
    flux-exchange-release-manifest.json) printf '262144\n'; return ;;
    flux-exchange-release-manifest.json.*.minisig)
      key_id="${basename#flux-exchange-release-manifest.json.}"
      key_id="${key_id%.minisig}"
      valid_key_id "$key_id" || fail 'unsafe release key id before signature path construction'
      printf '4096\n'; return ;;
  esac
  while IFS=$'\t' read -r target _runner format _executable; do
    [ "$format" = zip ] && suffix=.zip || suffix=.tar.zst
    if [ "$basename" = "flux-exchange-${version}-${target}${suffix}" ]; then
      printf '268435456\n'
      return
    fi
  done < <(awk '!/^#/ && NF' "$root/release-targets.tsv")
  fail 'asset basename is outside the closed release set'
}

require_bounded_curl() {
  local curl_version curl_major curl_minor _curl_patch
  curl_version="$(curl --version | awk 'NR == 1 { print $2 }')"
  IFS=. read -r curl_major curl_minor _curl_patch <<<"$curl_version"
  [[ "$curl_major" =~ ^[0-9]+$ && "$curl_minor" =~ ^[0-9]+$ ]] || fail 'cannot determine curl version for bounded download'
  if (( curl_major < 8 || (curl_major == 8 && curl_minor < 4) )); then
    fail 'curl 8.4.0 or newer is required for receive-time byte bounds'
  fi
}

if [ "${1:-}" = --self-test ]; then
  require_bounded_curl
  [ "$(byte_limit_for exchange-trust-v1 flux-exchange-release-trust.json)" = 65536 ]
  [ "$(byte_limit_for exchange-stable-v1 flux-exchange-release-channel.json.channel-2026-01.minisig)" = 4096 ]
  [ "$(byte_limit_for v1.2.3 flux-exchange-1.2.3-x86_64-pc-windows-msvc.zip)" = 268435456 ]
  if (byte_limit_for exchange-stable-v1 flux-exchange-release-channel.json.bad--key.minisig >/dev/null 2>&1); then fail 'self-test accepted an unsafe key id'; fi
  if (byte_limit_for v1.2.3 arbitrary-file >/dev/null 2>&1); then fail 'self-test accepted an arbitrary immutable asset'; fi
  if (byte_limit_for v1.2.3 flux-exchange-1.2.4-x86_64-pc-windows-msvc.zip >/dev/null 2>&1); then fail 'self-test accepted a mismatched archive version'; fi
  printf 'PASS release download policy self-test\n'
  exit 0
fi

[ "$#" = 3 ] || fail 'usage: release-download.sh <release-tag> <basename> <destination>|--self-test'
release_tag="$1"
basename="$2"
destination="$3"

case "$release_tag" in
  exchange-trust-v1|exchange-stable-v1) ;;
  v*) printf '%s' "${release_tag#v}" | grep -Eq '^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$' || fail 'invalid immutable release tag' ;;
  *) fail 'release tag is outside the closed transport policy' ;;
esac
printf '%s' "$basename" | grep -Eq '^[A-Za-z0-9]([A-Za-z0-9._-]{0,126}[A-Za-z0-9])?$' || fail 'unsafe asset basename'
case "$basename" in *..*) fail 'unsafe asset basename' ;; esac
byte_limit="$(byte_limit_for "$release_tag" "$basename")"
require_bounded_curl

url="https://github.com/codewandler/flux-exchange/releases/download/${release_tag}/${basename}"
headers="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-headers.XXXXXX")"
body="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-body.XXXXXX")"
trap 'rm -f -- "$headers" "$body"' EXIT
status="$(curl --silent --show-error --noproxy '*' --proxy '' --proxy-header 'Proxy-Authorization:' \
  --header 'Authorization:' --header 'Cookie:' --cookie '' --proto '=https' --max-redirs 0 --dump-header "$headers" \
  --max-filesize 65536 --output /dev/null --write-out '%{http_code}' "$url")"
[ "$status" = 302 ] || fail "initial GitHub response was HTTP $status, expected 302"
location="$(sed -nE 's/^[Ll]ocation:[[:space:]]*([^\r]*)\r?$/\1/p' "$headers")"
[ "$(printf '%s\n' "$location" | grep -c .)" = 1 ] || fail 'GitHub response did not contain exactly one Location'

# Validate before making the CDN request. The verifier repeats this check over the retained evidence;
# this guard is what prevents the network client itself from following an inadmissible location.
python3 "$root/scripts/release-validate-redirect.py" "$location"

final_status="$(curl --silent --show-error --noproxy '*' --proxy '' --proxy-header 'Proxy-Authorization:' \
  --header 'Authorization:' --header 'Cookie:' --cookie '' --proto '=https' --max-redirs 0 \
  --max-filesize "$byte_limit" --output "$body" \
  --write-out '%{http_code}' "$location")"
[ "$final_status" = 200 ] || fail "release CDN response was HTTP $final_status, expected 200"
[ "$(wc -c <"$body")" -le "$byte_limit" ] || fail 'release asset exceeded its declared byte bound'
mv -- "$body" "$destination"
body="$destination"

fixture="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-transport.XXXXXX")"
trap 'rm -f -- "$headers" "$fixture"' EXIT
jq -cSn --arg location "$location" \
  '{status:302,location:$location,forwarded_credentials:false,final_status:200,second_redirect:false}' >"$fixture"
cargo run --quiet --locked -p flux-exchange-release -- verify-transport-fixture "$fixture"
