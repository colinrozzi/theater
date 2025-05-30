# Actor Testing Framework

| Field     | Value                                             |
|-----------|---------------------------------------------------|
| Date      | 2025-03-21                                        |
| Author    | Theater Team                                      |
| Status    | Proposal                                          |
| Priority  | Medium                                            |

## Overview

Add a comprehensive testing framework with a new `theater test` command that enables structured, reproducible testing of actor behavior through declarative test definitions. This framework will allow developers to verify actor functionality, especially for complex interactions like channel-based streaming communication.

## Motivation

The Theater actor system currently lacks a formalized testing approach, making it difficult to:

1. Verify actor behavior in a reproducible manner
2. Test complex multi-step actor interactions
3. Validate streaming scenarios and channel-based communication
4. Create regression tests to catch regressions during development
5. Document expected actor behavior through executable tests

This proposal aims to address these gaps by introducing a structured testing framework that supports both simple request-response testing and complex, multi-step interaction flows including channel-based streaming communication.

## Design

### 1. Test Definition Format

Tests will be defined in JSON files with a structured format:

```json
{
  "name": "Example Actor Test",
  "description": "Tests the basic functionality of an example actor",
  "setup": {
    "actors": [
      {
        "id": "example-actor", 
        "manifest": "path/to/actor.toml",
        "init_data": { /* Optional JSON init data */ }
      }
    ]
  },
  "steps": [
    {
      "description": "Send a request to the actor",
      "action": "request",
      "address": "example-actor",
      "message": { "command": "hello" },
      "expect": {
        "result": { "status": "success" }
      }
    },
    // Additional steps...
  ],
  "teardown": {
    "cleanup": true  // Whether to stop actors after test
  }
}
```

### 2. Supported Actions

The testing framework will support a variety of actions:

#### Basic Communication
- `send`: One-way message sending
- `request`: Request-response pattern
- `wait`: Pause for a specified duration or condition

#### Channel Operations
- `open_channel`: Open a new channel to an actor
- `send_channel_message`: Send a message on an existing channel
- `close_channel`: Close an existing channel
- `expect`: Wait for a specific message or pattern on a channel

#### Variables and References
- Support for variable references (e.g., `${channel_id_0}`)
- Result capture from previous steps
- Environment variable substitution

### 3. Expectations

The framework will include a robust pattern matching system for validating responses:

```json
"expect": {
  "type": "channel_message",
  "timeout": 5000,  // milliseconds
  "pattern": {
    "Progress": {
      "stage": "Initializing"
      // Can match exact values or use wildcards
    }
  }
}
```

Pattern matching supports:
- Exact value matching
- Type checking
- Partial matching (only specified fields)
- Regular expressions for string values
- Custom validation functions (future enhancement)

### 4. CLI Command Structure

```
theater test [options] <test-file>
```

Options:
- `--verbose`: Show detailed test execution information
- `--timeout <ms>`: Override default timeout for all expect operations
- `--no-cleanup`: Don't stop actors after test completion
- `--report <format>`: Specify output format (text, json, junit)

### 5. Implementation Strategy

The implementation will consist of:

1. **Test Parser**: Parses and validates test definition files
2. **Test Runner**: Executes test steps sequentially
3. **Action Handlers**: Specific implementations for each action type
4. **Expectation Matchers**: Pattern matching system for validating responses
5. **Variable System**: Handles variable substitution and context
6. **Reporting System**: Formats and presents test results

### 6. Execution Flow

1. Parse test file and validate structure
2. Set up test environment (start required actors)
3. Execute steps sequentially:
   - Perform action
   - Process expectations (if any)
   - Capture results for variable substitution
4. Report results as test progresses
5. Clean up resources during teardown
6. Generate final report

## Use Cases

### 1. Simple Request-Response Testing

```json
{
  "steps": [
    {
      "description": "Test simple request-response",
      "action": "request",
      "address": "calculator-actor",
      "message": { "operation": "add", "numbers": [5, 7] },
      "expect": {
        "result": { "value": 12 }
      }
    }
  ]
}
```

### 2. Channel-Based Communication Testing

```json
{
  "steps": [
    {
      "description": "Open channel to programmer",
      "action": "open_channel",
      "address": "@programmer",
      "message": {}
    },
    {
      "description": "Start change process",
      "action": "send_channel_message",
      "channel_id": "${channel_id_0}",
      "message": {
        "command": "start_change",
        "change": "Print 'Hello Fun World!' when handling a request"
      },
      "expect": {
        "type": "channel_message",
        "pattern": {
          "Progress": {
            "stage": "Initializing",
            "description": "Starting change request"
          }
        }
      }
    },
    {
      "description": "Wait for completion",
      "action": "expect",
      "expectation": {
        "type": "channel_message",
        "pattern": {
          "TaskComplete": {
            "success": true
          }
        }
      }
    },
    {
      "description": "Close channel",
      "action": "close_channel",
      "channel_id": "${channel_id_0}"
    }
  ]
}
```

### 3. Multi-Actor Interaction Testing

```json
{
  "setup": {
    "actors": [
      { "id": "producer", "manifest": "producer.toml" },
      { "id": "consumer", "manifest": "consumer.toml" }
    ]
  },
  "steps": [
    // Steps to test interaction between producer and consumer
  ]
}
```

## Benefits

1. **Reproducibility**: Tests can be run consistently across different environments
2. **Documentation**: Test files serve as executable documentation of expected behavior
3. **Regression Prevention**: Changes can be validated against existing test suite
4. **Development Speed**: Faster feedback loop for actor developers
5. **Complex Scenario Testing**: Support for testing multi-step, multi-actor scenarios
6. **Channel Testing**: First-class support for testing streaming communication

## Implementation Plan

### Phase 1: Core Framework
1. Implement basic test runner with support for `request` and `send` actions
2. Add simple expectation matching system
3. Create basic CLI interface for the test command

### Phase 2: Channel Support
1. Add channel-related actions (`open_channel`, `send_channel_message`, etc.)
2. Implement channel message expectation matching
3. Add variable substitution system for dynamically created resources

### Phase 3: Advanced Features
1. Add support for multi-actor setup and teardown
2. Implement enhanced pattern matching for complex objects
3. Add reporting capabilities in multiple formats
4. Create comprehensive documentation and examples

## Conclusion

The Actor Testing Framework will significantly improve the developer experience when building and maintaining Theater actors by providing a structured, reproducible way to verify actor behavior. The framework's support for complex interaction patterns, especially channel-based streaming communication, will enable developers to build more reliable actors with better-documented behavior.
