# Environment Handler

The Environment Handler provides **read-only access** to host environment variables for WebAssembly actors in the Theater system. This handler enables agents to read configuration and runtime information from the environment while maintaining strict security controls and complete audit trails.

## Features

- **Read-only Access**: Agents can only read existing environment variables, not modify them
- **Fine-grained Permissions**: Control which variables can be accessed through allowlists, denylists, and prefix filters
- **Complete Auditability**: Every environment variable access is logged in the actor's event chain
- **Security by Default**: No variables are accessible unless explicitly permitted

## Configuration

Add the environment handler to your actor's `manifest.toml`:

```toml
[[handlers]]
type = "environment"
config = { 
    # Configuration options here
}
```

### Configuration Options

#### Allowlist Approach (Recommended)
```toml
[[handlers]]
type = "environment"
config = { 
    allowed_vars = ["NODE_ENV", "PORT", "DATABASE_URL"],
    allow_list_all = false 
}
```

#### Prefix-based Access
```toml
[[handlers]]
type = "environment"
config = { 
    allowed_prefixes = ["APP_", "PUBLIC_", "MY_SERVICE_"],
    denied_vars = ["APP_SECRET", "APP_PASSWORD"],
    allow_list_all = false 
}
```

#### Denylist Approach (Less Secure)
```toml
[[handlers]]
type = "environment"
config = { 
    denied_vars = ["PATH", "HOME", "USER", "SSH_KEY", "AWS_SECRET"],
    allow_list_all = true 
}
```

### Configuration Parameters

- **`allowed_vars`** (Optional): List of specific environment variable names that can be accessed
- **`denied_vars`** (Optional): List of environment variable names that cannot be accessed  
- **`allowed_prefixes`** (Optional): List of prefixes - only variables starting with these prefixes can be accessed
- **`allow_list_all`** (Default: `false`): Whether to allow the `list-vars` function to return all accessible variables

### Permission Logic

The handler uses the following permission logic (in order):

1. **Deny List Check**: If the variable is in `denied_vars`, access is denied
2. **Allow List Check**: If `allowed_vars` is specified, only those variables are allowed
3. **Prefix Check**: If `allowed_prefixes` is specified, only variables with matching prefixes are allowed
4. **Default**: If no restrictions are configured, all variables except those in `denied_vars` are allowed

## Available Functions

The environment handler provides three WebAssembly host functions:

### `get-var(name: string) -> option<string>`

Retrieves the value of an environment variable.

- **Parameters**: `name` - The environment variable name
- **Returns**: `Some(value)` if the variable exists and access is allowed, `None` otherwise
- **Security**: Returns `None` for both non-existent variables and access-denied variables

### `exists(name: string) -> bool`

Checks if an environment variable exists and is accessible.

- **Parameters**: `name` - The environment variable name  
- **Returns**: `true` if the variable exists and access is allowed, `false` otherwise
- **Security**: Returns `false` for access-denied variables

### `list-vars() -> list<tuple<string, string>>`

Lists all accessible environment variables.

- **Parameters**: None
- **Returns**: List of (name, value) tuples for all accessible variables
- **Security**: Only works if `allow_list_all` is `true`, returns empty list otherwise

## Security Considerations

### Best Practices

1. **Use Allowlists**: Prefer `allowed_vars` or `allowed_prefixes` over `denied_vars`
2. **Disable List All**: Keep `allow_list_all = false` unless specifically needed
3. **Principle of Least Privilege**: Only grant access to variables the agent actually needs
4. **Sensitive Data**: Never expose secrets, keys, or passwords through environment variables accessible to agents

### Example Secure Configuration

```toml
[[handlers]]
type = "environment"
config = { 
    # Only allow application-specific configuration
    allowed_prefixes = ["MYAPP_"],
    # Explicitly deny any sensitive variables that might match the prefix
    denied_vars = ["MYAPP_SECRET_KEY", "MYAPP_DB_PASSWORD"],
    # Don't allow listing all variables
    allow_list_all = false 
}
```

## Event Logging

All environment variable access attempts are logged with the following information:

- **Operation**: `get-var`, `exists`, or `list-vars`
- **Variable Name**: The name of the variable accessed
- **Success**: Whether the operation was allowed by permissions
- **Value Found**: Whether the variable actually existed (for successful operations)
- **Timestamp**: When the access occurred

## Example Usage in Rust Actor

```rust
// In your WebAssembly actor
use theater_bindings::environment::{get_var, exists, list_vars};

// Get a specific configuration value
if let Some(port) = get_var("PORT") {
    println!("Server will run on port: {}", port);
}

// Check if a variable exists before using it
if exists("DEBUG") {
    println!("Debug mode is enabled");
}

// List all accessible variables (if allow_list_all is true)
for (name, value) in list_vars() {
    println!("{}={}", name, value);
}
```

## Comparison with Other Handlers

| Feature | Environment Handler | File System Handler | HTTP Client Handler |
|---------|-------------------|-------------------|-------------------|
| **Access Type** | Read-only | Read/Write | Outbound requests |
| **Security Model** | Allowlist/Denylist | Path restrictions | URL filtering |
| **Audit Trail** | Complete | Complete | Complete |
| **Sandboxing** | Variable-level | Directory-level | Network-level |

The Environment Handler is designed to be the safest way for agents to access host configuration while maintaining the security guarantees that make Theater suitable for autonomous AI agent deployment.
