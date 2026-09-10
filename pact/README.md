# `pact/` — interface reference

This directory holds the pact interfaces that have **no handler crate to own
them** — the client-callback interfaces that *actors export* and the shared
type definitions:

- `tcp-client.pact` — callbacks the TCP handler invokes on an actor
- `message-server-client.pact` — callbacks the message-server handler invokes on an actor
- `loop-client.pact` — callbacks the loop handler invokes on an actor
- `types.pact` — shared record/type definitions (`channel-accept`, chain events, …)

## Host interfaces live in their handler crate — not here

The interface an actor **imports** from a handler (e.g. `theater:simple/tcp`,
`theater:simple/self`, `theater:simple/store`) is defined **canonically in that
handler's crate** and `include_str!`'d into the handler at build time, e.g.
`crates/theater-handler-tcp/tcp.pact`. That crate-local copy is the **single
source of truth** — it is what the handler registers and what the runtime hashes
at spawn (`build_actor_resources` computes the interface hash from the handler's
registered interfaces). This follows the registry-in-handler-crates model
(#185/#188): a handler owns its interface.

**Do not keep host-interface copies here.** A duplicate mirror silently drifts
from the crate copy, and because the runtime only ever hashes the crate copy, a
stale mirror gives an actor author a wrong signature that then fails
`MissingInterfaceMetadata` at spawn (this bit the client-interface pilot: a stale
`pact/tcp.pact` was missing `transfer-async`, and `pact/runtime.pact` predated
the `runtime`→`self` rename). To read the authoritative signature of a host
interface, read its handler crate's `.pact`.
