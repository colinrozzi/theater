# Change Request: Theater CLI Enhancement

## Overview
Enhance the Theater CLI to support actor development, deployment, and management. This change will transform the existing simple server startup CLI into a comprehensive tool for working with Theater actors.

## Motivation
Currently, developers need to manually manage actor deployment and monitoring through direct TCP connections to the Theater server. A dedicated CLI tool will make the developer experience more streamlined, allowing users to create, run, and monitor actors from their development directories without requiring complex setup.

## Detailed Design

### 1. Extended CLI Structure
Extend the current CLI with subcommands while maintaining the existing server startup functionality:

```
theater                    # Start server (current behavior)
theater create <name>      # Create new actor project 
theater deploy <path>      # Deploy actor from manifest
theater list               # List running actors
theater logs <id>          # Stream logs from actor
theater state <id>         # Get actor state
theater events <id>        # Get actor events
theater start <manifest>   # Start an actor
theater stop <id>          # Stop an actor
theater restart <id>       # Restart an actor
theater message <id> <msg> # Send message to actor
theater watch <path>       # Watch directory and redeploy on changes
```

### 2. Client Library Implementation
Create a client module that handles connecting to the Theater server:
- Connect using the TCP framing protocol
- Serialize/deserialize commands and responses
- Handle subscription events

### 3. File Structure Changes

```
/src
  /cli                    # New directory
    mod.rs                # CLI module
    client.rs             # TCP client for management API
    commands/             # Subcommand implementations
      create.rs           # Project creation
      deploy.rs           # Actor deployment
      ...
    templates/            # Project templates
      basic/              # Basic actor template
      http/               # HTTP actor template
      ...
```

### 4. Configuration Management
- Add a `.theater.toml` config file format for project settings
- Use standard config file locations:
  - Global: `~/.config/theater/config.toml`
  - Project: `./.theater.toml`
- Store server connection info and project defaults

### 5. Project Templating
- Templates stored in `~/.config/theater/templates/`
- Each template has a structure with template metadata and source files
- Include templates for common actor patterns (HTTP, messaging, etc.)

### 6. Actor Management Features
- Implement commands for starting, stopping, and restarting actors
- Add status and information retrieval
- Support message sending and function calling

### 7. Monitoring and Logging
- Implement log streaming from actors
- Add state inspection and event chain viewing
- Create human-readable formatting for logs and events

## Implementation Plan
1. Refactor CLI structure with clap subcommands
2. Implement client library for management API
3. Add project templating capabilities
4. Implement actor management commands
5. Add monitoring and logging features

## Impacts
- Improves developer experience when working with Theater actors
- Makes it easier to create, run, and debug actors
- Reduces the need for manual TCP communication with the server
- Establishes conventions for project structure and configuration

## Alternatives Considered
- Creating a separate CLI tool: Rejected in favor of extending the existing CLI to maintain a single entry point
- Web-based management interface: May be considered for future enhancement but CLI is preferred for developer workflows
