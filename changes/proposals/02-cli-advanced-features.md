# Change Request: Theater CLI Advanced Features

## Overview
This change request extends the basic CLI enhancement (01-cli-enhancement.md) with additional advanced features to further improve developer experience and add more powerful management capabilities.

## Motivation
While the basic CLI enhancement provides essential actor management functionality, additional features are needed to support more complex workflows, debugging, content management, and structured actor inspection. These advanced features will make Theater more powerful and user-friendly.

## Detailed Design

### 1. Actor Inspection and Visualization
- `theater inspect <id>` - Show detailed information about an actor including its manifest, interfaces, status, and metrics in a single view
- `theater tree` - Show the actor hierarchy with parent-child relationships as a tree view

### 2. Content Store Management
- `theater store list` - List content in the store
- `theater store get <hash>` - Retrieve content from store
- `theater store put <file>` - Add content to store

### 3. Actor Debugging
- `theater debug <id>` - Attach to an actor in a special debug mode with enhanced logging
- `theater replay <id> <event-id>` - Replay an actor's state to a specific event in its chain

### 4. Actor Communication Utilities
- `theater call <id> <function> <args>` - Call a specific function on an actor
- `theater websocket <id> <path>` - Open an interactive WebSocket session with an actor

### 5. Project Management
- `theater validate <manifest>` - Validate a manifest file without deploying
- `theater build <directory>` - Build an actor project (run cargo build with correct targets)
- `theater export <id> <directory>` - Export an actor's state, events, and manifest

### 6. Enhanced Configuration and Output
- Define consistent error messages and codes
- Add `--format json` flag to commands for machine-readable output
- Implement progress indicators for long-running operations

### 7. Log/Event Storage Structure
- Store logs per-actor in: `~/.local/share/theater/logs/<actor-id>/`
- Store events in: `~/.local/share/theater/events/<actor-id>/`
- Allow export/import of these logs and events

### 8. Project Structure Conventions
For newly created projects, establish a convention:
```
my-actor/
  Cargo.toml
  manifest.toml    # Actor manifest 
  src/             # Actor source code
  wit/             # WIT interfaces
  examples/        # Example usage
  .theater.toml    # Project-specific config
```

## Implementation Plan
1. Build on top of the basic CLI enhancement
2. Implement content store management commands
3. Add actor inspection and visualization
4. Create debugging and replay capabilities
5. Develop project management utilities
6. Enhance configuration handling and output formatting

## Dependencies
- This change builds upon the basic CLI enhancement (01-cli-enhancement.md) and should be implemented after that change is complete
- Some features may require extensions to the Theater server API

## Impacts
- Further improves developer experience
- Enhances debugging and troubleshooting capabilities
- Provides better visibility into actor state and relationships
- Establishes consistent conventions for project structure

## Alternatives Considered
- Implementing a graphical user interface: May be considered as a separate future enhancement
- Embedding these features in IDEs: Could be explored through editor extensions in the future
