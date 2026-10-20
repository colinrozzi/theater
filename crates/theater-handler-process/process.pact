// Theater Process Interface
//
// Spawn and manage native OS processes. This is a RAW primitive: it runs a
// program and gives the actor its stdio, a PTY, signals, and an exit status —
// nothing more. It is DELIBERATELY NOT a sandbox: a spawned process is an
// ordinary OS process with the host user's authority (cwd is just a starting
// directory, not a jail). Isolation comes from WHAT you spawn — bwrap,
// `podman exec`, a VM, ssh — never from this handler.
//
// Process lifecycle:
//   1. spawn(spec) -> process-id. The process is owned by the calling actor.
//   2. Interact: write-stdin / read (passive) or set-active (pushed output),
//      resize (PTY), signal, kill.
//   3. Observe the end: wait() / status(), or the on-exit callback.
//   4. close(process-id) forgets it (killing it first if still running).
// When the owning actor stops, every process it still owns is terminated
// (SIGTERM to the process group, then SIGKILL after the configured grace).
//
// For the common "run a command, give me the result" case, run() does
// spawn + feed stdin + collect output + wait in a single call.
//
// Output model: each process has ONE ordered output stream of chunks, each
// tagged "stdout" or "stderr" (a PTY has a single stream, reported as
// "stdout"). Output is never dropped: when the actor doesn't keep up, the
// bounded buffer fills and the child blocks on write, exactly like a pipe.
// Output modes (mirrors the tcp/udp handlers):
//   - "passive" (default): pull chunks with read().
//   - "active": each chunk pushed to on-output; after the last chunk, on-exit.
//   - "once": one callback (a chunk, or on-exit if output is done), then back
//     to passive.
// A sequential actor must not block in wait() while its process is in active
// mode with unread output — the child can't exit while its pipe is full.
//
// Callback interface the ACTOR implements (export side,
// theater:simple/process-client — see pact/process-client.pact):
//   on-output: func(process-id: string, stream: string, data: list<u8>) -> result<_, string>
//   on-exit: func(process-id: string, status: process-status) -> result<_, string>
//   handle-process-transfer: func(process-id: string) -> result<_, string>
//
// Permissions (manifest [[handler]] type = "process" config): allowed
// programs, max processes, env inheritance, kill grace, and the run() output
// cap. allowed-programs is a POLICY, not a wall — `bash -c` can launch anything
// bash can.

interface process {
    @package: string = "theater:simple"

    // Everything needed to start a process.
    record spawn-spec {
        // Program to run. A bare name is looked up on PATH.
        program: string,
        args: list<string>,
        // Working directory. none = the runtime's own cwd.
        cwd: option<string>,
        // Extra environment variables (added to / overriding the inherited env).
        env: list<tuple<string, string>>,
        // Start from an empty environment instead of inheriting the host's.
        clear-env: bool,
        // Run on a pseudo-terminal of (cols, rows). The PTY carries stdin and
        // a single combined output stream; stdin/stdout/stderr below are then
        // ignored. TERM defaults to xterm-256color unless set in env.
        pty: option<tuple<u16, u16>>,
        // Per-stream disposition for non-PTY processes: "pipe" (default when
        // empty) or "null".
        stdin: string,
        stdout: string,
        stderr: string,
    }

    record process-status {
        running: bool,
        // Exit code, when the process exited normally.
        exit-code: option<s32>,
        // Terminating signal, when the process was killed by one.
        signal: option<s32>,
    }

    // One piece of output.
    record output-chunk {
        // "stdout" or "stderr" (PTY output is "stdout").
        stream: string,
        data: list<u8>,
    }

    // Result of run().
    record run-output {
        status: process-status,
        stdout: list<u8>,
        stderr: list<u8>,
        // True if output beyond the handler's cap was discarded.
        truncated: bool,
    }

    exports {
        // Start a process. Returns its process-id (opaque, actor-scoped).
        spawn: func(spec: spawn-spec) -> result<string, string>

        // Spawn, write `input` to stdin and close it, collect all output, and
        // wait for exit. On timeout the process group is killed and an error
        // returned. Not registered in the process table (no process-id).
        run: func(spec: spawn-spec, input: list<u8>, timeout-ms: option<u64>) -> result<run-output, string>

        // Write to stdin (or the PTY). Returns the number of bytes written.
        write-stdin: func(process-id: string, data: list<u8>) -> result<u64, string>

        // Close stdin (EOF). On a PTY this sends the EOF character (^D).
        close-stdin: func(process-id: string) -> result<_, string>

        // PASSIVE read: the next output chunk, at most max-bytes (a larger
        // chunk is split). Blocks until output arrives, or until timeout-ms
        // (error "timeout: ..."). none = all output has been consumed.
        read: func(process-id: string, max-bytes: u32, timeout-ms: option<u64>) -> result<option<output-chunk>, string>

        // Set output mode: "passive", "active", or "once".
        set-active: func(process-id: string, mode: string) -> result<_, string>

        // Resize the PTY.
        resize: func(process-id: string, cols: u16, rows: u16) -> result<_, string>

        // Send a signal (e.g. 2 = SIGINT, 15 = SIGTERM) to the process group.
        signal: func(process-id: string, sig: s32) -> result<_, string>

        // SIGKILL the process group.
        kill: func(process-id: string) -> result<_, string>

        status: func(process-id: string) -> result<process-status, string>

        // Block until the process exits (or timeout-ms: error "timeout: ...").
        wait: func(process-id: string, timeout-ms: option<u64>) -> result<process-status, string>

        // The OS pid.
        os-pid: func(process-id: string) -> result<u32, string>

        // process-ids owned by this actor.
        list: func() -> result<list<string>, string>

        // Forget a process, SIGKILLing its group first if still running.
        close: func(process-id: string) -> result<_, string>

        // Hand ownership to another actor (its handle-process-transfer is
        // called). The process continues uninterrupted; its output mode resets
        // to passive so the new owner opts in to delivery.
        transfer: func(process-id: string, target-actor: string) -> result<_, string>
    }
}
