// Fixture data. Not a catalogue.
//
// **Read this before using anything below for anything.** flux-exchange has no service, no API and
// no generated catalogue yet. This file exists so the carried explorer components can be mounted,
// developed and looked at; it is the only data the console has.
//
// Two rules keep it from being mistaken for the real thing:
//
//   1. **Every name in here is invented.** No real vendor, host, service, credential or operation id
//      appears. `example.invalid` is the RFC 2606 reserved TLD — it resolves nowhere, on purpose, so
//      a base URL copied out of this console cannot accidentally reach anybody.
//   2. **It lives under `src/fixtures/`, and no component may import it.** The components take
//      everything they render as a prop; `test/components.test.mjs` holds that line mechanically, so
//      the day a real catalogue arrives it replaces this file at the app root and nowhere else.

import type { Catalog, CoreCatalog, Operation, Provider } from '../catalog.mts'

/** The empty schema stand-in — a fixture is not the place to invent a vendor's JSON Schema. */
const ANY_OBJECT = { type: 'object' } as const

const receiptOperations: Operation[] = [
  {
    id: 'list_widgets',
    provider: 'acme_fixture',
    service: 'default',
    description: 'List the widgets visible to the caller.',
    risk: 'low',
    idempotency: 'idempotent',
    method: 'GET',
    path: '/v1/widgets',
    parameters: [
      {
        name: 'limit',
        in: 'query',
        wire: 'limit',
        description: 'How many widgets to return, at most.',
        required: false,
        schema: { type: 'integer', minimum: 1, maximum: 100 },
      },
      {
        name: 'cursor',
        in: 'query',
        wire: 'cursor',
        description: 'An opaque position from a previous page.',
        required: false,
        schema: { type: 'string' },
      },
    ],
    body_schema: null,
    response_schema: {
      type: 'object',
      properties: { widgets: { type: 'array', items: ANY_OBJECT }, cursor: { type: 'string' } },
    },
    credentials: [['acme_api_token']],
    hosts: ['api.example.invalid'],
    flux: [
      'list_widgets(limit: Int?, cursor: String?) -> Any',
      '  GET https://api.example.invalid/v1/widgets',
      '    query limit = limit',
      '    query cursor = cursor',
    ].join('\n'),
    status: {
      works: false,
      issues: [
        {
          code: 'X-CATALOG-FIXTURE',
          scope: 'catalog',
          summary:
            'This console renders fixture data. No flux-exchange service exists, so no operation here can be called.',
          params: [],
        },
      ],
      notes: [],
    },
  },
  {
    id: 'create_widget',
    provider: 'acme_fixture',
    service: 'default',
    description: 'Create one widget.',
    risk: 'medium',
    idempotency: 'not_idempotent',
    method: 'POST',
    path: '/v1/widgets',
    parameters: [],
    body_schema: {
      type: 'object',
      required: ['name'],
      properties: { name: { type: 'string' }, colour: { type: 'string', enum: ['red', 'blue'] } },
    },
    response_schema: ANY_OBJECT,
    credentials: [['acme_api_token']],
    hosts: ['api.example.invalid'],
    flux: [
      'create_widget(name: String, colour: String?) -> Any',
      '  POST https://api.example.invalid/v1/widgets',
      '    body name = name',
      '    body colour = colour',
    ].join('\n'),
    status: {
      works: false,
      issues: [
        {
          code: 'X-CATALOG-FIXTURE',
          scope: 'catalog',
          summary:
            'This console renders fixture data. No flux-exchange service exists, so no operation here can be called.',
          params: [],
        },
      ],
      notes: [],
    },
  },
  {
    id: 'delete_widget',
    provider: 'acme_fixture',
    service: 'default',
    description: 'Delete one widget. The vendor keeps no tombstone.',
    risk: 'destructive',
    idempotency: 'idempotent',
    method: 'DELETE',
    path: '/v1/widgets/{widget_id}',
    parameters: [
      {
        name: 'widget_id',
        in: 'path',
        wire: 'widget_id',
        description: 'The widget to remove.',
        required: true,
        schema: { type: 'string' },
      },
    ],
    body_schema: null,
    response_schema: null,
    credentials: [['acme_api_token']],
    hosts: ['api.example.invalid'],
    flux: [
      'delete_widget(widget_id: String) -> Any',
      '  DELETE https://api.example.invalid/v1/widgets/{widget_id}',
      '    path widget_id = widget_id',
    ].join('\n'),
    status: {
      works: false,
      issues: [
        {
          code: 'X-CATALOG-FIXTURE',
          scope: 'catalog',
          summary:
            'This console renders fixture data. No flux-exchange service exists, so no operation here can be called.',
          params: [],
        },
        {
          code: 'X-NO-CONFIRMATION',
          scope: 'operation',
          summary:
            'The fixture vendor exposes no dry-run for this call, so a caller has no way to check the target before removing it.',
          params: [],
        },
      ],
      notes: [],
    },
  },
]

const ledgerOperations: Operation[] = [
  {
    id: 'get_entry',
    provider: 'ledger_fixture',
    service: 'entries',
    description: 'Read one ledger entry.',
    risk: 'low',
    idempotency: 'idempotent',
    method: 'GET',
    path: '/entries/{entry_id}',
    parameters: [
      {
        name: 'entry_id',
        in: 'path',
        wire: 'entry_id',
        description: 'The entry to read.',
        required: true,
        schema: { type: 'string' },
      },
    ],
    body_schema: null,
    response_schema: ANY_OBJECT,
    credentials: [],
    hosts: ['ledger.example.invalid'],
    flux: [
      'get_entry(entry_id: String) -> Any',
      '  GET https://ledger.example.invalid/entries/{entry_id}',
      '    path entry_id = entry_id',
    ].join('\n'),
    status: {
      works: false,
      issues: [
        {
          code: 'X-CATALOG-FIXTURE',
          scope: 'catalog',
          summary:
            'This console renders fixture data. No flux-exchange service exists, so no operation here can be called.',
          params: [],
        },
      ],
      notes: [
        {
          code: 'X-PUBLIC-READ',
          scope: 'operation',
          summary: 'The fixture vendor requires no credential for this read — there is nothing to supply.',
        },
      ],
    },
  },
  {
    id: 'append_entry',
    provider: 'ledger_fixture',
    service: 'entries',
    description: 'Append one entry to the ledger.',
    risk: 'high',
    idempotency: 'not_idempotent',
    method: 'POST',
    path: '/entries',
    parameters: [],
    body_schema: {
      type: 'object',
      required: ['amount'],
      properties: { amount: { type: 'integer' }, memo: { type: 'string' } },
    },
    response_schema: ANY_OBJECT,
    credentials: [['ledger_key']],
    hosts: ['ledger.example.invalid'],
    flux: [
      'append_entry(amount: Int, memo: String?) -> Any',
      '  POST https://ledger.example.invalid/entries',
      '    body amount = amount',
      '    body memo = memo',
    ].join('\n'),
    status: {
      works: false,
      issues: [
        {
          code: 'X-CATALOG-FIXTURE',
          scope: 'catalog',
          summary:
            'This console renders fixture data. No flux-exchange service exists, so no operation here can be called.',
          params: [],
        },
        {
          code: 'X-FIXTURE-PROVIDER',
          scope: 'provider',
          summary: 'This connector is invented for the console fixture. Nothing is behind it.',
          params: [],
        },
      ],
      notes: [],
    },
  },
  {
    id: 'ack_delivery',
    provider: 'ledger_fixture',
    service: 'events',
    description: 'Acknowledge one delivered event.',
    risk: 'low',
    idempotency: 'idempotent',
    method: 'POST',
    path: '/deliveries/{delivery_id}/ack',
    parameters: [
      {
        name: 'delivery_id',
        in: 'path',
        wire: 'delivery_id',
        description: 'The delivery being acknowledged.',
        required: true,
        schema: { type: 'string' },
      },
    ],
    body_schema: null,
    response_schema: null,
    credentials: [['ledger_key']],
    hosts: ['ledger.example.invalid'],
    flux: [
      'ack_delivery(delivery_id: String) -> Any',
      '  POST https://ledger.example.invalid/deliveries/{delivery_id}/ack',
      '    path delivery_id = delivery_id',
    ].join('\n'),
    status: {
      works: false,
      issues: [
        {
          code: 'X-CATALOG-FIXTURE',
          scope: 'catalog',
          summary:
            'This console renders fixture data. No flux-exchange service exists, so no operation here can be called.',
          params: [],
        },
        {
          code: 'X-FIXTURE-PROVIDER',
          scope: 'provider',
          summary: 'This connector is invented for the console fixture. Nothing is behind it.',
          params: [],
        },
      ],
      notes: [],
    },
  },
]

const providers: Provider[] = [
  {
    id: 'acme_fixture',
    authority: null,
    vendor: 'Acme Fixture',
    description: 'An invented connector, here so the explorer has a single-surface case to render.',
    base_url: 'https://api.example.invalid',
    api_version: '2026-01-01',
    hosts: ['api.example.invalid'],
    services: [
      {
        name: 'default',
        description: 'The only surface this fixture connector addresses.',
        base_url: 'https://api.example.invalid',
        hosts: ['api.example.invalid'],
        api_version: '2026-01-01',
        gid: null,
        operation_count: receiptOperations.length,
      },
    ],
    auth: {
      schemes: ['header'],
      credentials: [
        {
          name: 'acme_api_token',
          scheme: { kind: 'header', name: 'Authorization', prefix: 'Bearer ' },
          description: 'An invented bearer token. No value for it exists anywhere.',
          env: ['ACME_FIXTURE_API_TOKEN'],
          user_env: [],
          user_suffix: null,
          oauth2: false,
        },
      ],
      default: [['acme_api_token']],
    },
    operation_count: receiptOperations.length,
    operations: receiptOperations,
    events: [],
    channels: [],
  },
  {
    id: 'ledger_fixture',
    authority: 'ledger.example.invalid',
    vendor: 'Ledger Fixture',
    description:
      'An invented connector with two named services and an inbound surface, so the explorer has a multi-surface case.',
    base_url: 'https://ledger.example.invalid',
    api_version: null,
    hosts: ['ledger.example.invalid'],
    services: [
      {
        name: 'entries',
        description: 'Reads and appends.',
        base_url: 'https://ledger.example.invalid',
        hosts: ['ledger.example.invalid'],
        api_version: 'v2',
        gid: 'ledger.example.invalid/entries',
        operation_count: 2,
      },
      {
        name: 'events',
        description: 'Delivery acknowledgement.',
        base_url: 'https://ledger.example.invalid',
        hosts: ['ledger.example.invalid'],
        api_version: null,
        gid: 'ledger.example.invalid/events',
        operation_count: 1,
      },
    ],
    auth: {
      schemes: ['header', 'signing'],
      credentials: [
        {
          name: 'ledger_key',
          scheme: { kind: 'header', name: 'Authorization', prefix: 'Token token=' },
          description: 'An invented API key. No value for it exists anywhere.',
          env: ['LEDGER_FIXTURE_KEY'],
          user_env: [],
          user_suffix: null,
          oauth2: false,
        },
        {
          name: 'ledger_webhook_secret',
          scheme: { kind: 'signing', name: null, prefix: '' },
          description: 'The invented signing secret the fixture binding below names.',
          env: ['LEDGER_FIXTURE_WEBHOOK_SECRET'],
          user_env: [],
          user_suffix: null,
          oauth2: false,
        },
      ],
      default: [['ledger_key']],
    },
    operation_count: ledgerOperations.length,
    operations: ledgerOperations,
    events: [
      {
        name: 'entry.appended',
        service: 'events',
        oip: null,
        description: 'An entry was appended to the ledger.',
        default: true,
        group: 'entry',
        when: {},
        schema: ANY_OBJECT,
      },
      {
        name: 'entry.reversed',
        service: 'events',
        oip: null,
        description: 'A previously appended entry was reversed.',
        default: false,
        group: 'entry',
        when: {},
        schema: null,
      },
    ],
    channels: [
      {
        name: 'ledger_webhook',
        service: 'events',
        oip: null,
        description: 'The invented HTTP callback this fixture connector describes.',
        transport: 'webhook',
        events: ['entry.appended', 'entry.reversed'],
        verification: {
          kind: 'hmac',
          verified: true,
          hmac: {
            algorithm: 'sha256',
            encoding: 'hex',
            header: 'X-Ledger-Signature',
            prefix: 'sha256=',
            signed: 'timestamp.body',
            timestamp: { source: 'header', name: 'X-Ledger-Timestamp' },
            timestamp_format: 'unix_seconds',
            secret: 'ledger_webhook_secret',
            tolerance: '5m',
          },
        },
        discriminator: { source: 'body', name: 'type' },
        delivery_id: { source: 'header', name: 'X-Ledger-Delivery' },
        payload: { entry_id: 'body.data.id' },
        reply: { operation: 'ack_delivery', oip: null, result: null, bind: { delivery_id: 'delivery_id' } },
        cursor: null,
        interval: null,
        subscription: null,
        setup: {
          docs_url: null,
          steps: ['There is no dashboard. This connector is invented for the console fixture.'],
        },
      },
    ],
  },
]

const core: CoreCatalog = {
  $schema: 'https://flux-exchange.invalid/schemas/core-catalog.json',
  $id: 'flux:core',
  schema_version: 1,
  generator: 'console fixture (hand-written; no generator has run)',
  operations: [
    {
      $schema: 'https://flux-exchange.invalid/schemas/core-entry.json',
      $id: 'flux:core/operation/http_request',
      schema_version: 1,
      kind: 'operation',
      name: 'http_request',
      title: 'HTTP request',
      description: 'Send one HTTP request and return its response. Fixture entry.',
      category: ['network'],
      availability: 'available',
      tool_spec: {
        name: 'http_request',
        description: 'Send one HTTP request.',
        input_schema: {
          type: 'object',
          required: ['method', 'url'],
          properties: { method: { type: 'string' }, url: { type: 'string' } },
        },
        effects: ['network'],
        risk: 'medium',
        idempotency: 'not_idempotent',
        access: ['network'],
        group: 'http',
      },
    },
  ],
  nodes: [
    {
      $schema: 'https://flux-exchange.invalid/schemas/core-entry.json',
      $id: 'flux:core/node/map',
      schema_version: 1,
      kind: 'node',
      name: 'map',
      title: 'Map',
      description: 'Apply an expression to every element of a list. Fixture entry.',
      category: ['transform'],
      availability: 'available',
      schema_ref: 'flux:core/schema/flux_ast#/$defs/Map',
    },
  ],
  capabilities: [
    {
      $schema: 'https://flux-exchange.invalid/schemas/core-entry.json',
      $id: 'flux:core/capability/exchange',
      schema_version: 1,
      kind: 'capability',
      name: 'exchange',
      title: 'Exchange',
      description:
        'Resolve and install a connector from an exchange. Planned — nothing implements this yet, which is the whole point of the banner on this page.',
      category: ['network'],
      availability: 'planned',
      callable: false,
      operation_ids: [],
    },
  ],
  schemas: {
    catalog: ANY_OBJECT,
    entry: ANY_OBJECT,
    flux_ast: ANY_OBJECT,
  },
}

/** The whole fixture catalogue the console renders. */
export const fixtureCatalog: Catalog = {
  schema_version: 1,
  generator: 'console fixture (hand-written; no generator has run)',
  providers,
  core,
}
