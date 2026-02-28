// Theater Loop Client Interface
//
// The loop body that actors export for the runtime to call.
//
// The runtime calls loop(state) repeatedly:
// - On success: enqueues another loop(new_state) call
// - On error: stops the loop and reports the error
//
// Each call is a separate message, so other operations can interleave.

interface loop-client {
    @package: string = "theater:simple"

    exports {
        // Called repeatedly by runtime
        // Return Ok(new_state) to continue, Err(msg) to stop
        loop: func(state: list<u8>) -> result<list<u8>, string>
    }
}
