# Theater Manifest Variable Substitution - Migration Guide

## Overview

Theater v0.2.2 introduces variable substitution in manifest files, allowing dynamic configuration based on initial state values. This feature is **fully backward compatible** - existing manifests will continue to work without changes.

## What's New

### Variable Syntax
- `${variable_name}` - Simple variable reference
- `${nested.object.path}` - Nested object access using dot notation  
- `${variable_name:default_value}` - Variable with default value

### Example Usage

**Before (static configuration):**
```toml
name = "my-processor"
component = "./dist/processor.wasm"
save_chain = true

[[handler]]
type = "filesystem"
path = "/tmp/data"

[[handler]]
type = "http-client"
base_url = "https://api.example.com"
timeout = 5000
```

**After (dynamic configuration):**
```toml
name = "${app.name}"
component = "${build.component_path}"
save_chain = ${logging.save_events:true}

[[handler]]
type = "filesystem"
path = "${workspace.data_dir}"

[[handler]]
type = "http-client"
base_url = "${api.endpoint:https://api.default.com}"
timeout = ${api.timeout_ms:5000}
```

**Initial State (config.json):**
```json
{
  "app": {
    "name": "production-processor"
  },
  "build": {
    "component_path": "./dist/processor-v2.wasm"
  },
  "workspace": {
    "data_dir": "/var/data/prod"
  },
  "api": {
    "endpoint": "https://prod-api.example.com/v1",
    "timeout_ms": 10000
  },
  "logging": {
    "save_events": true
  }
}
```

## Migration Steps

### 1. No Changes Required for Existing Manifests
Your existing manifests will continue to work exactly as before. No migration is required.

### 2. Optional: Add Variable Substitution
To use the new feature:

1. **Identify Dynamic Values**: Look for configuration values that change between environments (dev/staging/prod) or deployments.

2. **Create Initial State File**: Extract these values into a JSON file:
   ```json
   {
     "environment": "production",
     "database": {
       "host": "prod-db.example.com",
       "port": 5432
     },
     "api": {
       "base_url": "https://prod-api.example.com"
     }
   }
   ```

3. **Update Manifest**: Replace static values with variable references:
   ```toml
   name = "app-${environment}"
   
   [[handler]]
   type = "database"
   host = "${database.host}"
   port = ${database.port}
   
   [[handler]]
   type = "http-client"
   base_url = "${api.base_url}"
   ```

4. **Reference Initial State**: Add the init_state field:
   ```toml
   init_state = "config.json"
   # or init_state = "https://config-server.com/prod-config.json"
   # or init_state = "store://my-store/prod-config"
   ```

### 3. CLI Usage Updates

**No changes required** - the CLI automatically detects and processes variables:

```bash
# Works with or without variables
theater start manifest.toml

# Override state still works
theater start manifest.toml --initial-state override.json
```

## New API Methods

### For Library Users

If you're using Theater as a library, new methods are available:

```rust
use theater::config::actor_manifest::ManifestConfig;
use serde_json::json;

// Original method (still works)
let config = ManifestConfig::from_str(toml_content)?;

// New method with variable substitution
let override_state = json!({"env": "staging"});
let config = ManifestConfig::from_str_with_substitution(
    toml_content, 
    Some(&override_state)
).await?;
```

## Best Practices

### 1. Use Defaults for Optional Values
```toml
debug_mode = ${features.debug:false}
timeout = ${api.timeout:30000}
```

### 2. Group Related Configuration
```json
{
  "database": {
    "host": "localhost",
    "port": 5432,
    "name": "myapp"
  },
  "api": {
    "base_url": "https://api.example.com",
    "timeout": 5000
  }
}
```

### 3. Use Environment-Specific Config Files
```bash
# Development
theater start manifest.toml --initial-state config/dev.json

# Production  
theater start manifest.toml --initial-state config/prod.json
```

### 4. Secure Sensitive Values
For production deployments, consider using Theater's store:// references for sensitive configuration:

```toml
init_state = "store://secure-store/prod-config"
```

## Error Handling

### Variable Not Found
```
Error: Variable substitution failed: Variable 'missing.variable' not found and no default provided
```

**Solution**: Add a default value or ensure the variable exists in your initial state:
```toml
# Add default
value = ${missing.variable:default_value}

# Or add to initial state
{"missing": {"variable": "value"}}
```

### Invalid Path
```
Error: Variable substitution failed: Invalid JSON path 'string.field': Cannot traverse into string value
```

**Solution**: Check your JSON structure and variable paths:
```toml
# If your state is: {"config": "simple_string"}
# This won't work:
value = ${config.field}

# This will work:
value = ${config}
```

## Troubleshooting

### Variables Not Being Substituted
1. **Check syntax**: Ensure variables use `${...}` format
2. **Verify initial state**: Make sure your JSON is valid and contains the referenced paths
3. **Check file paths**: Ensure init_state file paths are correct

### Performance Considerations
- Variable substitution adds minimal overhead
- Initial state is cached during the actor's lifetime
- No impact on manifests without variables

## Advanced Features

### Array Access
```toml
primary_server = "${servers.0.hostname}"
backup_server = "${servers.1.hostname}"
```

```json
{
  "servers": [
    {"hostname": "primary.example.com"},
    {"hostname": "backup.example.com"}
  ]
}
```

### Complex Data Types
```toml
# Arrays
tags = ${metadata.tags}

# Objects (converted to inline TOML tables)
config = ${server.settings}
```

## Security Considerations

### Restrictions
- The `init_state` field cannot contain variables (prevents circular dependencies)
- Variables can only access data from the resolved initial state
- No access to environment variables or external data sources

### Safe Practices
- Store sensitive configuration in Theater's secure store
- Use file permissions to protect initial state files
- Validate initial state JSON structure in your deployment pipeline

## Future Enhancements

The variable substitution system is designed to be extensible. Future versions may include:

- Function calls: `${env("HOME")}`, `${uuid()}`
- Conditional logic: `${debug_mode ? "verbose" : "quiet"}`
- Template includes: `${include("common.toml")}`

## Getting Help

If you encounter issues with variable substitution:

1. Check this migration guide
2. Review the error messages for specific guidance
3. Test with simple variable references first
4. Reach out to colinrozzi@gmail.com for support

## Changelog

### v0.2.2
- ✅ Added variable substitution with `${var}`, `${var:default}`, and `${nested.path}` syntax
- ✅ Support for array indexing and complex data types
- ✅ Full backward compatibility with existing manifests
- ✅ New `ManifestConfig::from_str_with_substitution()` API method
- ✅ Comprehensive error handling and validation
- ✅ Integration tests and documentation

## Implementation Details

This feature is powered by the excellent [`subst`](https://lib.rs/crates/subst) crate, providing:
- Shell-like variable substitution with `${}` syntax
- Built-in default value support
- Recursive substitution in default values
- Production-tested reliability with 1M+ downloads
