---
capability: connections
---

# Connections and credentials

A **Connector** declares what one remote system can do. A **connection** is one tenant's configured
relationship with that Connector: a host-minted instance identity, an operator-chosen label, the
credential addresses it requires, and any declared non-secret settings.

The distinction matters. A Connector is public catalogue vocabulary; a connection is tenant-owned
state. One tenant may have several labelled connections to the same Connector, but a label never
becomes part of a credential address and renaming one moves no credential.

## Values go in and do not come back

The collection entry point is `GET /api/connections`. It reports instances, labels, declared
credential addresses, whether each address is held, and value-free evidence about the last
successful supplier. It never serializes a credential or setting value.

Writes are operator work because they decide which authority later invocations use. A Service
Account can call admitted operations; it cannot create a connection, rotate its credential or
change the settings that decide where the Connector's own request goes.

## Selection stays inside the tenant

Invocation selects a connection by its operator label. Omitting the label is unambiguous only while
the tenant has one connection to that Connector; a second connection makes an omitted selection a
refusal. The tenant itself never comes from a label, request body, path field or header. It comes
from the principal the host resolved.

That is why a credential is addressed rather than handed out: a caller receives the authority of
an admitted operation against one selected connection, never the value that makes the remote
request possible. [The credential-boundary argument](/boundary) explains the enforcement and its
remaining limit.
