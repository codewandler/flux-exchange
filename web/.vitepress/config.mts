import { defineConfig } from 'vitepress'

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

    // Four pages. Three were X-63's deliberate floor and the fourth is X-69's getting-started page,
    // which is not the "volume" that story deferred: it describes how to run the software rather
    // than claiming what any build can do, so it does not wait on the derived status badge. That
    // volume — one page per capability, each carrying a status derived from the route table rather
    // than written by hand — still arrives with X-64 and X-65, in that order, because the mechanism
    // has to land before the pages that depend on it.
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
    ],

    socialLinks: [{ icon: 'github', link: repo }],

    footer: {
      message: 'Dual-licensed under MIT or Apache-2.0, at your option.',
      copyright: `<a href="${repo}">codewandler/flux-exchange</a>`,
    },

    outline: [2, 3],
  },
})
