# Changelog

All notable changes to this project are documented in this file. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **The backlog** — vision, roadmap, and thirteen stories across four epics (X-01…X-13), plus the
  operating contract in `AGENTS.md`. The first wave is eight ready stories: the HTTP surface,
  sign-in, the catalogue and the credential store.

## [0.0.1] - 2026-08-01

### Added

- **The charter, and the rules as tested types.** `crates/exchange-host` carries `Principal`/`Tenant`,
  `Grant`/`Selector`, `Runtime`/`Deployment`, `Lease` and the `Identity` port, with 19 tests. Four
  rules are executed rather than described: a tenant id that would traverse its credential-address
  prefix is refused at construction; a multi-tenant deployment refuses every locally-executing
  runtime, naming what would have worked; a grant selects by declared metadata with deny beating
  allow; and a lease requires the same principal, not merely the same tenant.
- **A binary that reports and exits**, deliberately not a service.
- **A console** over the 15 framework-free explorer components carried from flux-connectors,
  rendering fixture data behind a banner that says so, with the components' no-framework-import
  invariant ported and strengthened.
