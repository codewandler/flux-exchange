---
id: X-66
title: "The credential-boundary argument is readable by someone who has never seen this repository"
status: done
priority: 2
epic: public-docs-site
design: docs/designs/public-docs-site.md
areas: [web]
note: "the page that justifies the platform existing: the credential never crosses the boundary, the authority does — written for an evaluator rather than for a contributor"
---

# The credential-boundary argument is readable by someone who has never seen this repository

## Goal
A reader who has never seen this code can understand what flux-exchange refuses to do, and why that
is the product.

## Why this page and not a README link

The north star — **the credential never crosses the boundary; the authority does** — is currently
argued in `docs/vision.md` and `docs/designs/invoke.md`, both written for people building this. They
assume the vocabulary they define. An evaluator asking *"why would I put my credentials here?"*
should not have to read a design record with its own corrections in place.

The material is unusually strong and mostly already written; what is missing is a rendering aimed
outward.

## What it has to carry

- **The domain test** — *does it require holding a credential or knowing a tenant?* — which is what
  makes this a separate thing from the engine and the connector catalogue.
- **The outbound argument**: a caller names an operation and nothing else. Not the host, not the
  credential, not the tenant. Everything else is derived from the operation's own compiled Flux and
  from the principal this host resolved.
- **The inbound mirror** (`vision.md:50`): a subscriber cannot name a binding it has not been granted.
- **What it refuses, with the reasons that were expensive to learn.** The strongest material on this
  site is the corrections, and they should be public rather than buried:
  - a character allow-list constrains what a value **looks like**, not where a request goes;
  - **a suffix pin constrains which vendor a request reaches, not whose account at that vendor** —
    `*.zendesk.com` is self-service registrable. Whole-authority templates without a catalogue-declared
    closed choice are refused outright;
  - a name check catches the spelling somebody wrote, not the capability they reached.
- **Grants decide from what an operation declares**, never from a list of names — and *why* that
  distinction keeps mattering.

## Acceptance
- [x] The page stands alone: no unexplained internal vocabulary, no link required to follow the
      argument.
- [x] Every refusal it claims is one the code enforces. **Check each against a test**, and cite it, so
      this page is falsifiable the way the rest of the repository is.
- [x] It states what is **not** closed as plainly as what is — an authorized operator of the tenant
      who did not supply a credential can still expose its authority via a suffix-pinned host.
      X-91's operator policy and X-60's supplier evidence make that action attributable and
      operator-scoped; they do not turn supplier provenance into an authorization boundary.
- [x] Derived status where it makes a claim about a capability ([[X-64]]).
- [x] No configuration example contains anything credential-shaped, however obviously fake. A copyable
      example is a copied example.

## Notes
- This is the page most likely to be read and least likely to be checked, which is exactly why its
  claims must cite tests.

## Progress
- 2026-08-03: Re-measured the older acceptance language against X-60, X-70 and X-91 before writing
  public prose. The current catalogue census has catalogue-declared closed choices for Intercom and
  New Relic; the public argument names the general rule rather than preserving the obsolete count.
- 2026-08-03: Published the standalone domain test, outbound and inbound arguments, three expensive
  corrections, test citations and the remaining supplier-versus-operator gap. The public build and
  total rendered-content gate pass.
