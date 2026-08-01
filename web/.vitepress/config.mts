import { defineConfig } from 'vitepress'

import { assertDescriptorIsCurrent, statusFor } from './descriptor.mts'

const repo = 'https://github.com/codewandler/flux-exchange'
const base = '/flux-exchange/'

// Where the siblings publish. A link about *what a sibling is* goes here rather than to its
// repository, so that the three sites read as one product instead of three source trees (X-77) —
// `test/site.test.mjs`'s `subjectIsTheProject` states the rule and holds it, and it turns on the
// link's subject rather than on its hostname: `repo` above is still the right address for a clone
// URL, a releases page or a heading in the README.
//
// Verified 2026-08-02: both answer 200 and the Pages API reports no cname for either, so these are
// the addresses rather than redirects to nicer ones.
const flux = 'https://codewandler.github.io/flux/'
const fluxConnectors = 'https://codewandler.github.io/flux-connectors/'

// **Before anything is rendered**, and deliberately at config load rather than inside a hook: the
// agent descriptor every status badge on this site derives from is a *committed copy* of a
// derivation, and a copy can rot. `console/test/descriptor.test.mjs` fails when it has, but `web/`
// and `console/` are separate Node trees with separate lockfiles that can be tested apart — so a
// site that derived its badges from that artifact and assumed somebody else had checked it would be
// stale in the one place it advertises as derived. This re-derives and compares (X-64).
assertDescriptorIsCurrent()

export default defineConfig({
  lang: 'en-US',
  title: 'flux-exchange',
  description:
    'The platform layer of the flux family: a service that holds credentials so a caller never has to.',

  // This must match where GitHub actually serves the site, which is
  // https://codewandler.github.io/flux-exchange/ — every asset URL and root-relative link resolves
  // against it, so a wrong prefix 404s the stylesheet and the page renders unstyled.
  //
  // ../flux-connectors paid for the mistake this comment exists to prevent: its `web/public/CNAME`
  // named a custom domain and the base was flipped to '/' on the strength of it. A committed CNAME
  // is a *request* for a custom domain, not evidence one is serving — the Pages API still reported
  // `"cname": null`, and '/' 404s every bundled asset.
  //
  // This repository publishes no CNAME at all, so there is nothing here to be misled by yet. Flip
  // this to '/' **only** once `gh api repos/codewandler/flux-exchange/pages --jq .cname` reports the
  // domain — not when a CNAME file lands, which is what went wrong next door.
  base,

  cleanUrls: true,

  // web/README.md documents how to build this site for a contributor; it is not a published page.
  // Without this it renders at /README.
  srcExclude: ['README.md'],

  // Dead internal links fail the build rather than shipping. Combined with the Pages workflow —
  // which builds on pull requests and deploys only from `main` — that means a broken site cannot
  // publish silently. This is the site's failing-first test (X-63) and it is checked by
  // `test/site.test.mjs`, because the value that makes it a gate is the one an edit can quietly
  // flip to `true` when a link goes red.
  ignoreDeadLinks: false,

  // Every page's "is this built" is derived here, once, and stamped onto the page for the chrome to
  // render (X-64). No page states its own liveness, because no page is given the chance to: an
  // author writes the prose and this writes the status.
  //
  // **It refuses rather than rendering a blank.** A page under `capabilities/` that names no
  // capability, and any page naming one the descriptor does not publish, both throw from here and
  // fail the build. That is the point of doing it in `transformPageData`: a missing status has to be
  // a build failure, because a capability page rendering with no badge tells a reader nothing is
  // wrong — and this repository has already published that particular reassurance five times.
  transformPageData(pageData) {
    pageData.frontmatter.capabilityStatus = statusFor(pageData.relativePath, pageData.frontmatter)
  },

  themeConfig: {
    // `Run it yourself` is first, and that ordering is X-69's point rather than a preference: a
    // visitor could learn what this service refuses to do and could not learn how to start it. The
    // nav carries it on every page because a sidebar entry is something you find only after you
    // have decided to look.
    nav: [
      { text: 'Run it yourself', link: '/getting-started' },
      { text: 'The boundary', link: '/boundary' },
      { text: 'The surface', link: '/surface' },
      // The family on every page, not only on the overview: a reader who arrives at /surface from a
      // search result is one click from the engine and the catalogue. Each entry says which of the
      // three questions its project answers, because "flux" and "flux-connectors" side by side is a
      // pair of names rather than a division of labour.
      {
        text: 'The flux family',
        items: [
          { text: 'flux — the engine', link: flux },
          { text: 'flux-connectors — the catalogue', link: fluxConnectors },
        ],
      },
      // Labelled for its destination now that the entries above it lead to documentation. It means
      // the repository — releases are published there and nowhere else — and that is why it stays
      // on github.com.
      { text: 'Releases (GitHub)', link: `${repo}/releases` },
    ],

    // Three pages were X-63's deliberate floor; `getting-started` is X-69's, and it never waited on
    // the derived badge because it describes how to run the software rather than what a build can
    // do. The `Capabilities` group is where a status *is* the point: every page in it takes its
    // "is this built" from the agent descriptor rather than from its author (X-64).
    //
    // Two of them, and that is the mechanism's floor rather than the intended set — `invoke` and
    // `subscribe`, the two verbs of one binding, one served by this build and one not, so both
    // answers the badge can give are rendered by a real page. X-65 writes the rest of the surface on
    // top of this, which is why the epic is ordered the way it is.
    sidebar: [
      {
        text: 'flux-exchange',
        items: [
          { text: 'Overview', link: '/' },
          { text: 'Run it yourself', link: '/getting-started' },
          { text: 'The credential boundary', link: '/boundary' },
          { text: 'The surface', link: '/surface' },
        ],
      },
      {
        text: 'Capabilities',
        items: [
          { text: 'invoke', link: '/capabilities/invoke' },
          { text: 'subscribe', link: '/capabilities/subscribe' },
        ],
      },
    ],

    socialLinks: [{ icon: 'github', link: repo }],

    footer: {
      message: 'Dual-licensed under MIT or Apache-2.0, at your option.',
      copyright: `<a href="${repo}">codewandler/flux-exchange</a>`,
    },

    outline: [2, 3],
  },
})
