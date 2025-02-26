# Theater Actor Registry

The Actor Registry allows referencing actors by name instead of by full file paths, simplifying actor management and reuse.

## Usage

### Initializing a Registry

```bash
# Initialize a registry in the default location (~/.theater/registry)
theater registry init

# Or specify a custom location
theater registry init ~/my-registry
```

### Registering Actors

Register existing actors with the registry:

```bash
# Register an actor from a manifest or directory
theater registry register /path/to/actor 

# Register with a specific registry
theater registry register /path/to/actor --registry ~/my-registry
```

### Listing Registered Actors

```bash
# List all registered actors
theater registry list

# With a specific registry
theater registry list --path ~/my-registry
```

### Starting Actors by Name

Once an actor is registered, you can start it by name:

```bash
# Start the latest version of an actor
theater actor start chat

# Start a specific version
theater actor start chat:0.1.0
```

## Registry Structure

The registry follows this structure:

```
/registry
  ├── config.toml         # Registry configuration
  ├── index.toml          # Master index of all registered actors
  ├── components/         # Directory containing all registered WASM components
  │   ├── {name}/         # Directory for each actor by name
  │   │   ├── {version}/  # Directory for each version
  │   │   │   └── {name}.wasm  # The actual WASM component
  ├── manifests/          # Directory containing all actor manifests
  │   ├── {name}/         # Directory for each actor by name
  │   │   ├── {version}/  # Directory for each version
  │   │   │   └── actor.toml  # The actor manifest
  └── cache/              # Cache directory for faster lookups
```

## Registry Location

The registry will be automatically located in the following order:

1. The path specified in the `THEATER_REGISTRY` environment variable
2. The user's home directory at `~/.theater/registry`
3. A local `registry` directory in the current working directory
4. A project-relative `../registry` directory

## Actor References

Actors can be referenced in different ways:

- `name` - Uses the latest version of the named actor (e.g., `chat`)
- `name:version` - Uses a specific version (e.g., `chat:0.1.0`)
- `/path/to/actor.toml` - Direct path reference (fallback)

## Future Enhancements

- Remote registry support
- Actor dependency management
- Versioning strategies (latest, compatible, exact)
- Registry replication and synchronization
