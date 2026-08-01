<script setup lang="ts">
// The generated Flux, verbatim.
//
// Not a fenced code block: shiki has no Flux grammar, and `markdown.languageAlias` turns that
// warning into a hard build failure (C-43). Colouring Flux by another language's rules would be
// worse than plain text, so this is plain text — the bytes the emitter produced, unedited.

defineProps<{ source: string }>()
</script>

<template>
  <pre class="flux"><code>{{ source }}</code></pre>
</template>

<style scoped>
.flux {
  background-color: var(--vp-code-block-bg);
  border-radius: 8px;
  padding: 16px 20px;
  overflow-x: auto;
  margin: 0;
}

.flux code {
  font-family: var(--vp-font-family-mono);
  font-size: var(--vp-code-font-size);
  line-height: var(--vp-code-line-height);
  color: var(--vp-c-text-1);
  white-space: pre;
}
</style>
