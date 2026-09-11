// Theater FileSystem Interface
//
// Sandboxed filesystem access for actors. Every path is resolved relative to the
// handler's configured sandbox root (FileSystemHandlerConfig.path); a path that
// escapes the root (via `..`, an absolute path, or a symlink out) is rejected as
// `invalid-path`. Reads require the `read` capability, writes require `write`
// (FileSystemPermissions); `allowed-paths`, when set, further restricts which
// subtrees are reachable.
//
// Replay contract (same as store): every op here is a host call, so its result is
// recorded on the chain and replayed deterministically — on replay a `read-file`
// returns the RECORDED bytes, never a fresh disk read. So filesystem I/O is a
// replayable projection of the chain, exactly like store-backed state.

interface filesystem {
    @package: string = "theater:simple"

    // One entry of a directory listing.
    record dir-entry {
        name: string,
        is-dir: bool,
    }

    // Metadata for a path.
    record file-metadata {
        size: u64,
        is-dir: bool,
        read-only: bool,
    }

    // Structured so callers can react (retry / create parent / report) instead of
    // substring-matching one opaque string.
    variant filesystem-error {
        not-found(string),          // the path does not exist
        permission-denied(string),  // read/write cap not granted, or path outside sandbox / allowed-paths
        already-exists(string),     // create target already exists
        not-a-directory(string),    // expected a directory, found a file
        is-a-directory(string),     // expected a file, found a directory
        invalid-path(string),       // traversal / absolute escape / non-utf8 / bad component
        io-error(string),           // any other host io failure (detail preserved)
    }

    exports {
        // --- reads (require `read`) ---
        read-file: func(path: string) -> result<list<u8>, filesystem-error>
        exists: func(path: string) -> result<bool, filesystem-error>
        list-dir: func(path: string) -> result<list<dir-entry>, filesystem-error>
        metadata: func(path: string) -> result<file-metadata, filesystem-error>

        // --- writes (require `write`) ---
        // Create-or-truncate then write the bytes.
        write-file: func(path: string, content: list<u8>) -> result<_, filesystem-error>
        // Append the bytes (create if absent).
        append-file: func(path: string, content: list<u8>) -> result<_, filesystem-error>
        delete-file: func(path: string) -> result<_, filesystem-error>
        // Create the directory and any missing parents (mkdir -p).
        create-dir: func(path: string) -> result<_, filesystem-error>
        // Remove the directory and its contents.
        remove-dir: func(path: string) -> result<_, filesystem-error>
    }
}
