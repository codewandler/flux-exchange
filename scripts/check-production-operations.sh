#!/usr/bin/env bash
set -euo pipefail

fail() {
  printf 'production operations contract: %s\n' "$1" >&2
  return 1
}

check_root() {
  local root="$1"
  local dockerfile="$root/Dockerfile"
  local fly_config="$root/fly.toml"
  local deploy_workflow="$root/.github/workflows/production.yml"
  local snapshot_workflow="$root/.github/workflows/snapshot-watch.yml"
  local base_count digest_count build_count locked_count

  for required in "$dockerfile" "$fly_config" "$deploy_workflow" "$snapshot_workflow"; do
    [ -f "$required" ] || { fail "missing ${required#"$root"/}"; return 1; }
  done

  base_count="$(sed -nE '/^[[:space:]]*FROM[[:space:]]+/p' "$dockerfile" | wc -l | tr -d ' ')"
  digest_count="$(sed -nE '/^[[:space:]]*FROM[[:space:]]+[^[:space:]]+@sha256:[0-9a-f]{64}([[:space:]]+AS[[:space:]]+[^[:space:]]+)?[[:space:]]*$/p' "$dockerfile" | wc -l | tr -d ' ')"
  [ "$base_count" -gt 0 ] || { fail 'Dockerfile has no base stages'; return 1; }
  [ "$base_count" = "$digest_count" ] || { fail 'every Dockerfile base must use an immutable sha256 digest'; return 1; }

  build_count="$(sed -nE '/^[[:space:]]*(RUN[[:space:]]+|&&[[:space:]]+)?cargo build/p' "$dockerfile" | wc -l | tr -d ' ')"
  locked_count="$(sed -nE '/^[[:space:]]*(RUN[[:space:]]+|&&[[:space:]]+)?cargo build .*--locked/p' "$dockerfile" | wc -l | tr -d ' ')"
  [ "$build_count" -gt 0 ] || { fail 'Dockerfile has no Cargo build'; return 1; }
  [ "$build_count" = "$locked_count" ] || { fail 'every container Cargo build must use --locked'; return 1; }

  grep -Eq 'snapshot_retention[[:space:]]*=[[:space:]]*14' "$fly_config" || {
    fail 'fly.toml must retain volume snapshots for 14 days'; return 1;
  }
  grep -Eq 'scheduled_snapshots[[:space:]]*=[[:space:]]*true' "$fly_config" || {
    fail 'fly.toml must explicitly enable scheduled snapshots'; return 1;
  }

  grep -Eq 'environment:[[:space:]]*$' "$deploy_workflow" || { fail 'production deploy must name an environment'; return 1; }
  grep -Eq 'name:[[:space:]]*production' "$deploy_workflow" || { fail 'production deploy must use the production environment'; return 1; }
  grep -Eq 'scripts/verify-production-release\.sh' "$deploy_workflow" || {
    fail 'production deploy must run the post-deploy verifier'; return 1;
  }
  grep -Eq 'anchore/scan-action@[0-9a-f]{40}' "$deploy_workflow" || {
    fail 'production deploy must scan the image with a SHA-pinned action'; return 1;
  }
  grep -Eq 'anchore/sbom-action@[0-9a-f]{40}' "$deploy_workflow" || {
    fail 'production deploy must emit an SBOM with a SHA-pinned action'; return 1;
  }

  grep -Eq '^  schedule:' "$snapshot_workflow" || { fail 'snapshot verification must run on a schedule'; return 1; }
  grep -Eq 'scripts/verify-fly-snapshot\.sh' "$snapshot_workflow" || {
    fail 'snapshot workflow must run the bounded snapshot verifier'; return 1;
  }
}

self_test() {
  local fixture_dir
  fixture_dir="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-production.XXXXXX")"
  trap 'rm -rf "$fixture_dir"' RETURN
  mkdir -p "$fixture_dir/.github/workflows"
  printf '%s\n' 'FROM base@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa AS build' \
    'RUN cargo build --locked' >"$fixture_dir/Dockerfile"
  printf '%s\n' '[mounts]' 'snapshot_retention = 14' 'scheduled_snapshots = true' >"$fixture_dir/fly.toml"
  printf '%s\n' 'environment:' '  name: production' \
    'uses: anchore/scan-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
    'uses: anchore/sbom-action@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' \
    'run: scripts/verify-production-release.sh' >"$fixture_dir/.github/workflows/production.yml"
  printf '%s\n' 'on:' '  schedule:' 'run: scripts/verify-fly-snapshot.sh' >"$fixture_dir/.github/workflows/snapshot-watch.yml"
  check_root "$fixture_dir"

  sed -i 's/@sha256:[a-f0-9]*/:latest/' "$fixture_dir/Dockerfile"
  if check_root "$fixture_dir" >/dev/null 2>&1; then
    fail 'self-test accepted a movable container base'
    return 1
  fi
  printf 'production operations self-test passed\n'
}

if [ "${1:-}" = '--self-test' ]; then
  self_test
else
  check_root .
  printf 'production operations contract is complete\n'
fi
