// What under `web/` is site content, stated once, for the publisher and for the suite that checks it.
//
// **Why one constant rather than two lists that agree.** X-64's first rework gave the test suite a
// list of directories to skip when predicting which pages the site publishes. The config had its own
// separate `srcExclude`. Two statements of one fact, and the failure they produce is not that they
// disagree — it is that they *agree wrongly*: the suite predicts no page under `test/`, VitePress
// publishes one anyway, and the coverage check that exists to catch exactly this compares one blind
// list against another and reports success.
//
// So the exclusion lives here and both sides read it. Changing what is content is one edit, and it
// moves the publisher and the predictor together.
//
// **This is the source side only.** Nothing here is applied when walking the *built* site — see
// `web/test/rendered.mjs`, where that distinction is the whole subject. A directory being "not
// content" is a claim about what should be published; it is never a licence to skip reading
// something that was.

/**
 * Directories directly under `web/` that hold no page.
 *
 * `node_modules` is the one that matters by volume. `test/`, `scripts/` and `.vitepress/` are this
 * site's own machinery, and a markdown file dropped in any of them was — until this constant existed
 * — published to the public site as a page. `public/` is copied verbatim as static assets rather
 * than rendered.
 */
export const NOT_CONTENT = ['node_modules', '.vitepress', 'test', 'scripts', 'public']

/**
 * What VitePress must not publish, in the glob form `srcExclude` takes.
 *
 * `README.md` is the contributor readme at the root: a real file that would otherwise render at
 * `/README`, carrying build instructions and internal story ids onto a public site.
 */
export const SRC_EXCLUDE = ['README.md', ...NOT_CONTENT.map((directory) => `${directory}/**`)]
