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
  local config_verifier="$root/scripts/verify-production-config.sh"
  local release_verifier="$root/scripts/verify-production-release.sh"
  local snapshot_verifier="$root/scripts/verify-fly-snapshot.sh"
  local base_count immutable_count build_count locked_count preflight_line image_build_line

  for required in "$dockerfile" "$fly_config" "$deploy_workflow" "$snapshot_workflow" \
    "$config_verifier" "$release_verifier" "$snapshot_verifier"; do
    [ -f "$required" ] || { fail "missing ${required#"$root"/}"; return 1; }
  done

  base_count="$(sed -nE '/^[[:space:]]*FROM[[:space:]]+/p' "$dockerfile" | wc -l | tr -d ' ')"
  # Docker's built-in empty `scratch` root has no registry object to digest-pin. It is the only
  # digest-free base admitted here; every fetched base remains an immutable sha256 reference.
  immutable_count="$(sed -nE '/^[[:space:]]*FROM[[:space:]]+(scratch|[^[:space:]]+@sha256:[0-9a-f]{64})([[:space:]]+AS[[:space:]]+[^[:space:]]+)?[[:space:]]*$/p' "$dockerfile" | wc -l | tr -d ' ')"
  [ "$base_count" -gt 0 ] || { fail 'Dockerfile has no base stages'; return 1; }
  [ "$base_count" = "$immutable_count" ] || { fail 'every fetched Dockerfile base must use an immutable sha256 digest'; return 1; }

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
  grep -Eq 'scripts/verify-production-config\.sh --self-test' "$deploy_workflow" || {
    fail 'the repository gate must self-test the production configuration verifier'; return 1;
  }
  preflight_line="$(awk '/scripts\/verify-production-config\.sh[[:space:]]*$/ { line = NR } END { print line }' "$deploy_workflow")"
  image_build_line="$(awk '/name:[[:space:]]*Build exactly the selected checkout/ { print NR; exit }' "$deploy_workflow")"
  [ -n "$preflight_line" ] && [ -n "$image_build_line" ] && [ "$preflight_line" -lt "$image_build_line" ] || {
    fail 'production must verify operator configuration before building an image'; return 1;
  }
  grep -Eq 'anchore/scan-action@[0-9a-f]{40}' "$deploy_workflow" || {
    fail 'production deploy must scan the image with a SHA-pinned action'; return 1;
  }
  grep -Eq 'anchore/sbom-action@[0-9a-f]{40}' "$deploy_workflow" || {
    fail 'production deploy must emit an SBOM with a SHA-pinned action'; return 1;
  }

  grep -Eq 'flyctl secrets list .*--json' "$config_verifier" || {
    fail 'production configuration verification must read Fly secret metadata'; return 1;
  }
  grep -Fq 'FLUX_EXCHANGE_OPERATOR_SUBJECTS' "$config_verifier" || {
    fail 'production configuration verification must require the operator policy'; return 1;
  }
  grep -Fq 'Deployed' "$config_verifier" || {
    fail 'production configuration verification must require deployed status'; return 1;
  }
  grep -Eq 'operator_policy:[[:space:]]*"deployed"' "$release_verifier" || {
    fail 'production evidence must record the value-free operator policy result'; return 1;
  }

  grep -Eq '^  schedule:' "$snapshot_workflow" || { fail 'snapshot verification must run on a schedule'; return 1; }
  grep -Eq 'scripts/verify-fly-snapshot\.sh' "$snapshot_workflow" || {
    fail 'snapshot workflow must run the bounded snapshot verifier'; return 1;
  }

  if grep -Eq '(^|[[:space:]])rg[[:space:]]' "$config_verifier" "$release_verifier" "$snapshot_verifier"; then
    fail 'workflow verifiers must not depend on ripgrep being preinstalled'; return 1
  fi
}

self_test() {
  local fixture_dir
  fixture_dir="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-production.XXXXXX")"
  trap 'rm -rf "$fixture_dir"' RETURN
  mkdir -p "$fixture_dir/.github/workflows" "$fixture_dir/scripts"
  printf '%s\n' 'FROM base@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa AS build' \
    'RUN cargo build --locked' 'FROM scratch AS runtime' >"$fixture_dir/Dockerfile"
  printf '%s\n' '[mounts]' 'snapshot_retention = 14' 'scheduled_snapshots = true' >"$fixture_dir/fly.toml"
  printf '%s\n' 'environment:' '  name: production' \
    'run: scripts/verify-production-config.sh --self-test' \
    'run: scripts/verify-production-config.sh' \
    'name: Build exactly the selected checkout' \
    'uses: anchore/scan-action@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa' \
    'uses: anchore/sbom-action@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb' \
    'run: scripts/verify-production-release.sh' >"$fixture_dir/.github/workflows/production.yml"
  printf '%s\n' 'on:' '  schedule:' 'run: scripts/verify-fly-snapshot.sh' >"$fixture_dir/.github/workflows/snapshot-watch.yml"
  printf '%s\n' 'flyctl secrets list --app "$APP" --json' \
    'FLUX_EXCHANGE_OPERATOR_SUBJECTS' 'Deployed' >"$fixture_dir/scripts/verify-production-config.sh"
  printf '%s\n' 'operator_policy: "deployed"' >"$fixture_dir/scripts/verify-production-release.sh"
  printf '%s\n' ':' >"$fixture_dir/scripts/verify-fly-snapshot.sh"
  check_root "$fixture_dir"

  cp "$fixture_dir/.github/workflows/production.yml" "$fixture_dir/production.valid"
  sed -i '/^run: scripts\/verify-production-config\.sh$/d' "$fixture_dir/.github/workflows/production.yml"
  if check_root "$fixture_dir" >/dev/null 2>&1; then
    fail 'self-test accepted a production workflow without the operator-policy preflight'
    return 1
  fi
  mv "$fixture_dir/production.valid" "$fixture_dir/.github/workflows/production.yml"

  sed -i 's/FLUX_EXCHANGE_OPERATOR_SUBJECTS/ANOTHER_SETTING/' "$fixture_dir/scripts/verify-production-config.sh"
  if check_root "$fixture_dir" >/dev/null 2>&1; then
    fail 'self-test accepted a verifier that did not require the operator policy'
    return 1
  fi
  sed -i 's/ANOTHER_SETTING/FLUX_EXCHANGE_OPERATOR_SUBJECTS/' "$fixture_dir/scripts/verify-production-config.sh"

  sed -i 's/Deployed/Staged/' "$fixture_dir/scripts/verify-production-config.sh"
  if check_root "$fixture_dir" >/dev/null 2>&1; then
    fail 'self-test accepted an operator policy that was not deployed'
    return 1
  fi
  sed -i 's/Staged/Deployed/' "$fixture_dir/scripts/verify-production-config.sh"

  printf '%s\n' 'rg -q expected file' >"$fixture_dir/scripts/verify-production-release.sh"
  if check_root "$fixture_dir" >/dev/null 2>&1; then
    fail 'self-test accepted an undeclared workflow-runner dependency'
    return 1
  fi
  printf '%s\n' ':' >"$fixture_dir/scripts/verify-production-release.sh"

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
