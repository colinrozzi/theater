// Theater Store Interface
//
// Content-addressable storage for actors.
// Note: content-ref is represented as a string (the hash) for interface hashing simplicity.

interface store {
    @package: string = "theater:simple"

    exports {
        // Create a new store, returns store ID
        new: func() -> result<string, string>

        // Store content, returns content reference (hash string)
        store: func(store-id: string, content: list<u8>) -> result<string, string>

        // Retrieve content by reference
        get: func(store-id: string, content-ref: string) -> result<list<u8>, string>

        // Check if content exists
        exists: func(store-id: string, content-ref: string) -> result<bool, string>

        // Attach a label to content
        label: func(store-id: string, label: string, content-ref: string) -> result<_, string>

        // Get content reference by label
        get-by-label: func(store-id: string, label: string) -> result<option<string>, string>

        // Remove a label
        remove-label: func(store-id: string, label: string) -> result<_, string>

        // Store and label in one operation
        store-at-label: func(store-id: string, label: string, content: list<u8>) -> result<string, string>

        // Replace content at label
        replace-content-at-label: func(store-id: string, label: string, content: list<u8>) -> result<string, string>

        // Replace label to point to existing content
        replace-at-label: func(store-id: string, label: string, content-ref: string) -> result<_, string>

        // List all content references
        list-all-content: func(store-id: string) -> result<list<string>, string>

        // Calculate total size
        calculate-total-size: func(store-id: string) -> result<u64, string>

        // List all labels
        list-labels: func(store-id: string) -> result<list<string>, string>
    }
}
