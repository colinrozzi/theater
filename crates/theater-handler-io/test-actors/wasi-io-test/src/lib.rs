wit_bindgen::generate!({
    world: "wasi-io-test",
});

struct Component;

impl Guest for Component {
    fn init() -> Result<Vec<u8>, String> {
        // Successfully imported wasi:io/error and wasi:io/streams
        // The real testing will happen when we integrate with WASI HTTP
        // which will provide actual stream resources

        Ok(b"WASI I/O imports successful!".to_vec())
    }

    fn handle(_msg: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(vec![])
    }
}

export!(Component);
