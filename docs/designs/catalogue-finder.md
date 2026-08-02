# Design: an exchange-owned catalogue finder

**Status:** accepted · **Story:** X-86

## Decision

flux-exchange owns its catalogue UI. The copied flux-connectors documentation explorer is retired
rather than synchronized or packaged: its source document is richer than this host's anonymous API,
so it renders blank method, path, host, credential and Flux-source fields here and makes every local
improvement a cross-repository maintenance problem.

The replacement is one search field over three real result kinds:

```text
[ Search connectors, services, and operations... ]

[ Connectors 54 ] [ Services N ] [ Operations 679 ]

Showing ...
[ results ]
```

Connectors are the default and an empty query browses the active kind. Services are derived exactly
from the service on every served operation. Channels do not appear until the exchange catalogue
actually publishes channel metadata; a disabled or permanently empty tab would imply a capability
the service does not have.

## Data and search

The connector listing gains `vendor` and `description`, projected from the same compiled catalogue
as `id` and `operation_count`. It remains anonymous and contains no principal, tenant, holding,
grant, credential value or deployment state.

The console keeps an exchange-native model containing only served facts. Search is case-insensitive,
splits on whitespace and requires every term to occur in a visible field:

- connector: id, vendor, description;
- service: name, connector id and vendor;
- operation: id, description, connector id and vendor, service, risk, idempotency and effects.

Exact primary names rank first, then prefixes, primary-name substrings and metadata matches. Source
order breaks ties and remains the browse order for an empty query. A connector or service result can
replace the view with the Operations tab and the terms that identify its scope.

## Address and interaction

Finder state belongs to the fragment route, for example
`#/explorer?kind=operations&q=google%20gmail`. The default connector kind and an empty query are
omitted. Changes replace the current history entry so typing does not turn Back into an undo stack;
following an operation still pushes a real navigation entry, so Back restores the finder.

Legacy top-level `q` state is carried forward and a provider anchor becomes a connector search.
Unknown kinds and obsolete facets widen to the default rather than matching nothing. Tabs expose
selection and controls relationships, support arrow/Home/End navigation, retain the query between
kinds, and report match counts through a polite live region.

## Ownership and presentation

The current shell and CSS token vocabulary remain the visual contract, including light and dark
themes. Their VitePress-shaped names are harmless implementation history; no palette is copied or
synchronized after this decision. Catalogue views continue to receive data from `App.vue` rather
than fetching for themselves, so request failures remain distinct from empty data.

Documentation-only views for Flux core, inbound bindings, generated source, request paths, hosts and
credentials are removed because this API publishes none of those facts. Operation detail retains
description, connector, service, risk, idempotency, effects with provenance, and three-valued
admission—the facts this host does publish.

## Proof

Rust tests compare the complete connector listing with `connector_catalog::providers()` and assert
the anonymous wire shape. Pure console tests cover derivation, search, ranking and URL state. Mounted
view tests cover tabs, keyboard behavior, narrowing actions and empty states. The production build
and a phone/desktop light/dark inspection cover the CSS behavior a unit test cannot.

