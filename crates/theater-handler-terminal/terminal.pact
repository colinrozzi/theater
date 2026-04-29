// Terminal Handler Interface
//
// Provides terminal I/O capabilities to WebAssembly actors.
// Enables building interactive CLI applications, REPLs, and TUI apps.

interface terminal {
    @package: string = "theater:simple"

    exports {
        // Write bytes to stdout
        // Returns the number of bytes written
        write-stdout: func(data: list<u8>) -> result<u64, string>

        // Write bytes to stderr
        // Returns the number of bytes written
        write-stderr: func(data: list<u8>) -> result<u64, string>

        // Enable or disable raw mode
        // Raw mode disables line buffering and echo, needed for TUI apps
        set-raw-mode: func(enabled: bool) -> result<_, string>

        // Get terminal size as (columns, rows)
        get-size: func() -> result<tuple<u16, u16>, string>

        // Enable input reading from stdin
        // This starts the background input loop that calls handle-input on the actor
        // Must be called explicitly by the actor to start receiving input
        enable-input: func() -> result<_, string>
    }
}
