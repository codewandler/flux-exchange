<script setup lang="ts">
// An operation's parameters, in the emitter's own order — path, query, header, body — which is also
// the argument order of the Flux declaration.
//
// The type column is a label; the vendor's JSON Schema is carried verbatim underneath it, because
// the keywords a label drops (`format`, `enum`, `oneOf`) are frequently the ones that matter. A
// `wire` name is shown only when the vendor spells the parameter differently from the caller.

import { schemaType, type Parameter } from '../catalog.mts'

defineProps<{ parameters: Parameter[] }>()
</script>

<template>
  <div
    v-if="parameters.length"
    class="params-scroll"
    tabindex="0"
    aria-label="Operation parameters"
  >
    <table class="params">
      <thead>
        <tr>
          <th>Parameter</th>
          <th>Location</th>
          <th>Type</th>
          <th>Required</th>
          <th>Description</th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="parameter in parameters" :key="parameter.name">
          <td>
            <code>{{ parameter.name }}</code>
            <div v-if="parameter.wire" class="params__wire">
              sent as <code>{{ parameter.wire }}</code>
            </div>
          </td>
          <td>{{ parameter.in }}</td>
          <td>
            <code>{{ schemaType(parameter.schema) }}</code>
            <details class="params__schema">
              <summary>JSON Schema</summary>
              <pre><code>{{ JSON.stringify(parameter.schema, null, 2) }}</code></pre>
            </details>
          </td>
          <td>{{ parameter.required ? 'required' : 'optional' }}</td>
          <td>{{ parameter.description }}</td>
        </tr>
      </tbody>
    </table>
  </div>
  <p v-else class="params__none">This operation takes no parameters.</p>
</template>

<style scoped>
.params-scroll {
  width: 100%;
  overflow-x: auto;
}

.params {
  width: 100%;
  min-width: 720px;
  margin: 0;
}

.params th,
.params td {
  vertical-align: top;
  font-size: 14px;
}

.params__wire,
.params__schema {
  font-size: 12px;
  color: var(--vp-c-text-2);
  margin-top: 4px;
}

.params__schema summary {
  cursor: pointer;
}

.params__schema pre {
  margin: 4px 0 0;
  padding: 8px 10px;
  border-radius: 6px;
  background-color: var(--vp-c-bg-soft);
  overflow-x: auto;
}

.params__schema code {
  font-size: 11px;
  white-space: pre;
}

.params__none {
  font-size: 14px;
  color: var(--vp-c-text-2);
}
</style>
