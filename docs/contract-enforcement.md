# Contract Enforcement

Theater enforces type contracts between the runtime and actors. When an actor declares typed function signatures, the runtime guarantees those types are respected — both on input and output.

## How It Works

Actors declare their types in `pack_types!`:

```rust
pack_types! {
    record actor-state {
        count: s32,
        name: string,
    }

    exports {
        init: func(state: value) -> result<actor-state, string>,
        increment: func(state: actor-state) -> result<tuple<actor-state, s32>, string>,
    }
}
```

This embeds full type metadata in the WASM module. At actor startup, the runtime reads this metadata and caches it. On every function call:

1. **Input validation**: The state Value is checked against the function's declared first parameter type *before* crossing the WASM boundary.
2. **Output validation**: The return Value is checked against the declared result type *after* the function returns.

If either check fails, the call is rejected with a clear error — the actor never sees invalid data, and invalid results never propagate.

## Type Space Membership

Validation works by checking whether a runtime `Value` is a valid inhabitant of a declared type space. The type space is defined by the function's signature and its type definitions.

This is compositional:

- **Primitives**: direct match (bool, u8, string, etc.)
- **Records**: all declared fields must be present with correct types, no extra fields
- **Variants**: the active case must exist in the type definition, and its payload must match
- **Enums**: the case name must be in the declared set
- **Tuples, Lists, Options, Results**: recurse into children
- **`value`**: the escape hatch — any Value is valid (actor opts out of static typing for that position)

Errors include full context paths: `"in field 'pos' of record 'actor-state': expected f64, got string"`.

## What Gets Validated

| Check | When | What |
|-------|------|------|
| Input state | Before WASM call | State matches first parameter type |
| Output result | After WASM call | Return value matches declared result type |
| Interface hashes | Actor startup | Actor's imports match handler-provided interfaces |
| Export existence | Handler setup | Handler checks actor exports before registering callbacks |

## The `value` Escape Hatch

`Type::Value` matches any runtime Value. Use it when:

- A function accepts arbitrary input (like `init` which takes whatever the runtime passes)
- You're prototyping and don't want to commit to a type yet
- The function genuinely operates on dynamic data

```rust
// init takes anything, returns typed state
init: func(state: value) -> result<actor-state, string>,

// subsequent calls require typed state
handle: func(state: actor-state, msg: string) -> result<tuple<actor-state>, string>,
```

## Custom Types in `pack_types!`

The `pack_types!` macro supports all pack type definitions:

```rust
pack_types! {
    // Records — product types with named fields
    record position { x: f64, y: f64 }

    // Variants — sum types with optional payloads
    variant command {
        move-to(position),
        stop,
        set-speed(f64),
    }

    // Enums — variants without payloads
    enum color { red, green, blue }

    // Flags — bit sets
    flags permissions { read, write, execute }

    // Type aliases
    type velocity = f64

    // Types compose freely
    record actor-state {
        pos: position,
        cmd: command,
        speed: velocity,
    }

    imports { ... }
    exports { ... }
}
```

## Design Principles

**Fail fast**: Type mismatches are caught at the runtime boundary, not inside the WASM module. No wasted encoding, no crossing the WASM boundary, no opaque guest-side errors.

**Belt and suspenders**: The guest-side macro also validates types when converting from `Value` to Rust types. Both sides enforce the contract independently.

**Runtime stays simple**: The runtime holds a `Value` and passes it through. It doesn't interpret state — it just validates that the shape matches what the function expects. Pack handles the actual type system.

**Metadata is mandatory**: All actors must export `__pack_types` with their type metadata. Actors without metadata fail to start.
