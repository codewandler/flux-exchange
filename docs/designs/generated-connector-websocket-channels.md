# Generated connector WebSocket channels — Exchange slice

The master cross-repository design is `docs/designs/generated-connector-websocket-channels.md` in
Flux. This record fixes the Exchange ownership boundary while its release dependencies arrive.

Exchange owns channel identity, tenant derivation, persistence, supervision, inbound grants and
subscriber fan-out. The connector catalogue owns binding/event declarations and connection-plan
composition. Flux owns guarded WebSocket execution on the selected substrate. Exchange therefore
defines ports for the released catalogue/runner capabilities; it does not open a socket or compose
an HTTP/WebSocket request in `exchange-host`.

A channel is operator-owned state `{id, tenant, connector, connection, binding, selected_events}`.
Tenant, endpoint, credentials and placement never occur in mutation bodies. The runner receives the
stored identity only after the binding/event subset has passed; inbound grants are checked separately
when an agent subscribes and do not control vendor-channel lifetime. Placement is an operator-owned
resolver result, and a missing admissible placement fails before the credential port is invoked.

One supervisor exists per stored channel, independent of subscribers. It reconnects transient
vendor failures, stops on terminal configuration/auth failures, restores records at process start,
and restarts on relevant connection or credential rotation. One vendor event is fanned out live to
bounded subscriber queues; overflow disconnects only the slow subscriber. Exchange stores no event,
cursor or acknowledgement, so delivery is live at-most-once.

The authenticated `/api/subscribe` WebSocket accepts subscribe/unsubscribe commands by opaque
channel id. Responses correlate by request id. Events expose only connector, binding, declared
event, receive time and raw typed payload. Operator-only `/api/channels` routes manage stored channel
records, deriving tenant and connection authority from existing authenticated state.

The descriptor and public surfaces report the route live because the route, real WebSocket
end-to-end test and built-in single-tenant runner now ship together. A configured channel still
refuses honestly when its stores, inbound grant or admissible placement are missing. The dependency
pins moved atomically only after Flux 0.54 and connector-pack 0.17 published the required guarded
channel APIs; sibling path dependencies remain forbidden.
