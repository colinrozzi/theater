# Pact

**Pact** - Package interface specification for Theater.

## Status

Exploratory design document. Capturing ideas from initial exploration.

## Motivation

Theater has different goals and constraints than wasmtime, so it is time to re-evaluate the design decisions that we have inherited from WIT. Wasmtime made many decisions in the name of safety, with the goal of running untrusted code together on the same machine. Fundamentally, their atom is the Component. Theater's atom is the actor, which could be composed of multiple packages.

Pact is Theater's answer to WIT - a type system for describing package interfaces, designed for first-class manipulation by packages themselves.

## Core Primitives

### 1. Types

Primitive types:
```
bool, u8, u16, u32, u64, s8, s16, s32, s64, f32, f64, char, string
```

### 2. Type Constructors

Type constructors are functions from types to types:
```
list: Type -> Type
option: Type -> Type
result: (Type, Type) -> Type
```

### 3. Records

Named product types:
```
record point {
    x: f32,
    y: f32,
}
```

### 4. Variants

Tagged unions:
```
variant shape {
    circle(f32),
    rectangle(f32, f32),
    point,
}
```

### 5. Functions

First-class functions with explicit signatures:
```
func(s32, s32) -> s32
```

Functions can be:
- Passed as values
- Returned from other functions
- Stored in records

### 6. Interfaces

Interfaces are first-class values describing the full contract of a package - what it imports and what it exports.

```
interface calculator {
    imports { logger, types.big-number }
    exports {
        add: func(big-number, big-number) -> big-number;
        sub: func(big-number, big-number) -> big-number;
    }
}
```

### 7. Metadata

Interfaces can carry typed metadata using `@` annotations:

```
interface calculator {
    @version: string = "1.2.3"
    @author: string = "colin"
    @retry-count: u32 = 3
    @config: CalculatorConfig = { timeout: 30, debug: false }

    imports { ... }
    exports { ... }
}
```

- Built-in metadata (e.g., `@version`) - Pack understands these
- User-defined metadata - any `@name: Type = value`
- Metadata flows with the interface when passed around
- Packages can inspect metadata: `calculator.@version`

As first-class values, interfaces can be:
- Passed to functions
- Returned from functions
- Stored in records
- Manipulated programmatically

This moves interface operations into package space - instead of special tooling with hardcoded operations, packages can write whatever interface manipulations they need.

## Syntax

- Comments: `//`
- Terminators: semicolons (`;`)
- Blocks: braces (`{ }`)
- Type annotations: colon (`:`)
- Metadata: `@name: Type = value;`

## File Structure

Pact files live in a `pact/` directory:

```
pact/
  calculator.pact
  logger.pact
  types.pact
```

No special `package` or `world` declarations. Each file defines interfaces. Reference other files with dot notation:

```
// In calculator.pact
interface calculator {
    imports {
        logger.log,          // function from logger.pact
        types.BigNum         // type from types.pact
    }
    exports {
        add: func(types.BigNum, types.BigNum) -> types.BigNum
    }
}
```

Namespacing via nested interfaces:

```
interface my-org {
    interface calculator { ... }
    interface logger { ... }
}

// Access: my-org.calculator.add
```

Versioning via nesting or metadata:

```
interface calculator {
    @version: string = "2.0.0"
    ...
}

// Or nested versions
interface calculator {
    interface v1 { ... }
    interface v2 { ... }
}
```

## Type Operations

### On Types

Standard type constructors:
```
list<T>           // List of T
option<T>         // Optional T
result<T, E>      // Success T or error E
tuple<T, U, ...>  // Product type
```

### On Interfaces

Interfaces are data. Write functions that operate on them:

```
// Compose two interfaces
fn compose(a: Interface, b: Interface) -> Interface {
    // Merge imports, merge exports, check for conflicts
}

// Check compatibility
fn satisfies(provider: Interface, consumer: Interface) -> bool {
    // Does provider export what consumer imports?
}

// Subset exports
fn only(i: Interface, funcs: list<string>) -> Interface {
    // Return interface with only specified exports
}

// Transform all function signatures
fn wrap_results(i: Interface) -> Interface {
    // Wrap each export's return type in result<T, error>
}
```

No blessed operations - packages define whatever manipulations they need. The type system provides the primitives; you build the operations.

## What This Enables

### Programmatic Package Composition

Instead of static manifest files wiring packages together, write code that composes interfaces:

```
fn build_system() -> Interface {
    let calc = load_interface("calculator.wasm");
    let logger = load_interface("logger.wasm");

    // Check compatibility
    if !satisfies(logger, calc.imports) {
        error("logger doesn't satisfy calculator's imports");
    }

    // Compose into a system
    compose(calc, logger)
}
```

### Custom Tooling

Build whatever interface tools you need:
- Compatibility checkers
- Binding generators
- Adapter synthesizers
- Documentation extractors

These are just packages that operate on interfaces - no special tooling required.

### Runtime Capabilities

Interfaces can also be used at runtime as typed capabilities:

```
fn setup() {
    let calc: Calculator = bind(calc_actor_id);
    worker.give_calculator(calc);  // Pass capability
}
```

The same first-class interface serves both compile-time manipulation and runtime capability passing.

## Relationship to Existing Systems

### Pack Compiler

Pack currently has hardcoded interface operations. With first-class interfaces:
- Pack becomes simpler - it provides primitives, not operations
- Interface manipulation moves to packages
- Users can extend/customize without modifying Pack

### Handler Matching

Handlers can be written as packages that operate on interfaces:
- Inspect an interface's imports
- Claim interfaces they can satisfy
- No special handler registration - just interface matching

### RPC

RPC is just one pattern built on first-class interfaces:
- `bind(actor_id)` returns a capability (interface bound to an actor)
- Pass capabilities between actors
- No special RPC mechanism - packages implement whatever patterns they need

## Generics

Type parameters are declared in the interface body with `type`:

```
interface storage {
    type T: Serializable;  // Type param with constraint

    exports {
        get: func() -> T;
        set: func(T) -> ();
    }
}
```

Constraints use interface names - no separate trait system. `T: Serializable` means T must satisfy the `Serializable` interface.

Instantiation mirrors the body style:

```
storage { T = User }
```

Multiple type parameters:

```
interface pair {
    type A;
    type B;
}

pair { A = string, B = u32 }
```

## Next Steps

1. [ ] Implement Pact parser in Pack
2. [ ] Make interfaces introspectable at runtime (first-class)
3. [ ] Define built-in metadata (`@version`, others?)
