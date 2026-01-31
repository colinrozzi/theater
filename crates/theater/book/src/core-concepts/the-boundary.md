# The Deterministic Boundary

Theater's architecture rests on a single structural insight: WebAssembly gives us a genuinely deterministic box, and the interesting problems live at the boundary between that box and the non-deterministic world outside it.

Everything else — the chain, replay, handlers, verification — follows from taking that boundary seriously.

## The Non-Deterministic World

The world is non-deterministic. Network responses arrive in unpredictable orders. Clocks drift. External services change their behavior without notice. File contents shift between reads. Two identical requests made a second apart may produce entirely different results.

Any useful system must interact with this world. Theater doesn't avoid non-determinism — interfacing with the non-deterministic world is the entire point. The question is how to do it without losing the ability to reason about what happened.

## The Deterministic Box

WebAssembly provides something rare: genuine determinism. Given the same module and the same sequence of inputs, execution produces the same outputs. Always. This isn't a property Theater builds or enforces — it's something WASM gives us for free, a consequence of its design as a portable, sandboxed instruction set.

A WASM module has no ambient access to anything. No clock, no network, no filesystem, no randomness. It can only compute on what it's given. This constraint, which might seem limiting, turns out to be the foundation everything else stands on.

## The Boundary

If the inside of the box is deterministic and the outside is not, then the boundary between them is where all the interesting work happens. Theater's core job is managing this boundary.

Every interaction between an actor and the outside world crosses the boundary. A message arrives — that's an inbound crossing. The actor calls a host function to read from a store — that's an outbound crossing followed by an inbound response. Every byte that crosses gets recorded in the chain.

The chain and replay system don't sit inside the box or outside it. They sit at the edges, watching what crosses.

## What the Chain Records

There's a common misunderstanding about what the chain contains. The chain doesn't record "what the actor did." It doesn't need to. The actor's behavior is already fully specified — it IS the WASM module. Given the same inputs, it will do the same things every time.

What the chain records is the other half: what the world looked like to the actor. Every input the world provided. Every response it gave back. The non-deterministic side of each boundary crossing.

Module plus chain fully determines the computation. The module supplies the logic. The chain supplies the reality the logic operated on. Separately, neither means anything. Together, they are a complete, replayable record of what happened.

## Handlers as Boundary Adapters

Each handler in Theater is a protocol adapter between the deterministic interior and a specific slice of the non-deterministic world. The message-server handler adapts actor-to-actor messaging. The HTTP handler adapts web requests. The filesystem handler adapts file operations. The store handler adapts persistent state. The supervisor handler adapts lifecycle management.

They look different from the outside — different protocols, different data shapes, different semantics. But from the boundary's perspective, they are structurally identical: opaque data crosses in, opaque data crosses out, and the crossing gets recorded.

This uniformity is what makes the handler system Theater's core abstraction. Adding a new handler — TCP, GPU, database, anything — requires no changes to the recording or replay infrastructure. The new handler is just another shaped hole in the box. The boundary doesn't care what the shape is. It only cares that crossings are recorded.

## The Chain as Source of Truth

This framing inverts a familiar assumption. Normally, code is the source of truth. You trust the code, and its outputs are derived consequences.

In Theater, the chain is the source of truth. The module is one implementation that produces a given chain. You could rewrite the module entirely — different language, different algorithm, different internal architecture — and as long as it produces the same chain given the same boundary inputs, it is equivalent. Correctness is chain-compatibility.

The module is not what you trust. The chain is what you trust. The module is what you validate against the chain.

## Chains as Specifications

This inversion has a practical consequence: past chains become specifications for future development.

A corpus of chains from a running system is a regression suite — not test cases someone wrote, but recorded interactions with the real world. Each chain captures not just inputs but the full dialogue between actor and environment: what was asked, what was provided, how the actor responded, and what happened next.

A new module version is correct if it reproduces existing chains. Replay becomes your type checker. You don't write test cases that approximate reality — you program against recorded reality itself.

## Why Verification, Not Re-Execution

There is a temptation to treat replay as a way to re-execute computations — to use recorded chains as a substitute for running the system again. This misses the point and collapses the value of the approach.

Ashby's Law of Requisite Variety applies here: to control or reproduce an external system's behavior, your internal model needs at least as much variety as that system exhibits. A replay trace has exactly one path of variety — the recorded one. This is sufficient for verification but insufficient for re-execution, because re-execution implies the ability to handle new situations, which a fixed trace cannot provide.

The power of replay is precisely that it is less than full execution. It trades the ability to affect the world for the ability to prove what happened. That trade is the point. Verification is a weaker claim than execution, and weaker claims are easier to make with certainty.

A replay that succeeds tells you: this module, given this reality, produces this behavior. It tells you nothing about other realities. But what it does tell you, it tells you with certainty. And a large corpus of chains covers a large surface of real behavior — not hypothetical behavior someone imagined, but behavior that actually occurred.
