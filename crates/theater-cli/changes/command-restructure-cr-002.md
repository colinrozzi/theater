# Theater CLI Command Structure Restructure - Change Request

**Status**: Planning Phase - Ready for Implementation  
**Created**: 2025-07-20  
**Updated**: 2025-07-20  
**Target**: Phase 3 - Command Organization and Enhanced Functionality  
**Related**: [modernization-cr-001.md](./modernization-cr-001.md)

## Executive Summary

This change request proposes a comprehensive restructuring of the Theater CLI command organization from a flat command structure to a hierarchical, grouped structure. The restructuring addresses scalability concerns, improves discoverability, and provides a foundation for advanced functionality while maintaining backward compatibility.

## Problem Statement

The current CLI has grown to include numerous commands in a flat structure that creates several issues:

### Current State Analysis
- **18+ top-level commands** making help output overwhelming
- **Poor discoverability** of related functionality scattered across different command names
- **Limited scalability** as new features require new top-level commands
- **Inconsistent naming** and organization of related operations
- **No logical grouping** of actor, event, message, and server operations

### Specific Pain Points
1. **Actor Operations Scattered**: `create`, `build`, `start`, `stop`, `list`, `inspect`, `state`, `list-stored` all at top level
2. **Event Commands Mixed**: `events`, `events-explore`, `subscribe` don't clearly indicate they're related
3. **No Server Management**: No way to check server status, logs, or configuration
4. **Future Growth Blocked**: Adding new functionality requires polluting the top-level namespace

## Solution Overview

Implement a hierarchical command structure with logical groupings:

```bash
# Current (Flat Structure)
theater create <name>
theater build [path]
theater start <manifest>
theater stop <id>
theater list
theater events <id>
theater subscribe <id>
theater message <id> <msg>

# Proposed (Hierarchical Structure)
theater actor create <name>
theater actor build [path]
theater actor start <manifest>
theater actor stop <id>
theater actor list

theater event list <id>
theater event explore <id>
theater event subscribe <id>

theater message send <id> <msg>
theater message channel open <id>

theater server status
theater server logs
```

---

## 📋 Implementation Plan

### Phase 3.1: Command Structure Foundation ⏳
- [ ] Create new hierarchical CLI command definitions
- [ ] Implement command group dispatch logic
- [ ] Maintain backward compatibility with top-level shortcuts
- [ ] Update help text and documentation

### Phase 3.2: Directory Restructuring ⏳
- [ ] Reorganize `src/commands/` into logical subdirectories
- [ ] Move existing command implementations to new locations
- [ ] Update module imports and exports
- [ ] Create module structure for new command groups

### Phase 3.3: Enhanced Functionality ⏳
- [ ] Implement new server management commands
- [ ] Add advanced event management operations
- [ ] Enhance message/channel operations
- [ ] Add configuration management commands

### Phase 3.4: Advanced Features 📋
- [ ] Enhanced tab completion for nested commands
- [ ] Context-aware help and suggestions
- [ ] Command aliasing and shortcuts
- [ ] Command history and favorites

---

## 🏗️ Detailed Design

### New Command Hierarchy

#### Actor Commands (`theater actor`)
**Purpose**: Complete actor lifecycle management

```bash
theater actor create <name> [--template <template>]
theater actor build [path] [--release] [--clean]
theater actor start <manifest> [--subscribe] [--parent]
theater actor stop <id> [--timeout <sec>] [--force]
theater actor restart <id> [--timeout <sec>]
theater actor list [--format <format>]
theater actor inspect <id>
theater actor state <id>
theater actor scale <id> <instances>
theater actor list-stored
```

#### Event Commands (`theater event`)
**Purpose**: Comprehensive event system operations

```bash
theater event list <actor-id> [--limit <n>] [--format <format>]
theater event explore <actor-id> [--live] [--follow]
theater event subscribe <actor-id> [--format <format>]
theater event export <actor-id> [--output <file>] [--format <format>]
theater event replay <actor-id> [--from <index>] [--to <index>] [--speed <multiplier>]
theater event search <query> [--actor-id <id>] [--type <type>]
theater event analyze <actor-id> [--analysis <type>] [--window <time>]
theater event watch <actor-id> [--follow] [--format <format>]
```

#### Message Commands (`theater message`)
**Purpose**: All communication and messaging operations

```bash
theater message send <actor-id> <message>
theater message broadcast <message> [--targets <ids>] [--selector <selector>]
theater message channel open <actor-id>
theater message channel close <channel-id>
theater message channel list [--actor-id <id>]
theater message list-channels [--actor-id <id>]
```

#### Server Commands (`theater server`) - NEW
**Purpose**: Server management and monitoring

```bash
theater server status [--watch <interval>]
theater server logs [--lines <n>] [--follow] [--level <level>]
theater server config [--key <key>]
theater server metrics [--type <type>] [--window <time>]
theater server health [--check] [--monitor]
```

#### Config Commands (`theater config`) - NEW
**Purpose**: CLI configuration management

```bash
theater config show [--section <section>]
theater config set <key> <value>
theater config get <key>
theater config reset [--section <section>] [--force]
theater config edit [--editor <editor>]
```

#### Completion Commands (`theater completion`)
**Purpose**: Shell completion management

```bash
theater completion generate <shell>
theater completion install <shell> [--force]
```

### Backward Compatibility

Maintain top-level shortcuts for the most common operations:

```bash
# These still work (shortcuts to new commands)
theater start <manifest>    # -> theater actor start <manifest>
theater list                # -> theater actor list
theater stop <id>           # -> theater actor stop <id>
```

### Directory Structure

```
src/commands/
├── mod.rs                 # Re-exports all command modules
├── actor/                 # Actor lifecycle commands
│   ├── mod.rs
│   ├── create.rs          # theater actor create
│   ├── build.rs           # theater actor build
│   ├── start.rs           # theater actor start
│   ├── stop.rs            # theater actor stop
│   ├── restart.rs         # theater actor restart (NEW)
│   ├── list.rs            # theater actor list
│   ├── inspect.rs         # theater actor inspect
│   ├── state.rs           # theater actor state
│   ├── scale.rs           # theater actor scale (NEW)
│   └── list_stored.rs     # theater actor list-stored
├── event/                 # Event system commands
│   ├── mod.rs
│   ├── list.rs            # theater event list (was events.rs)
│   ├── explore.rs         # theater event explore (was events_explore.rs)
│   ├── subscribe.rs       # theater event subscribe
│   ├── export.rs          # theater event export (NEW)
│   ├── replay.rs          # theater event replay (NEW)
│   ├── search.rs          # theater event search (NEW)
│   ├── analyze.rs         # theater event analyze (NEW)
│   └── watch.rs           # theater event watch (NEW)
├── message/               # Messaging commands
│   ├── mod.rs
│   ├── send.rs            # theater message send (was message.rs)
│   ├── broadcast.rs       # theater message broadcast (NEW)
│   ├── list_channels.rs   # theater message list-channels (NEW)
│   └── channel/           # Channel subcommands
│       ├── mod.rs
│       ├── open.rs        # theater message channel open
│       ├── close.rs       # theater message channel close (NEW)
│       └── list.rs        # theater message channel list (NEW)
├── server/                # Server management commands (NEW)
│   ├── mod.rs
│   ├── status.rs          # theater server status
│   ├── logs.rs            # theater server logs
│   ├── config.rs          # theater server config
│   ├── metrics.rs         # theater server metrics
│   └── health.rs          # theater server health
├── config/                # Configuration commands (NEW)
│   ├── mod.rs
│   ├── show.rs            # theater config show
│   ├── set.rs             # theater config set
│   ├── get.rs             # theater config get
│   ├── reset.rs           # theater config reset
│   └── edit.rs            # theater config edit
└── completion/            # Completion commands
    ├── mod.rs
    ├── generate.rs        # theater completion generate (was completion.rs)
    ├── install.rs         # theater completion install (NEW)
    └── dynamic_completion.rs
```

---

## 🔧 Technical Implementation

### Command Dispatch Changes

#### Current Implementation
```rust
#[derive(Debug, Subcommand)]
pub enum Commands {
    Create(commands::create::CreateArgs),
    Build(commands::build::BuildArgs),
    Start(commands::start::StartArgs),
    // ... 15+ more top-level commands
}
```

#### New Implementation
```rust
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Actor lifecycle management
    Actor(ActorCommands),
    /// Event system operations  
    Event(EventCommands),
    /// Messaging operations
    Message(MessageCommands),
    /// Server operations
    Server(ServerCommands),
    /// Configuration management
    Config(ConfigCommands),
    /// Completion management
    Completion(CompletionCommands),
    
    // Backward compatibility shortcuts
    Start(commands::actor::start::StartArgs),
    List(commands::actor::list::ListArgs),
    Stop(commands::actor::stop::StopArgs),
}
```

### Migration Strategy

1. **File Movement**: Use automated script to move existing command files to new directory structure
2. **Import Updates**: Update module imports to reflect new structure
3. **Command Registration**: Update CLI definitions to use new hierarchy
4. **Stub Creation**: Create placeholder implementations for new commands
5. **Testing**: Verify all existing functionality still works

### New Command Implementations

Priority new commands to implement:

1. **Server Status** (`theater server status`)
   - Connection status to server
   - Server uptime and version
   - Basic resource usage
   - Number of active actors

2. **Event Export** (`theater event export`)
   - Export events to JSON, CSV, or YAML
   - Time range filtering
   - Event type filtering
   - Large dataset handling

3. **Config Management** (`theater config show/set/get`)
   - Display current configuration
   - Set configuration values
   - Reset to defaults
   - Edit in preferred editor

---

## 📊 Success Metrics

### User Experience Metrics
- **Discoverability**: Time to find relevant command reduced by 50%
- **Learning Curve**: New users can understand command structure in < 5 minutes
- **Efficiency**: Power users can access functionality 30% faster

### Developer Metrics  
- **Maintainability**: Adding new commands requires 60% less boilerplate
- **Testing**: Command-specific tests can be written more easily
- **Documentation**: Help text is automatically organized and discoverable

### Backward Compatibility
- **No Breaking Changes**: All existing scripts continue to work
- **Transition Path**: Users can gradually adopt new command structure
- **Migration Support**: Clear documentation for moving to new commands

---

## 🧪 Testing Strategy

### Existing Functionality Tests
- [ ] All current commands work with new structure
- [ ] Backward compatibility shortcuts function correctly
- [ ] Output formats remain consistent
- [ ] Error messages are preserved

### New Command Tests
- [ ] Each new command has basic functionality tests
- [ ] Help text is accurate and helpful
- [ ] Input validation works correctly
- [ ] Error handling follows established patterns

### Integration Tests
- [ ] Command completion works correctly
- [ ] Nested command dispatch functions properly
- [ ] Configuration system integrates with new commands
- [ ] Output formatting works across all command groups

---

## 🚀 Migration Plan

### Pre-Migration
1. **Backup Current State**: Create branch with current implementation
2. **Update Documentation**: Document current command structure
3. **Test Coverage**: Ensure good test coverage for existing commands

### Migration Execution
1. **Run Migration Script**: Automated file movement and directory creation
2. **Update Imports**: Fix module imports across codebase
3. **Update CLI Definition**: Replace flat structure with hierarchical
4. **Create Stubs**: Implement basic versions of new commands
5. **Test Existing Commands**: Verify no regressions

### Post-Migration
1. **Update Documentation**: Reflect new command structure
2. **Enhanced Tab Completion**: Implement completion for nested commands
3. **User Communication**: Announce changes and migration path
4. **Implement New Features**: Build out the enhanced functionality

---

## 🔮 Future Enhancements

### Command Aliasing
Allow users to create custom shortcuts:
```bash
theater alias create deploy "actor start --subscribe"
theater deploy my-manifest.toml  # runs: theater actor start --subscribe my-manifest.toml
```

### Command Context
Remember context to reduce typing:
```bash
theater actor select my-actor-123
theater state        # operates on my-actor-123
theater restart      # operates on my-actor-123
```

### Interactive Mode
```bash
theater interactive
theater> actor create my-new-actor
theater> actor start my-new-actor/manifest.toml
theater> event subscribe my-new-actor
```

### Command Templates
```bash
theater template create deployment
theater template run deployment --actor my-actor
```

---

## 📝 Implementation Checklist

### Phase 3.1: Foundation
- [ ] Create new CLI command structure definitions
- [ ] Implement hierarchical command dispatch
- [ ] Add backward compatibility shortcuts
- [ ] Update help text and documentation

### Phase 3.2: File Organization  
- [ ] Create new directory structure
- [ ] Move existing command files to new locations
- [ ] Update all module imports
- [ ] Create module files for new structure

### Phase 3.3: New Commands
- [ ] Implement `theater server status`
- [ ] Implement `theater server logs`
- [ ] Implement `theater event export`
- [ ] Implement `theater config show/set/get`
- [ ] Implement remaining stub commands

### Phase 3.4: Enhanced Features
- [ ] Enhanced tab completion
- [ ] Command context awareness
- [ ] Interactive help improvements
- [ ] Performance optimizations

---

## 📚 Related Documents

- [modernization-cr-001.md](./modernization-cr-001.md) - Foundation modernization
- Architecture documentation in main README
- CLI user guide (to be updated)

---

## 🎯 Conclusion

This command structure restructuring represents a significant improvement in the Theater CLI's usability, maintainability, and extensibility. By organizing commands into logical groups, we create a foundation that can grow naturally while maintaining the excellent user experience established in the previous modernization phase.

The hierarchical structure aligns with user mental models (actors, events, messages, server) and provides clear pathways for discovering and using functionality. Combined with backward compatibility shortcuts, this change will benefit both new and existing users.

**Next Steps**: Begin Phase 3.1 implementation with the new CLI command structure definitions.
