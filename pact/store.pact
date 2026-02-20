// Theater Store Interface
//
// Content-addressable storage for actors.

interface store {
    @package: string = "theater:simple"

    // Reference to stored content (hash-based)
    record content-ref {
        hash: string,
    }

    exports {
        // Create a new store, returns store ID
        new: func() -> result<string, string>

        // Store content, returns content reference
        store: func(store-id: string, content: list<u8>) -> result<content-ref, string>

        // Retrieve content by reference
        get: func(store-id: string, content-ref: content-ref) -> result<list<u8>, string>

        // Check if content exists
        exists: func(store-id: string, content-ref: content-ref) -> result<bool, string>

        // Attach a label to content
        label: func(store-id: string, label: string, content-ref: content-ref) -> result<_, string>

        // Get content reference by label
        get-by-label: func(store-id: string, label: string) -> result<option<content-ref>, string>

        // Remove a label
        remove-label: func(store-id: string, label: string) -> result<_, string>

        // Remove content reference from label
        remove-from-label: func(store-id: string, label: string, content-ref: content-ref) -> result<_, string>

        // Store and label in one operation
        store-at-label: func(store-id: string, label: string, content: list<u8>) -> result<content-ref, string>

        // Replace content at label
        replace-content-at-label: func(store-id: string, label: string, content: list<u8>) -> result<content-ref, string>

        // Replace label to point to existing content
        replace-at-label: func(store-id: string, label: string, content-ref: content-ref) -> result<_, string>

        // List all labels
        list-labels: func(store-id: string) -> result<list<string>, string>

        // List all content references
        list-all-content: func(store-id: string) -> result<list<content-ref>, string>

        // Calculate total size
        calculate-total-size: func(store-id: string) -> result<u64, string>
    }
}
