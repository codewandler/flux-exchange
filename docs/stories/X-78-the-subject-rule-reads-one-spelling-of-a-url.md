---
id: X-78
title: "The family-link rule catches one spelling of a repository URL, and its comment says it catches the kind"
status: backlog
epic: public-docs-site
design: docs/designs/public-docs-site.md
areas: [web]
note: "found by X-77's independent review, 2026-08-02: subjectIsTheProject matches the exact canonical href, so [flux](http://github.com/codewandler/flux) — plain http — passes silently with the project's own name as the anchor text. The narrowness is real and the comment that web/README.md calls the statement of record does not mention it"
---

# The family-link rule catches one spelling of a repository URL, and its comment says it catches the kind

## Goal
X-77's guard refuses a family link however it is spelled, or its comment says exactly which spellings
it reads.

## What the review found

X-77's `subjectIsTheProject` decides whether a github.com link is *about the project* (must point at
the documentation site) or *about the repository* (legitimately stays). The href half compares against
the exact canonical form — `https://github.com/codewandler/<name>`, with or without a trailing slash.
Three spellings therefore pass with the project's own name as the anchor text:

| Spelling | Caught by | Silent? |
|---|---|---|
| `http://github.com/codewandler/flux` | nothing | **yes** |
| `https://github.com/codewandler/flux?tab=readme-ov-file` | nothing | **yes** |
| `https://www.github.com/codewandler/flux` | the host allow-list, one test over | no |

The text half's narrowness **is** documented — `site.test.mjs:498-503` states that
`[the flux engine](…/flux)` is deliberately admitted, because a rule over prose would be a guess. This
one is not, and `web/README.md` calls that comment *"the statement of record"*.

## Why it is worth a story rather than a one-line fix

This is the shape this repository keeps correcting, and X-52 is the precedent: **a guard whose name
claims a category while its body checks an instance.** The fix is probably three lines — normalise the
scheme, drop the query, strip a `www.` — but the decision underneath is which of two things the rule
is: *the canonical link is wrong* or *any link to this repository, however written, is wrong*. Pick
one and say so, because the next contributor reads the comment and not this story.

Note that the pressure is low and the correct answer may be **document the narrowness rather than
widen it**. Nothing on the site uses these spellings today; the guard fires on what is actually
written, and X-77's own failing-first proof exercised it against the real pages.

## Acceptance
- [ ] Decide: normalise, or document. One sentence in the test comment either way.
- [ ] **Failing-first test** — if normalising, a page carrying `[flux](http://github.com/codewandler/flux)`
      fails the guard. Watch it pass first; that is the whole finding.
- [ ] `web/README.md`'s statement of record agrees with whatever the comment now says. The two
      disagreeing is what made this a finding rather than a note.

## Progress
- (not started)

## Notes
- Found by the independent review of [[X-77]], 2026-08-02, not by its implementor — the implementor
  documented the *text* half's width and missed the *href* half's narrowness.
- The same review recorded a **pre-existing** hole it confirmed is untouched by X-77:
  `https://github.com@evil.example/` scans as host `github.com` and passes the off-site allow-list at
  `site.test.mjs:186`, because the host regex reads the userinfo. Byte-identical at the merge base.
  That is [[X-19]]'s defect class — a URL parser disagreeing with the parser that matters — on a test
  rather than on a request path, so it costs a wrong *pass* on a site scan and not a leaked secret.
  Worth folding into this story's decision, since both are "which parser does this rule use".
