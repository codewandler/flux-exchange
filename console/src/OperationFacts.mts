// The two things the service publishes about an operation that the carried catalogue contract has
// no field for: its effects, and whether the caller may call it.
//
// Both are here rather than in `catalog.mts` because that contract is shared with flux-connectors
// and this console does not get to extend it, and neither is dropped because between them they are
// most of what a grant is written over — an agent that cannot see an operation's effects cannot
// predict what a `Selector` admits.
//
// **The two claims this element refuses to make.**
//
//   - `effects_derived: true` means the service *inferred* the effects from the operation itself
//     rather than reading a declaration. Printing them the same way in both cases would launder an
//     inference into a published fact, so the footnote says which one this is, every time.
//   - `admitted` is three-valued and `null` is not `false`. `null` means no principal was resolved,
//     so the catalogue is saying what *exists*; rendering that as "denied" would tell a reader they
//     had been refused something nobody has yet asked on their behalf.
//
// A render function rather than a single-file component, for the reason set out in
// `CatalogueFailure.mts`: this is asserted by a test that runs under plain Node.

import { defineComponent, h, type PropType } from 'vue'
import type { ServedOperation } from './service.mts'

/** How this operation's effects came to be known, in one sentence under them. */
function provenance(operation: ServedOperation): string {
  if (operation.effects_derived) {
    return operation.effects.length
      ? 'Inferred by the service from the operation itself — the connector declares no effects of its own.'
      : 'The service inferred no effects for this operation. Nothing here was declared.'
  }
  return operation.effects.length
    ? 'Declared by the connector.'
    : 'Declared by the connector: this operation has no effects.'
}

/** What the service said about calling this operation, and what it deliberately did not say. */
function admission(operation: ServedOperation): string {
  if (operation.admitted === true) return 'Admitted for the resolved principal.'
  if (operation.admitted === false) return 'Refused for the resolved principal.'
  return 'Not evaluated — no principal is resolved, so the catalogue is telling you what exists rather than what you may call. Nothing here has been withheld from you.'
}

export default defineComponent({
  name: 'OperationFacts',
  props: {
    operation: { type: Object as PropType<ServedOperation>, required: true },
  },
  setup(props) {
    return () =>
      h('section', { class: 'facts', 'data-admitted': String(props.operation.admitted) }, [
        h('h2', null, 'Effects'),
        props.operation.effects.length
          ? h(
              'p',
              { class: 'facts__effects' },
              props.operation.effects.map((effect) => h('code', { key: effect }, effect))
            )
          : null,
        h(
          'p',
          { class: 'facts__note', 'data-derived': String(props.operation.effects_derived) },
          provenance(props.operation)
        ),

        h('h2', null, 'Admission'),
        h('p', { class: 'facts__note' }, admission(props.operation)),
      ])
  },
})
