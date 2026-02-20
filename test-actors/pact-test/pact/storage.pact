// Storage interface - demonstrates generics

interface storage {
    @version: string = "1.0.0"

    type T: Serializable

    exports {
        get: func(key: string) -> option<T>
        set: func(key: string, value: T)
        delete: func(key: string) -> bool
        keys: func() -> list<string>
    }
}
