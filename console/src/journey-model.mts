// Pure operator-journey questions shared by Connections, Grants and Invoke.

import type { Catalog } from './catalog.mts'
import type { Selector } from './service.mts'

export type JourneyStepId = 'connect' | 'grant' | 'invoke'
export type JourneyState = 'complete' | 'current' | 'ready' | 'locked'

export interface JourneyStep {
  id: JourneyStepId
  label: string
  path: string
  state: JourneyState
}

interface JourneySource {
  connections: readonly { connector: string; credentials: readonly { held: boolean }[] }[]
  grants: readonly { connector: string; admits: readonly { id: string }[] }[]
  active: JourneyStepId
}

/** The three-step state, derived only from the latest server answers. */
export function setupJourney(source: JourneySource): JourneyStep[] {
  const connected = source.connections.some((connection) =>
    connection.credentials.length === 0 || connection.credentials.some((credential) => credential.held)
  )
  const granted = source.grants.some((grant) => grant.admits.length > 0)
  const state = (id: JourneyStepId): JourneyState => {
    if (id === source.active) return 'current'
    if (id === 'connect') return connected ? 'complete' : 'ready'
    if (id === 'grant') return granted ? 'complete' : connected ? 'ready' : 'locked'
    return connected && granted ? 'ready' : 'locked'
  }
  return [
    { id: 'connect', label: 'Connect', path: '/connections', state: state('connect') },
    { id: 'grant', label: 'Grant', path: '/grants', state: state('grant') },
    { id: 'invoke', label: 'Invoke', path: '/invoke', state: state('invoke') },
  ]
}

export type GrantPreset = 'read-only' | 'no-destructive' | 'custom'

/** Conservative presets expressed only in the selector vocabulary the service accepts. */
export function grantPreset(preset: GrantPreset, effects: readonly string[]): Selector {
  if (preset === 'read-only') {
    return { maxRisk: 'low', effectsWithin: null, idempotency: null }
  }
  if (preset === 'no-destructive') {
    return {
      maxRisk: null,
      effectsWithin: effects.filter((effect) => effect !== 'delete' && effect !== 'money'),
      idempotency: null,
    }
  }
  return { maxRisk: null, effectsWithin: null, idempotency: null }
}

export type PreviewChange = 'narrower' | 'unchanged' | 'wider'

/** Compare consequences, not selector syntax: admitted operation ids are the authority. */
export function previewChange(held: readonly string[], proposed: readonly string[]): PreviewChange {
  const before = new Set(held)
  const after = new Set(proposed)
  if (before.size === after.size && [...before].every((id) => after.has(id))) return 'unchanged'
  if ([...after].every((id) => before.has(id))) return 'narrower'
  return 'wider'
}

export interface RiskGroup {
  risk: string
  operations: string[]
}

export interface ServiceGroup {
  connector: string
  service: string
  risks: RiskGroup[]
}

/** Group the server-admitted ids using only catalogue display metadata. */
export function groupAdmitted(catalog: Catalog, admitted: readonly string[]): ServiceGroup[] {
  const wanted = new Set(admitted)
  const groups: ServiceGroup[] = []
  for (const connector of catalog.connectors) {
    for (const operation of connector.operations) {
      if (!wanted.has(operation.id)) continue
      let service = groups.find(
        (group) => group.connector === connector.id && group.service === operation.service
      )
      if (!service) {
        service = { connector: connector.id, service: operation.service, risks: [] }
        groups.push(service)
      }
      let risk = service.risks.find((group) => group.risk === operation.risk)
      if (!risk) {
        risk = { risk: operation.risk, operations: [] }
        service.risks.push(risk)
      }
      risk.operations.push(operation.id)
    }
  }
  return groups
}
