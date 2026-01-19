mod bindings;

struct Component;

impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(
        _state: Option<Vec<u8>>,
        _params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Successfully imported wasi:io/error, wasi:io/streams, and wasi:io/poll
        // The real testing will happen when we integrate with WASI HTTP
        // which will provide actual stream resources
        //
        // For now, we just verify that the imports are satisfied and
        // the bindings compile correctly.
        
        Ok((Some(b"WASI I/O imports successful!".to_vec()),))
    }
}

bindings::export!(Component with_types_in bindings);
