---
id: X-109
title: Refine flow editor feedback and navigation
status: in-progress
epic: flow-editor
design: docs/designs/flow-editor.md
---

# Refine flow editor feedback and navigation

## Goal
Make the existing visual and source editor safe and legible during ordinary authoring, especially
when a draft has unsaved work or an invalid parameter object.

## Acceptance
- [x] Switching drafts cannot silently discard unsaved title, source or graph edits.
- [x] The editor states whether the draft is saved or modified and explains why publication is
      unavailable.
- [x] Invalid node and run parameter JSON has inline, accessible feedback.
- [x] An empty operation search result is distinguishable from a catalogue that failed to load.
- [ ] Console tests and build pass without adding literal colours outside the token system.

## Progress
- 2026-08-03: story opened after exercising the released editor and finding that selecting another
  draft silently replaced unsaved local state.
- 2026-08-03: dirty-draft navigation now requires an explicit discard, saved/modified and publish
  state remain visible, malformed parameter objects report inline and palette emptiness is distinct
  from catalogue failure. Focused console coverage passes; the full release gate remains.
