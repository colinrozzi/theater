mod bindings;

use bindings::wasi::http::outgoing_handler;
use bindings::wasi::http::types::{
    self, Headers, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

struct Component;

// Implement the theater actor interface (for init)
impl bindings::exports::theater::simple::actor::Guest for Component {
    fn init(
        _state: Option<Vec<u8>>,
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Simple init - just return success with a message
        let message = "WASI HTTP test actor initialized successfully!\n";
        Ok((Some(message.as_bytes().to_vec()),))
    }
}

// Implement the incoming HTTP handler
impl bindings::exports::wasi::http::incoming_handler::Guest for Component {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        // Get request information
        let method = format!("{:?}", request.method());
        let path = request.path_with_query().unwrap_or_default();
        let authority = request.authority().unwrap_or_default();

        // Get request headers
        let req_headers = request.headers();
        let host_header = req_headers.get(&"host".to_string());
        let host = if !host_header.is_empty() {
            String::from_utf8(host_header[0].clone()).unwrap_or_default()
        } else {
            authority.clone()
        };

        // Read request body if present
        let body_content = if let Ok(incoming_body) = request.consume() {
            if let Ok(stream) = incoming_body.stream() {
                let mut body = Vec::new();
                loop {
                    match stream.read(4096) {
                        Ok(chunk) => {
                            if chunk.is_empty() {
                                break;
                            }
                            body.extend(chunk);
                        }
                        Err(_) => break,
                    }
                }
                String::from_utf8(body).unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Build a JSON response
        let response_body = format!(
            r#"{{
  "message": "Hello from WASI HTTP incoming handler!",
  "request": {{
    "method": "{}",
    "path": "{}",
    "host": "{}",
    "body_length": {}
  }}
}}"#,
            method,
            path,
            host,
            body_content.len()
        );

        // Create response headers
        let response_headers = Headers::new();
        response_headers.append(&"content-type".to_string(), &b"application/json".to_vec());
        response_headers.append(
            &"x-powered-by".to_string(),
            &b"Theater WASI HTTP".to_vec(),
        );

        // Create outgoing response
        let outgoing_response = OutgoingResponse::new(response_headers);
        outgoing_response.set_status_code(200).unwrap();

        // Get the outgoing body and write to it
        let outgoing_body = outgoing_response.body().unwrap();
        let output_stream = outgoing_body.write().unwrap();

        // Write response body
        let body_bytes = response_body.as_bytes();
        output_stream.blocking_write_and_flush(body_bytes).unwrap();
        drop(output_stream);

        // Finish the body
        OutgoingBody::finish(outgoing_body, None).unwrap();

        // Send the response via the outparam
        ResponseOutparam::set(response_out, Ok(outgoing_response));
    }
}

bindings::export!(Component with_types_in bindings);
