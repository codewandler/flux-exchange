// A searchable, keyboard-operable connector choice using the exchange catalogue's human facts.

import { computed, defineComponent, h, ref, watch, type PropType } from 'vue'
import type { Connector } from './catalog.mts'

export default defineComponent({
  name: 'ConnectorPicker',
  props: {
    connectors: { type: Array as PropType<Connector[]>, required: true },
    connected: { type: Array as PropType<string[]>, default: () => [] },
    value: { type: String, default: '' },
    label: { type: String, default: 'Connector' },
    disabled: { type: Boolean, default: false },
  },
  emits: ['choose'],
  setup(props, { emit }) {
    const query = ref('')
    const open = ref(false)
    const active = ref(0)
    const connected = computed(() => new Set(props.connected))
    const matching = computed(() => {
      const terms = query.value.trim().toLowerCase().split(/\s+/).filter(Boolean)
      return props.connectors.filter((connector) => {
        const facts = `${connector.vendor} ${connector.id} ${connector.description}`.toLowerCase()
        return terms.every((term) => facts.includes(term))
      })
    })

    watch(() => props.value, (id) => {
      if (!id) query.value = ''
      else {
        const connector = props.connectors.find((candidate) => candidate.id === id)
        if (connector) query.value = connector.vendor
      }
    }, { immediate: true })

    function choose(connector: Connector) {
      query.value = connector.vendor
      open.value = false
      emit('choose', connector.id)
    }

    function keydown(event: KeyboardEvent) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault()
        open.value = true
        const direction = event.key === 'ArrowDown' ? 1 : -1
        active.value = (active.value + direction + matching.value.length) % Math.max(matching.value.length, 1)
      } else if (event.key === 'Enter' && open.value && matching.value[active.value]) {
        event.preventDefault()
        choose(matching.value[active.value])
      } else if (event.key === 'Escape') {
        open.value = false
      }
    }

    return () => h('label', { class: 'connector-picker' }, [
      h('span', { class: 'connector-picker__label' }, props.label),
      h('input', {
        class: 'connector-picker__input',
        role: 'combobox',
        type: 'search',
        value: query.value,
        disabled: props.disabled,
        autocomplete: 'off',
        placeholder: 'Search by vendor, id, or description',
        'aria-expanded': String(open.value),
        'aria-controls': 'connector-picker-options',
        'aria-autocomplete': 'list',
        onFocus: () => { open.value = true },
        onInput: (event: Event) => {
          query.value = (event.target as HTMLInputElement).value
          active.value = 0
          open.value = true
          if (!query.value) emit('choose', '')
        },
        onKeydown: keydown,
      }),
      open.value
        ? h('ul', { id: 'connector-picker-options', class: 'connector-picker__options', role: 'listbox' },
            matching.value.length
              ? matching.value.map((connector, index) => h('li', {
                  key: connector.id,
                  class: ['connector-picker__option', index === active.value ? 'connector-picker__option--active' : ''],
                  role: 'option',
                  'aria-selected': String(connector.id === props.value),
                  onMouseenter: () => { active.value = index },
                  onMousedown: (event: Event) => event.preventDefault(),
                  onClick: () => choose(connector),
                }, [
                  h('span', { class: 'connector-picker__option-head' }, [
                    h('strong', null, connector.vendor),
                    h('code', null, connector.id),
                    connected.value.has(connector.id)
                      ? h('span', { class: 'connector-picker__connected' }, 'Connected')
                      : null,
                  ]),
                  h('span', { class: 'connector-picker__description' }, connector.description),
                ]))
              : [h('li', { class: 'connector-picker__empty' }, 'No connector matches this search.')]
          )
        : null,
    ])
  },
})
