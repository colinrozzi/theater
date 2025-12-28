# WIT Debugging Guide

This guide covers common issues when implementing WASI/WIT interfaces in Theater handlers and how to debug them.

## Common Errors and Solutions

### 1. "Component imports instance X, but a matching implementation was not found in the linker"

This is the most common error when implementing WIT interfaces. It means the component expects an interface that isn't properly registered with the linker.

**Possible causes:**

1. **Resources in wrong interface**: WASI interfaces often define resources in a `types` interface but use them in other interfaces. All resources must be registered in their defining interface.

   ```rust
   // WRONG: Defining fields resource in outgoing-handler
   let mut interface = linker.instance("wasi:http/outgoing-handler@0.2.0")?;
   interface.resource("fields", ...)?;

   // CORRECT: Define in types interface
   let mut interface = linker.instance("wasi:http/types@0.2.0")?;
   interface.resource("fields", ...)?;
   ```

2. **Type signature mismatch**: Even if the interface is registered, if function signatures don't match exactly, the linker won't find a match.

   ```rust
   // WRONG: Returns u32 for error
   |ctx, (res, name, value): (Resource<Fields>, String, Vec<u8>)|
       -> Result<(Result<(), u32>,)>

   // CORRECT: Returns the proper WIT variant type
   |ctx, (res, name, value): (Resource<Fields>, String, Vec<u8>)|
       -> Result<(Result<(), WasiHeaderError>,)>
   ```

3. **Missing version in interface name**: WASI interfaces require version numbers.

   ```rust
   // WRONG
   linker.instance("wasi:http/types")?;

   // CORRECT
   linker.instance("wasi:http/types@0.2.0")?;
   ```

**Debugging steps:**

1. Use `wasm-tools component wit` to inspect what the component actually imports:
   ```bash
   wasm-tools component wit path/to/component.wasm
   ```

2. Add debug logging to see what interfaces are being set up:
   ```rust
   debug!("Setting up interface: {}", interface_name);
   ```

3. Compare the WIT definition with your implementation function by function.

### 2. Type Signature Mismatches

WIT uses specific type representations that must be matched exactly in Rust.

#### Variant Types

WIT variants map to Rust enums with specific derives:

```wit
variant method {
    get,
    head,
    post,
    put,
    delete,
    connect,
    options,
    trace,
    patch,
    other(string),
}
```

```rust
#[derive(Debug, Clone, ComponentType, Lift, Lower)]
#[component(variant)]
pub enum WasiMethod {
    #[component(name = "get")]
    Get,
    #[component(name = "head")]
    Head,
    #[component(name = "post")]
    Post,
    // ... etc
    #[component(name = "other")]
    Other(String),
}
```

Key points:
- Use `#[component(variant)]` attribute
- Use `#[component(name = "...")]` for each variant matching the WIT name exactly
- Derive `ComponentType`, `Lift`, and `Lower` from `wasmtime::component`

#### Result Types

WIT `result<T, E>` maps to `Result<T, E>` but must be wrapped in a tuple for function returns:

```rust
// WIT: func() -> result<_, header-error>
// Rust return type:
-> Result<(Result<(), WasiHeaderError>,)>
```

The outer `Result` is for host errors, the inner tuple contains the WIT return value.

#### Option Types

WIT `option<T>` maps directly to `Option<T>`:

```rust
// WIT: func() -> option<string>
// Rust:
-> Result<(Option<String>,)>
```

#### Nested Types

Some WASI functions have deeply nested return types:

```wit
// future-incoming-response.get returns:
// option<result<result<incoming-response, error-code>, ()>>
```

```rust
-> Result<(Option<Result<Result<Resource<IncomingResponse>, WasiErrorCode>, ()>>,)>
```

### 3. "Cannot start a runtime from within a runtime"

This panic occurs when using `tokio::runtime::Runtime::block_on()` inside an existing tokio async context.

**Solution:** Use `block_in_place` with the current handle:

```rust
// WRONG
let rt = tokio::runtime::Runtime::new()?;
let result = rt.block_on(async_operation());

// CORRECT
let result = tokio::task::block_in_place(|| {
    tokio::runtime::Handle::current().block_on(async_operation())
});
```

### 4. Resource Lifecycle Issues

WASI resources have specific ownership semantics. Common issues:

1. **Resource not found**: The resource was dropped or never created
2. **Resource already consumed**: Some resources can only be used once (e.g., `incoming-body`)

**Debugging:**

```rust
// Add logging when resources are created/accessed
fn create_resource<T>(table: &mut ResourceTable, value: T) -> Resource<T> {
    let res = table.push(value)?;
    debug!("Created resource {:?}", res.rep());
    res
}
```

### 5. Handler Not Being Selected

Theater selects handlers based on component imports/exports matching. If your handler isn't being activated:

1. **Check what the component imports:**
   ```bash
   wasm-tools component wit path/to/component.wasm | grep "import"
   ```

2. **Verify handler's `imports()` method returns matching interfaces:**
   ```rust
   fn imports(&self) -> Option<String> {
       Some("wasi:http/types@0.2.0,wasi:http/outgoing-handler@0.2.0".to_string())
   }
   ```

3. **Run with debug logging:**
   ```bash
   theater process manifest.toml --log-level debug --log-stdout
   ```

## Debugging Tools

### wasm-tools

Inspect component WIT:
```bash
wasm-tools component wit component.wasm
```

Validate component:
```bash
wasm-tools validate component.wasm
```

### Theater Debug Logging

Enable debug logging:
```bash
theater process manifest.toml --log-level debug --log-stdout
```

Or set environment variable:
```bash
RUST_LOG=debug theater process manifest.toml
```

### Adding Temporary Debug Output

When stuck, add temporary `eprintln!` statements to trace execution:

```rust
eprintln!("[DEBUG] Setting up interface: {}", name);
eprintln!("[DEBUG] Registering function: {}", func_name);
```

Remember to remove these before committing - use proper `debug!()` or `tracing::debug!()` macros for permanent logging.

## Checklist for New WASI Handler Implementation

1. [ ] Identify all interfaces the component imports (use `wasm-tools component wit`)
2. [ ] Create variant types with proper `ComponentType`, `Lift`, `Lower` derives
3. [ ] Register resources in the correct interface (usually `*/types`)
4. [ ] Match function signatures exactly (check return type wrapping)
5. [ ] Handle async operations with `block_in_place` if needed
6. [ ] Set handler's `imports()` to return the interface names
7. [ ] Test with a minimal component first
8. [ ] Add proper event logging for chain traceability

## Example: Debugging a Linker Error

Given this error:
```
component imports instance `wasi:http/types@0.2.0`, but a matching
implementation was not found in the linker
```

Steps:
1. Confirm you're calling `linker.instance("wasi:http/types@0.2.0")`
2. Check all functions in that interface have correct signatures
3. Verify resources are defined in this interface, not another
4. Look for type mismatches in return types (especially error variants)
5. Enable debug logging and trace which functions are being registered
6. Compare against the official WIT definition

## References

- [WASI HTTP Spec](https://github.com/WebAssembly/wasi-http)
- [Wasmtime Component Model](https://docs.wasmtime.dev/api/wasmtime/component/index.html)
- [WIT Format](https://component-model.bytecodealliance.org/design/wit.html)
