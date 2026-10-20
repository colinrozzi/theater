// Theater Process Client Interface
//
// Actor exports for process events (see crates/theater-handler-process/
// process.pact). In-module state: each callback takes only its own arguments.

interface process-client {
    @package: string = "theater:simple"

    record process-status {
        running: bool,
        exit-code: option<s32>,
        signal: option<s32>,
    }

    exports {
        // A chunk of output, in active/once mode. `stream` is "stdout" or
        // "stderr" (PTY output is "stdout").
        on-output: func(process-id: string, stream: string, data: list<u8>) -> result<_, string>

        // The process exited, in active/once mode. Delivered after the last
        // on-output for that process.
        on-exit: func(process-id: string, status: process-status) -> result<_, string>

        // Called on the TARGET actor after a process is handed to it via
        // process.transfer.
        handle-process-transfer: func(process-id: string) -> result<_, string>
    }
}
