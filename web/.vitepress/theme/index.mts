// The default VitePress theme, plus one thing: the derived capability status, in the page chrome.
//
// `doc-before` is the default theme's slot immediately above a page's rendered markdown — above the
// page's own `<h1>`, and therefore above the first sentence a reader could take as a claim. That
// position is the requirement rather than a preference: `docs/designs/public-docs-site.md` §2 says
// *"planned pages carry the marker in the page chrome, not in a paragraph three screens down,
// because the way the five renderings went wrong was that the caveat and the claim drifted apart"*,
// and `web/test/status.test.mjs` asserts the badge renders before the heading.
//
// Extending rather than replacing: everything else about this site is the default theme, and a
// custom theme here would be twenty decisions taken in order to make one.

import DefaultTheme from 'vitepress/theme'
import { h } from 'vue'

import CapabilityStatus from './CapabilityStatus.mts'
import './capability-status.css'

export default {
  extends: DefaultTheme,
  Layout: () =>
    h(DefaultTheme.Layout, null, {
      'doc-before': () => h(CapabilityStatus),
    }),
}
