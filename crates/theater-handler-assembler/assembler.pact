// Wisp Assembler Interface
//
// WAT to WASM assembly capabilities.

interface runtime {
    @package: string = "wisp:assembler"

    exports {
        // Convert WebAssembly Text format to binary WASM
        wat-to-wasm: func(wat: string) -> result<list<u8>, string>
    }
}
