// Small JSON-schema helpers for the exact top-level body the invoke route accepts.

export type JsonSchema = Record<string, unknown>

function object(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function emptyFor(schema: JsonSchema): unknown {
  switch (schema.type) {
    case 'string': return ''
    case 'integer':
    case 'number': return 0
    case 'boolean': return false
    case 'array': return []
    case 'object': return {}
    default: return null
  }
}

/** A valid JSON starting point containing the schema's required top-level properties. */
export function bodyFromSchema(schema: JsonSchema): Record<string, unknown> {
  const properties = object(schema.properties) ? schema.properties : {}
  const required = Array.isArray(schema.required)
    ? schema.required.filter((name): name is string => typeof name === 'string')
    : []
  return Object.fromEntries(
    required.map((name) => [name, object(properties[name]) ? emptyFor(properties[name]) : null])
  )
}

function matches(type: unknown, value: unknown): boolean {
  switch (type) {
    case 'string': return typeof value === 'string'
    case 'integer': return typeof value === 'number' && Number.isInteger(value)
    case 'number': return typeof value === 'number' && Number.isFinite(value)
    case 'boolean': return typeof value === 'boolean'
    case 'array': return Array.isArray(value)
    case 'object': return object(value)
    default: return true
  }
}

/** Top-level required/type validation; the runtime remains authoritative for the full schema. */
export function validateBody(schema: JsonSchema, body: unknown): string[] {
  if (!object(body)) return ['the invocation body must be a JSON object']
  const properties = object(schema.properties) ? schema.properties : {}
  const problems: string[] = []
  for (const [name, value] of Object.entries(body)) {
    const declared = properties[name]
    if (object(declared) && !matches(declared.type, value)) {
      problems.push(`\`${name}\` must be ${declared.type === 'integer' ? 'an integer' : `a ${String(declared.type)}`}`)
    }
  }
  const required = Array.isArray(schema.required) ? schema.required : []
  for (const name of required) {
    if (typeof name === 'string' && !(name in body)) problems.push(`\`${name}\` is required`)
  }
  return problems
}

