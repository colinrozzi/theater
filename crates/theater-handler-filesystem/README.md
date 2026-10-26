# Theater FileSystem Handler

Sandboxed, permission-gated filesystem access for WebAssembly actors in the
Theater system. Implements the `theater:simple/filesystem` interface
(`filesystem.pact`).

## Functions

Every path is relative to the handler's sandbox root. Every function returns
`result<_, filesystem-error>` (or a value on success, for reads).

| Function | Needs | What it does |
|----------|-------|--------------|
| `read-file(path)` | read | The file's bytes |
| `exists(path)` | read | Whether the path exists |
| `list-dir(path)` | read | `list<dir-entry>` (name, is-dir) |
| `metadata(path)` | read | `file-metadata` (size, is-dir, read-only) |
| `write-file(path, content)` | write | Create or truncate, then write |
| `append-file(path, content)` | write | Append (create if absent) |
| `delete-file(path)` | write | Remove a file |
| `create-dir(path)` | write | `mkdir -p` |
| `remove-dir(path)` | write | Remove a directory and its contents |
| `sync-file(path)` | write | fsync a file's data and metadata |
| `sync-dir(path)` | write | fsync a directory, so a create, delete or rename in it is durable |
| `rename(from, to)` | write | Atomic rename; replaces a file at `to` |
| `write-file-durable(path, content)` | write | Temp file, sync, rename over `path`, sync the directory |

## Durability

`write-file` and `append-file` return once the bytes are in the kernel. They
survive the actor or the runtime dying, but not a kernel crash or power loss,
and a write cut off midway leaves a partial file.

To make a write durable, use `write-file-durable(path, content)`. Afterwards
`path` holds the new content, and a crash at any point leaves it with either
the old content or all of the new. It writes a hidden temp file
(`.<name>.tmp-<nanos>`) in the same directory, syncs it, renames it over
`path` and syncs the directory. If it fails before the rename, the temp file
is removed; a crash midway can leave it behind.

To make the same sequence yourself (for example, writing several files then
publishing them), use the primitives:

```
write-file("dir/.msg.tmp", bytes)
sync-file("dir/.msg.tmp")
rename("dir/.msg.tmp", "dir/msg")
sync-dir("dir")
```

After `append-file`, `sync-file` makes the appended bytes durable.

## Security model

- **Sandbox root.** A path that escapes the root via `..`, an absolute path,
  or a windows prefix is rejected as `invalid-path`. With no configured root
  (or `new_dir = true`) a fresh per-instance temp directory is used and
  removed when the actor stops.
- **Capabilities (default-deny).** Reads need the `read` permission; writes,
  including the durability functions, need `write`. Otherwise
  `permission-denied`.
- **allowed-paths.** When set, the path must fall within one of the listed
  subtrees (relative to the root; `"/"` means the whole sandbox), or
  `permission-denied`.
- **rename.** Both `from` and `to` must pass all of the above. In addition,
  the real location of each path's parent directory, with symlinks resolved,
  must be inside the sandbox and the allowed-paths, so a symlinked directory
  can't be used to move a file into or out of the sandbox. `write-file-durable`
  applies the same check to its path. The final component is not resolved:
  renaming a symlink moves the link, and renaming over a symlink replaces the
  link, never its target. The sandbox root itself can't be renamed or
  replaced.
- **Across filesystems.** If `from` and `to` are on different filesystems (a
  mount inside the sandbox), `rename` fails with `io-error` (EXDEV). It does
  not fall back to copying, which would not be atomic.

The other functions resolve paths by path arithmetic only; a symlink inside
the sandbox that points out is not caught for them.

## Chain events

Every call is a host call, recorded on the actor's chain as a
`theater:simple/filesystem/<function>` event with its input and output, and
replayed from the chain: on replay the recorded result is returned and the
disk is not touched.
