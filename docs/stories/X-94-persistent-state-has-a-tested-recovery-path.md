---
id: X-94
title: "Persistent state has a tested recovery path"
status: ready
priority: 1
epic: remote-deployment
areas: [deployment, operations]
note: "The one production volume was encrypted but listed no snapshots on 2026-08-02; no recovery point or restore time has been demonstrated."
---

# Persistent state has a tested recovery path

## Goal
Give the one credential-bearing Fly volume an explicit recovery-point policy and prove restoration
in isolation before an incident asks the procedure to work for the first time.

## Acceptance
- [ ] Declare scheduled Fly snapshots and 14-day retention in `fly.toml`, and update the already
      existing production volume as a separate verified operation; config for future volumes is not
      evidence about the current one.
- [ ] Verify daily that a recent snapshot exists and alert when it is absent or stale. Treat snapshot
      identifiers and all restored data as credential-bearing operational state.
- [ ] Document RPO at most 24 hours and RTO at most 60 minutes, including the writes that can be lost
      between the selected snapshot and failure.
- [ ] Perform a quarterly restore drill into a new encrypted, isolated volume. Never attach the
      restored copy beside the active store or let two machines accept writes.
- [ ] In the drill, verify ownership and modes, open all four stores, confirm grants and connection
      metadata, start one replacement machine, run health/security-header checks and then destroy
      the drill copy under the decommission procedure.
- [ ] Define an independent recovery measure for important single-volume data, following Fly's
      warning that daily snapshots are not a complete backup plan:
      <https://fly.io/docs/volumes/snapshots/>.
- [ ] Record dated restore evidence without recording store contents. Produce a versioned Fly
      release and live-verify the retention and newest snapshot after rollout.
