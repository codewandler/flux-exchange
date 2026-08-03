#!/usr/bin/env bash
#
# Keep X-92's committed security contract from silently shrinking. GitHub-side controls cannot be
# proved from an unprivileged pull-request job; those are verified through read-only API calls when
# changed. This checker owns the local half: a real private-reporting route, explicit report-safety
# expectations, every dependency tree, and one inseparable Flux/connector update group.
#
set -uo pipefail

fail() { printf '\033[31mFAIL\033[0m %s\n' "$1" >&2; }

contains() {
  local file="$1" pattern="$2" description="$3"
  if ! grep -qE "$pattern" "$file"; then
    fail "$file: missing $description"
    return 1
  fi
}

check_security() {
  local file="$1" failures=0
  [ -f "$file" ] || { fail "$file: missing security policy"; return 1; }
  contains "$file" 'https://github\.com/codewandler/flux-exchange/security/advisories/new' \
    'the repository private-advisory URL' || failures=$((failures + 1))
  contains "$file" '^## Supported versions' 'a Supported versions section' || failures=$((failures + 1))
  contains "$file" '^## Response expectations' 'a Response expectations section' || failures=$((failures + 1))
  contains "$file" '(credential|token|cookie|secret)' 'a prohibition on credential-shaped report data' || failures=$((failures + 1))
  contains "$file" '(customer|tenant|personal|production) data' 'a prohibition on sensitive report data' || failures=$((failures + 1))
  [ "$failures" -eq 0 ]
}

check_dependabot() {
  local file="$1" failures=0
  [ -f "$file" ] || { fail "$file: missing Dependabot configuration"; return 1; }
  entry_present() {
    local ecosystem="$1" directory="$2"
    awk -v ecosystem="$ecosystem" -v directory="$directory" '
      function verdict() {
        if (in_entry && found_ecosystem == ecosystem && found_directory == directory) found = 1
      }
      /^  - package-ecosystem:/ {
        verdict()
        in_entry = 1
        found_ecosystem = $0
        sub(/^.*package-ecosystem:[[:space:]]*"?/, "", found_ecosystem)
        sub(/"?[[:space:]]*$/, "", found_ecosystem)
        found_directory = ""
        next
      }
      in_entry && /^    directory:/ {
        found_directory = $0
        sub(/^.*directory:[[:space:]]*"?/, "", found_directory)
        sub(/"?[[:space:]]*$/, "", found_directory)
      }
      END { verdict(); exit !found }
    ' "$file"
  }
  entry_present cargo / || { fail "$file: missing Cargo update entry for /"; failures=$((failures + 1)); }
  entry_present npm /console || { fail "$file: missing npm update entry for /console"; failures=$((failures + 1)); }
  entry_present npm /web || { fail "$file: missing npm update entry for /web"; failures=$((failures + 1)); }
  entry_present github-actions / || { fail "$file: missing GitHub Actions update entry for /"; failures=$((failures + 1)); }

  # These patterns must occur in the same YAML group. Restrict the scan to the first group block
  # containing either family pattern; a second group starts at the same six-space indentation.
  local family_group
  family_group="$(awk '
    /^      [A-Za-z0-9_-]+:$/ {
      if (collect && seen) exit
      collect = 1; seen = 0; block = $0 ORS; next
    }
    collect {
      block = block $0 ORS
      if ($0 ~ /codewandler-(flux|connector)-\*/) seen = 1
    }
    END { if (seen) printf "%s", block }
  ' "$file")"
  if ! printf '%s' "$family_group" | grep -q 'codewandler-flux-\*' ||
     ! printf '%s' "$family_group" | grep -q 'codewandler-connector-\*'; then
    fail "$file: codewandler-flux-* and codewandler-connector-* must share one update group"
    failures=$((failures + 1))
  fi
  [ "$failures" -eq 0 ]
}

check_workflow() {
  local file="$1" failures=0
  [ -f "$file" ] || { fail "$file: missing CI workflow"; return 1; }
  local commands
  commands="$(grep -E '^[[:space:]]*\./scripts/check-repository-security\.sh' "$file" || true)"
  if [ "$(printf '%s\n' "$commands" | grep -c -- '--self-test')" -ne 1 ] ||
     [ "$(printf '%s\n' "$commands" | grep -cv -- '--self-test')" -ne 1 ] ||
     [ "$(printf '%s\n' "$commands" | sed -n '1p')" != "$(printf '%s\n' "$commands" | grep -- '--self-test')" ]; then
    fail "$file: checker must run --self-test once, before one real scan"
    failures=$((failures + 1))
  fi
  [ "$failures" -eq 0 ]
}

if [ "${1:-}" = "--self-test" ]; then
  fixture_dir="$(mktemp -d)"
  trap 'rm -rf "$fixture_dir"' EXIT
  security="$fixture_dir/SECURITY.md"
  dependabot="$fixture_dir/dependabot.yml"
  workflow="$fixture_dir/ci.yml"

  printf '%s\n' \
    '# Security' \
    'https://github.com/codewandler/flux-exchange/security/advisories/new' \
    '## Supported versions' \
    '## Response expectations' \
    'Never include a credential, token, cookie, or secret.' \
    'Never include customer data or tenant data.' >"$security"
  printf '%s\n' \
    'version: 2' \
    'updates:' \
    '  - package-ecosystem: "cargo"' \
    '    directory: "/"' \
    '    groups:' \
    '      flux-family:' \
    '        patterns:' \
    '          - "codewandler-flux-*"' \
    '          - "codewandler-connector-*"' \
    '  - package-ecosystem: "npm"' \
    '    directory: "/console"' \
    '  - package-ecosystem: "npm"' \
    '    directory: "/web"' \
    '  - package-ecosystem: "github-actions"' \
    '    directory: "/"' >"$dependabot"
  printf '%s\n' \
    'name: ci' \
    '          ./scripts/check-repository-security.sh --self-test' \
    '          ./scripts/check-repository-security.sh' >"$workflow"

  check_security "$security" || { fail 'self-test: valid security policy rejected'; exit 1; }
  check_dependabot "$dependabot" || { fail 'self-test: valid Dependabot policy rejected'; exit 1; }
  check_workflow "$workflow" || { fail 'self-test: valid workflow rejected'; exit 1; }

  sed -i '/security\/advisories\/new/d' "$security"
  if check_security "$security" >/dev/null 2>&1; then
    fail 'self-test: policy without the private reporting URL was accepted'; exit 1
  fi
  sed -i '/codewandler-connector/d' "$dependabot"
  if check_dependabot "$dependabot" >/dev/null 2>&1; then
    fail 'self-test: split Flux/connector family was accepted'; exit 1
  fi
  sed -i '/--self-test/d' "$workflow"
  if check_workflow "$workflow" >/dev/null 2>&1; then
    fail 'self-test: workflow without failing-first execution was accepted'; exit 1
  fi

  printf '\033[32mPASS\033[0m self-test: missing reporting, dependency and failing-first contracts are rejected\n'
  exit 0
fi

cd "$(git rev-parse --show-toplevel)"
failures=0
check_security SECURITY.md || failures=$((failures + 1))
check_dependabot .github/dependabot.yml || failures=$((failures + 1))
check_workflow .github/workflows/ci.yml || failures=$((failures + 1))

[ "$failures" -eq 0 ] || exit 1
printf '\033[32mPASS\033[0m security reporting, dependency updates and CI policy are complete\n'
