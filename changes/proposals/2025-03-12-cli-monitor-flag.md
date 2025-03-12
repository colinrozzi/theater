# Change Request: CLI Start Command Monitor Flag

## Overview
Add a `--monitor` flag to the `theater start` command that will start the actor as normal but then also continuously monitor events from the actor and print them to stdout in real-time, allowing developers to observe actor behavior without running a separate command.

## Motivation
Currently, developers need to use two separate commands to start an actor and then check its events:
1. `theater start <manifest>` to start the actor
2. `theater events <id>` to view events from that actor (which only shows a snapshot of events)

The current `events` command only retrieves events at a point in time and doesn't provide a real-time monitoring capability. Adding a `--monitor` flag to the start command will streamline the development experience by allowing developers to continuously observe events as they happen, which is crucial for debugging and understanding actor behavior.

## Detailed Design

### 1. CLI Argument Addition
Extend the `start` command to accept a `--monitor` flag:

```
theater start <manifest> [--monitor]
```

### 2. Implementation Changes

#### Modified Files:
- `/src/cli/commands/start.rs` - Add the flag option and implement the monitoring logic
- `/src/theater_server.rs` - Ensure proper support for event subscription and streaming

#### New Functionality:
The TheaterClient already has methods for subscription but they're not fully utilized in the CLI:
- `subscribe_to_actor()` - Exists but not used in the events command
- `unsubscribe_from_actor()` - Will be needed for clean shutdown

#### Process Flow:
1. Parse the `--monitor` flag in the start command
2. Execute the normal start process to deploy the actor and get the actor ID
3. If `--monitor` is provided:
   - Subscribe to the actor's events using the existing `subscribe_to_actor()` method
   - Set up a continuous event listener to receive `ActorEvent` responses
   - Format and display events in real-time to stdout
   - Handle Ctrl+C to unsubscribe and exit gracefully

### 3. Event Reception Implementation
Unlike the current `events` command which gets a one-time snapshot, the monitoring functionality will:
1. Create a subscription to receive real-time events
2. Use a continuous loop to receive and process incoming messages
3. Watch for and handle `ManagementResponse::ActorEvent` type messages
4. Format events similar to the existing events command but in real-time
5. Include proper signal handling for clean termination

### 4. User Experience
When the `--monitor` flag is used:
1. User will see the normal output from starting an actor (actor ID, etc.)
2. The command will not exit, but instead display a message like "Monitoring events for actor [id]..."
3. As events occur, they will be displayed in real-time
4. User can press Ctrl+C to stop monitoring and exit
5. A proper shutdown sequence will unsubscribe from events before exiting

## Implementation Plan
1. Modify the CLI parser to accept the new flag for the start command
2. Implement a continuous event monitoring loop for the start command
3. Add signal handling for clean termination including unsubscribing
4. Test the subscription mechanism to ensure it's working properly
5. Update documentation and help text

## Impacts
- Improves developer experience by enabling real-time event observation
- Makes testing and debugging actors more efficient
- Maintains backward compatibility (the flag is optional)
- Leverages existing subscription functionality that isn't currently used in the CLI

## Alternatives Considered
- Creating a separate `monitor` command: Rejected in favor of an optional flag to keep the command structure simple
- Enhancing the existing `events` command with a `--watch` option: Considered but starting + monitoring is a more common workflow
- Implementing a WebSocket-based event viewer: May be considered for future enhancement but CLI is preferred for developer workflows