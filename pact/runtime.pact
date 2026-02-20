// Theater Runtime Interface
//
// Core runtime capabilities for actors: logging, chain access, shutdown.

interface runtime {
    @package: string = "theater:simple"

    use types.{chain}

    exports {
        // Log a message to the actor's log stream
        log: func(msg: string)

        // Retrieve the actor's event chain
        get-chain: func() -> chain

        // Shutdown the actor with optional final data
        shutdown: func(data: option<list<u8>>) -> result<_, string>
    }
}
