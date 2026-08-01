import { defineConfig } from 'vitepress'

const repo = 'https://github.com/codewandler/flux-exchange'
const base = '/flux-exchange/'

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
    nav: [
      { text: 'The boundary', link: '/boundary' },
      { text: 'The surface', link: '/surface' },
      { text: 'Releases', link: `${repo}/releases` },
    ],

    // Three pages, deliberately (X-63). The volume — one page per capability, each carrying a
    // status derived from the route table rather than written by hand — arrives with X-64 and X-65,
    // in that order, because the mechanism has to land before the pages that depend on it.
    sidebar: [
      {
        text: 'flux-exchange',
        items: [
          { text: 'Overview', link: '/' },
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
