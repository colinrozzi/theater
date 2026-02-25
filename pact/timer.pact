// Timer Handler Interface
//
// Provides periodic tick callbacks for actors.
// Useful for game loops, polling, heartbeats, and scheduled tasks.

interface timer {
    @package: string = "theater:simple"

    exports {
        // Set a recurring interval timer
        // Returns timer ID on success
        set-interval: func(name: string, interval-ms: u64) -> result<string, string>

        // Clear a timer by name
        clear-interval: func(name: string) -> result<_, string>

        // Get current time in milliseconds since epoch
        now: func() -> u64
    }
}
