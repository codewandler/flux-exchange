// A real Vue mount, in a process with no DOM.
//
// **Why this exists.** Every other test in this console renders through `vue/server-renderer`,
// which is exactly right for asserting what a page *says*: one render, one string, no lifecycle.
// It cannot assert what a page *does*. `test/agents.test.mjs` has to, because the property that
// story is about is temporal — a token appears once, in response to a click, and is gone when the
// reader leaves — and "gone when the reader leaves" is a statement about unmounting that a function
// returning a string cannot make. A structural argument (nothing outside the view holds it) is
// worth having and is asserted too, but it is not the same claim.
//
// **Why not a DOM library.** Adding one is adding a dependency, and this console has four
// devDependencies on purpose. Vue's own `createRenderer` is the seam for exactly this: the runtime
// is platform-agnostic, and `@vue/runtime-dom` is nothing but these same operations implemented
// against `document`. So the tree below is the smallest possible platform — objects with a tag,
// props and children — and the component under test cannot tell the difference, because it only
// ever asked Vue to render.
//
// **What it is not.** Not a DOM: no layout, no selectors, no events bubbling, no `FormData`. An
// element's handlers are ordinary props, so "clicking" is calling one. That is a real limitation
// and it shapes what the screen may do — a screen that read its inputs out of a live `FormData`
// could not be driven here. `Connect.mts` does exactly that and is tested through SSR instead;
// `Agents.mts` holds an agent's name and expiry in state, which are not credentials, so it can be.

import { createRenderer, nextTick } from 'vue'

/** Detach a node from wherever it currently is. Insert moves nodes, so this runs on every insert. */
function detach(node) {
  const parent = node.parent
  if (!parent) return
  const at = parent.children.indexOf(node)
  if (at !== -1) parent.children.splice(at, 1)
  node.parent = null
}

/**
 * The platform, as Vue's runtime asks for it.
 *
 * Every operation here is the obvious one over an array of children. `patchProp` stores the value
 * verbatim rather than translating it — so `onClick` stays the function the component passed, which
 * is what lets a test invoke it.
 */
const operations = {
  createElement: (tag) => ({ kind: 'element', tag, props: {}, children: [], text: null, parent: null }),
  createText: (text) => ({ kind: 'text', props: {}, children: [], text, parent: null }),
  createComment: (text) => ({ kind: 'comment', props: {}, children: [], text, parent: null }),
  setText: (node, text) => {
    node.text = text
  },
  setElementText: (node, text) => {
    node.children = []
    node.text = text
  },
  insert: (child, parent, anchor) => {
    detach(child)
    const at = anchor ? parent.children.indexOf(anchor) : -1
    if (at === -1) parent.children.push(child)
    else parent.children.splice(at, 0, child)
    child.parent = parent
  },
  remove: detach,
  parentNode: (node) => node.parent,
  nextSibling: (node) => {
    const parent = node.parent
    if (!parent) return null
    return parent.children[parent.children.indexOf(node) + 1] ?? null
  },
  querySelector: () => null,
  setScopeId: () => {},
  patchProp: (element, key, _previous, next) => {
    element.props[key] = next
  },
}

const { createApp } = createRenderer(operations)

/** Every node in the tree, root first, in document order. */
export function nodes(root) {
  const found = []
  const walk = (node) => {
    found.push(node)
    for (const child of node.children) walk(child)
  }
  walk(root)
  return found
}

/**
 * Everything a reader would see, as one string.
 *
 * Text nodes and `setElementText` content, in document order, separated by a space so two adjacent
 * words do not run together into a third one nobody rendered.
 */
export function text(root) {
  return nodes(root)
    .map((node) => (node.kind === 'comment' ? '' : (node.text ?? '')))
    .filter((value) => value !== '')
    .join(' ')
}

/** Every attribute value on every node, as one string — where a value rendered into markup lands. */
export function attributes(root) {
  return nodes(root)
    .flatMap((node) => Object.entries(node.props))
    .filter(([, value]) => typeof value === 'string' || typeof value === 'number')
    .map(([key, value]) => `${key}=${value}`)
    .join(' ')
}

/** Text and attributes together: everything of the component that reached the platform. */
export const rendered = (root) => `${text(root)} ${attributes(root)}`

/** Every element carrying `attribute="value"`, in document order. */
export const find = (root, attribute, value) =>
  nodes(root).filter((node) => node.props[attribute] === value)

/** The one element carrying `attribute="value"`, or `null` when there is none. */
export const one = (root, attribute, value) => find(root, attribute, value)[0] ?? null

/**
 * Mount a component and hand back the tree it produced.
 *
 * `unmount` is the interesting half: it is what navigating away does, and the whole question the
 * mint screen exists to answer is what survives it.
 */
export function mount(component, props = {}) {
  const root = { kind: 'element', tag: 'root', props: {}, children: [], text: null, parent: null }
  const app = createApp(component, props)
  app.mount(root)
  return {
    root,
    unmount: () => app.unmount(),
    /** Fire a handler the component rendered, then let the resulting re-render settle. */
    fire: async (node, handler, event = {}) => {
      const listener = node.props[handler]
      if (typeof listener !== 'function') {
        throw new Error(`no \`${handler}\` handler on <${node.tag}>`)
      }
      await listener({ preventDefault: () => {}, ...event })
      await nextTick()
    },
  }
}
