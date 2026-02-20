// Theater namespace - demonstrates nested interfaces

interface theater {
    @version: string = "0.1.0"

    // Nested interface for runtime functions
    interface runtime {
        exports {
            get-actor-id: func() -> string
            spawn: func(name: string, wasm: list<u8>) -> string
            send: func(target: string, msg: list<u8>) -> result<_, string>
        }
    }

    // Nested interface for state management
    interface state {
        exports {
            get: func(key: string) -> option<list<u8>>
            set: func(key: string, value: list<u8>)
        }
    }

    // Nested interface for capabilities
    interface capabilities {
        flags permissions {
            read,
            write,
            spawn,
            network,
        }

        exports {
            has: func(perm: permissions) -> bool
            request: func(perm: permissions) -> result<_, string>
        }
    }
}
