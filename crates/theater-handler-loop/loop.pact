// Theater Loop Interface
//
// Cooperative looping for actors. Instead of blocking in a tight loop,
// actors yield after each iteration, allowing the runtime to:
// - Record state transitions to the chain
// - Process other messages (RPC, timers, etc.)
// - Schedule other actors fairly
//
// Usage:
//   1. Actor calls start-loop(initial_state)
//   2. Runtime begins calling actor's loop export repeatedly
//   3. Each loop(state) -> state transition is a chain event
//   4. Actor calls stop-loop() when done

interface loop {
    @package: string = "theater:simple"

    exports {
        // Start the cooperative loop with initial state
        // Runtime will begin calling the actor's loop-client.loop export
        start-loop: func(initial-state: list<u8>) -> result<_, string>

        // Stop the loop after the current iteration completes
        stop-loop: func() -> result<_, string>
    }
}
