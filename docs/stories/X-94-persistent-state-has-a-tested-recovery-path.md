---
id: X-94
title: "Persistent state has a tested recovery path"
status: done
priority: 1
epic: remote-deployment
areas: [deployment, operations]
note: "The encrypted production volume has daily 14-day snapshots, automated freshness monitoring and a completed isolated 597-second recovery drill; the v0.16.1 post-release snapshot is inside the 24-hour RPO."
---

# Persistent state has a tested recovery path

## Goal
Give the one credential-bearing Fly volume an explicit recovery-point policy and prove restoration
in isolation before an incident asks the procedure to work for the first time.

## Acceptance
- [x] Declare scheduled Fly snapshots and 14-day retention in `fly.toml`, and update the already
      existing production volume as a separate verified operation; config for future volumes is not
      evidence about the current one.
- [x] Verify daily that a recent snapshot exists and alert when it is absent or stale. Treat snapshot
      identifiers and all restored data as credential-bearing operational state.
- [x] Document RPO at most 24 hours and RTO at most 60 minutes, including the writes that can be lost
      between the selected snapshot and failure.
- [x] Perform a quarterly restore drill into a new encrypted, isolated volume. Never attach the
      restored copy beside the active store or let two machines accept writes.
- [x] In the drill, verify ownership and modes, open all four stores, confirm grants and connection
      metadata, start one replacement machine, run health/security-header checks and then destroy
      the drill copy under the decommission procedure.
- [x] Define an independent recovery measure for important single-volume data, following Fly's
      warning that daily snapshots are not a complete backup plan:
      <https://fly.io/docs/volumes/snapshots/>.
- [x] Record dated restore evidence without recording store contents. Produce a versioned Fly
      release and live-verify the retention and newest snapshot after rollout.

## Evidence

- 2026-08-03 — `fly.toml` explicitly enables daily snapshots and declares 14-day retention for future
  volumes. `scripts/verify-fly-snapshot.sh` has passing fresh/stale/non-disclosure fixtures and checks
  the live volume's uniqueness, encryption, scheduling, retention and newest completed point without
  emitting a volume or snapshot id.
- 2026-08-03 — the existing encrypted volume was updated separately to scheduled snapshots with
  14-day retention. The live verifier passed against the newest completed point, about 13.5 hours
  old at verification, without emitting its volume or snapshot identifier.
- 2026-08-03 — `.github/workflows/snapshot-watch.yml` runs daily in the production environment,
  retains identifier-free evidence, opens one private issue on absence/staleness/misconfiguration and
  closes it on recovery. `docs/deploying.md` defines the 24-hour loss window, 60-minute timed recovery,
  no-two-writers drill order, all current stores and mode checks, decommission order, evidence fields,
  and a daily age-encrypted copy in an independent provider account.
- 2026-Q3 — a new encrypted one-day-retention volume was restored from the 2026-08-03T01:20:45Z
  completed snapshot while detached. The sole production writer was stopped before one private,
  DNS-unregistered drill machine was attached. The restored process ran as uid 10001; every
  materialized store directory/file was `0700`/`0600`; startup opened credentials, Service Accounts,
  grants, workflows and audit state; connection/grant routes returned structured guarded responses;
  and private health, console, descriptor, CSP, HSTS, `nosniff`, referrer, permissions and API
  `no-store` checks passed. The drill machine was stopped and destroyed, then the restored volume
  was destroyed before production restarted. Elapsed recovery/decommission time was 597 seconds,
  well below the 60-minute RTO. Public health and the one-machine/one-encrypted-volume topology
  passed after restart. No machine, volume, snapshot or store identifier was retained.
- 2026-08-03 — after the v0.16.1 workflow release, the verifier again found exactly one encrypted
  production volume with scheduled snapshots and 14-day retention. The newest completed snapshot
  was about 15.8 hours old, inside the 24-hour RPO. The retained evidence names only timestamps,
  age, policy and topology counts; no volume, snapshot or store identifier or content was recorded.
