// Theater Runtime Interface
//
// Core runtime capabilities for actors: logging, chain access, shutdown.

interface runtime {
    @package: string = "theater:simple"

    exports {
        // Log a message to the actor's log stream
        log: func(msg: string)

        // Retrieve the actor's event chain
        // Note: Returns list<u8> for simplicity in the interface hash.
        // The actual implementation returns the structured chain record.
        get-chain: func() -> list<u8>

        // Shutdown the actor with optional final data
        shutdown: func(data: option<list<u8>>) -> result<_, string>
    }
}
