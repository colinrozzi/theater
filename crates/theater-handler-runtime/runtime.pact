// Theater Runtime Interface
//
// Core runtime capabilities for actors: logging and shutdown.

interface runtime {
    @package: string = "theater:simple"

    exports {
        // Log a message to the actor's log stream
        log: func(msg: string)

        // Return this actor's own id as a string
        self: func() -> string

        // Shutdown the actor with optional final data
        shutdown: func(data: option<list<u8>>) -> result<_, string>
    }
}
