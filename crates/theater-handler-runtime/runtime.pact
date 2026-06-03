// Theater Runtime Interface
//
// Core runtime capabilities for actors: logging and shutdown.

interface runtime {
    @package: string = "theater:simple"

    exports {
        // Log a message to the actor's log stream
        log: func(msg: string)

        // Shutdown the actor with optional final data
        shutdown: func(data: option<list<u8>>) -> result<_, string>
    }
}
