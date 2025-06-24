# theater-cli

> [!NOTE]
> This documentation is incomplete, please reach out to me at colinrozzi@gmail.com I very much appreciate your interest and would love to hear from you!

Command-line interface for the Theater WebAssembly actor system.

## Installation

```bash
cargo install theater-cli
```

## Create a new agent project
For now, we have to use cargo component manually
```bash
cargo component new my-agent --lib
```
Navigate to the agent directory
```bash
cd my-agent
```
edit wit/world.wit to include theater runtime interfaces:
[theater-simple](https://wa.dev/theater:simple)
a minimal world is:
```wit
package component:my-agent;

world default {
    import theater:simple/runtime;
    export theater:simple/actor;
}
```

Create a `manifest.toml` file in the agent directory with the following content:
```toml
name = "my-agent"
version = "0.1.0"
component = "will be replaced by theater build"

[[handlers]]
type = "runtime"

[handlers.config]
```

## Build the agent
```bash
theater build
```
OR 
```bash
cargo component build --target wasm32-unknown-unknown
```
theater build will automatically update the component path in `manifest.toml` to point to the built component, but it does hide some information that is nice to see.

## Start an agent
```bash
theater start manifest.toml
```
Ensure the Theater server is running before starting agents, check out theater-server-cli for information on starting the server. The server manages the lifecycle of agents and provides a UI for monitoring.

## List running agents
```bash
theater list
```

## View agent events
```bash
theater events <agent-id>
```

## License

This project is licensed under the Apache License 2.0 - see the [LICENSE](../../LICENSE) file for details.
