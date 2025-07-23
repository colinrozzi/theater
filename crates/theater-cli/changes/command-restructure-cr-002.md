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
theater actor send <id> <msg>
theater actor request <id> <msg>
theater actor channel <id>
theater actor subscribe <id>

theater event list
theater event <event-id>
theater event chain <event-id>

theater server status
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
theater actor inspect <id>
theater actor state <id>
theater actor send <id> <message>
theater actor request <id> <message> [--timeout <sec>]
theater actor channel <id> [--create] [--list]
theater actor subscribe <id> [--format <format>]
```

#### Event Commands (`theater event`)
**Purpose**: Comprehensive event system operations

```bash
theater event list <actor-id> [--limit <n>] [--format <format>]
theater event export <actor-id> [--output <file>] [--format <format>]
```

#### Server Commands (`theater server`) - NEW
**Purpose**: Server management and monitoring

```bash
theater server list [--all] [--format <format>]
theater server status [--watch <interval>]
theater server config [--key <key>]
theater server config show [--section <section>]
theater server config edit [--editor <editor>]
theater server metrics [--type <type>] [--window <time>]
theater server health [--check] [--monitor]
```

#### Completion Commands (`theater completion`)
**Purpose**: Shell completion management

```bash
theater completion generate <shell>
theater completion install <shell> [--force]
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
    /// Server operations
    Server(ServerCommands),
    /// Completion management
    Completion(CompletionCommands)
}
```

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

3. **Config Management** (`theater server config show/set/get`)
   - Display current configuration
   - Set configuration values
   - Reset to defaults
   - Edit in preferred editor

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

---

## 📝 Implementation Checklist

### Phase 3.1: Foundation
- [ ] Create new CLI command structure definitions
- [ ] Implement hierarchical command dispatch
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

The hierarchical structure aligns with user mental models (actors, events, messages, server) and provides clear pathways for discovering and using functionality. This change will benefit both new and existing users.

**Next Steps**: Begin Phase 3.1 implementation with the new CLI command structure definitions.
