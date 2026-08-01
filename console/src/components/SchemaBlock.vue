<script setup lang="ts">
// A JSON Schema, rendered so a human can read it.
//
// Unlike `FluxSource.vue`, colouring here is legitimate: shiki has no Flux grammar, so Flux stays
// plain text rather than being coloured by another language's rules (C-43) — but JSON and YAML are
// real grammars we are not guessing at.
//
// The highlighter is hand-rolled and about forty lines. That is deliberate: shiki is a build-time
// dependency in VitePress, and this content is read out of the catalogue at *runtime*, so the built
// pipeline does not apply. Pulling a client-side highlighter in for one block would cost more bytes
// than the catalogue itself.
//
// Tokens are rendered as elements, never as `v-html`. Schema text is generated data, but a
// highlighter that interpolates raw markup is one upstream change away from injecting it.

import { computed, ref } from 'vue'

const props = defineProps<{ schema: unknown }>()

type Token = { t: string; c: string }

const format = ref<'json' | 'yaml'>('json')
const copied = ref(false)

/** YAML for a JSON value. Strings are quoted whenever bare output could be re-read as something else. */
function toYaml(value: unknown, indent = 0): string {
  const pad = '  '.repeat(indent)

  if (value === null) return 'null'
  if (typeof value === 'boolean' || typeof value === 'number') return String(value)
  if (typeof value === 'string') return scalar(value)

  if (Array.isArray(value)) {
    if (!value.length) return '[]'
    return value
      .map((item) => `${pad}- ${nested(item, indent + 1).replace(/^\s+/, '')}`)
      .join('\n')
  }

  const entries = Object.entries(value as Record<string, unknown>)
  if (!entries.length) return '{}'
  return entries
    .map(([key, item]) => {
      const rendered = nested(item, indent + 1)
      const inline = !rendered.includes('\n') && !isBranch(item)
      return inline ? `${pad}${scalar(key)}: ${rendered}` : `${pad}${scalar(key)}:\n${rendered}`
    })
    .join('\n')
}

function nested(value: unknown, indent: number): string {
  return isBranch(value) ? toYaml(value, indent) : toYaml(value, 0)
}

function isBranch(value: unknown): boolean {
  if (Array.isArray(value)) return value.length > 0
  return typeof value === 'object' && value !== null && Object.keys(value).length > 0
}

/** A bare scalar where that reads back unambiguously, a quoted one everywhere else. */
function scalar(text: string): string {
  const bare = /^[A-Za-z_][A-Za-z0-9_.\-/ ]*$/.test(text)
  const reserved = /^(y|n|yes|no|true|false|null|on|off)$/i.test(text)
  return bare && !reserved && text.trim() === text ? text : JSON.stringify(text)
}

const text = computed(() =>
  format.value === 'json' ? JSON.stringify(props.schema, null, 2) : toYaml(props.schema)
)

const JSON_TOKENS = /("(?:\\.|[^"\\])*")(\s*:)?|\b(true|false|null)\b|(-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?)/g
const YAML_TOKENS = /^(\s*)([A-Za-z_"][^:\n]*?)(:)|("(?:\\.|[^"\\])*")|\b(true|false|null)\b|(-?\d+(?:\.\d+)?)/gm

/** One line per row, each a list of classified tokens. */
const lines = computed<Token[][]>(() =>
  text.value.split('\n').map((line) => tokenize(line, format.value))
)

function tokenize(line: string, kind: 'json' | 'yaml'): Token[] {
  const pattern = kind === 'json' ? JSON_TOKENS : YAML_TOKENS
  pattern.lastIndex = 0
  const out: Token[] = []
  let at = 0
  let match: RegExpExecArray | null

  while ((match = pattern.exec(line)) !== null) {
    if (match.index > at) out.push({ t: line.slice(at, match.index), c: 'plain' })

    if (kind === 'json') {
      const [, str, colon, literal, num] = match
      if (str !== undefined) {
        out.push({ t: str, c: colon ? 'key' : 'string' })
        if (colon) out.push({ t: colon, c: 'punct' })
      } else if (literal !== undefined) out.push({ t: literal, c: 'literal' })
      else if (num !== undefined) out.push({ t: num, c: 'number' })
    } else {
      const [, lead, key, colon, str, literal, num] = match
      if (key !== undefined) {
        if (lead) out.push({ t: lead, c: 'plain' })
        out.push({ t: key, c: 'key' })
        out.push({ t: colon, c: 'punct' })
      } else if (str !== undefined) out.push({ t: str, c: 'string' })
      else if (literal !== undefined) out.push({ t: literal, c: 'literal' })
      else if (num !== undefined) out.push({ t: num, c: 'number' })
    }
    at = match.index + match[0].length
  }

  if (at < line.length) out.push({ t: line.slice(at), c: 'plain' })
  return out
}

async function copy() {
  try {
    await navigator.clipboard.writeText(text.value)
    copied.value = true
    setTimeout(() => (copied.value = false), 1600)
  } catch {
    // A clipboard a browser refuses is not an error worth showing; the text is selectable anyway.
  }
}
</script>

<template>
  <div class="schema">
    <div class="schema__bar">
      <div class="schema__formats" role="group" aria-label="Schema format">
        <button
          v-for="option in (['json', 'yaml'] as const)"
          :key="option"
          type="button"
          class="schema__format"
          :class="{ 'schema__format--on': format === option }"
          :aria-pressed="format === option"
          @click="format = option"
        >
          {{ option.toUpperCase() }}
        </button>
      </div>
      <button type="button" class="schema__copy" @click="copy">
        {{ copied ? 'Copied' : 'Copy' }}
      </button>
    </div>
    <pre class="schema__code"><code><span
      v-for="(line, index) in lines"
      :key="index"
      class="schema__line"
    ><span
        v-for="(token, at) in line"
        :key="at"
        :class="`tok tok--${token.c}`"
      >{{ token.t }}</span>{{ '\n' }}</span></code></pre>
  </div>
</template>

<style scoped>
.schema {
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  overflow: hidden;
  margin: 0 0 24px;
}

.schema__bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding: 6px 8px 6px 10px;
  background-color: var(--vp-c-bg-soft);
  border-bottom: 1px solid var(--vp-c-divider);
}

.schema__formats {
  display: flex;
  gap: 2px;
}

.schema__format,
.schema__copy {
  border: 0;
  border-radius: 6px;
  padding: 3px 10px;
  font-size: 12px;
  font-weight: 600;
  line-height: 20px;
  color: var(--vp-c-text-2);
  background-color: transparent;
  cursor: pointer;
}

.schema__format:hover,
.schema__copy:hover {
  color: var(--vp-c-text-1);
  background-color: var(--vp-c-default-soft);
}

.schema__format--on,
.schema__format--on:hover {
  color: var(--vp-c-brand-1);
  background-color: var(--vp-c-brand-soft);
}

.schema__code {
  margin: 0;
  padding: 14px 18px;
  background-color: var(--vp-code-block-bg);
  overflow-x: auto;
}

.schema__code code {
  font-family: var(--vp-font-family-mono);
  font-size: var(--vp-code-font-size);
  line-height: var(--vp-code-line-height);
  white-space: pre;
}

/* Theme-aware by construction: every hue is a VitePress token, so light and dark both work and a
   future theme change carries automatically. */
.tok--plain { color: var(--vp-c-text-1); }
.tok--punct { color: var(--vp-c-text-3); }
.tok--key { color: var(--vp-c-brand-1); font-weight: 500; }
.tok--string { color: var(--vp-c-green-1); }
.tok--number { color: var(--vp-c-purple-1); }
.tok--literal { color: var(--vp-c-yellow-1); }
</style>
