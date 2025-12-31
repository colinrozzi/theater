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
        _params: (String,),
    ) -> Result<(Option<Vec<u8>>,), String> {
        // Test WASI HTTP by making a request to httpbin.org

        // Create headers
        let headers = types::Headers::new();
        headers.append(&"user-agent".to_string(), &b"wasi-http-test/0.1.0".to_vec());

        // Create outgoing request
        let request = outgoing_handler::OutgoingRequest::new(headers);

        // Configure the request
        request.set_method(&types::Method::Get);
        request.set_scheme(Some(&types::Scheme::Https));
        request.set_authority(Some("httpbin.org"));
        request.set_path_with_query(Some("/get"));

        // Make the request
        let future_response = outgoing_handler::handle(request, None)
            .map_err(|e| format!("Failed to initiate HTTP request: {:?}", e))?;

        // Wait for the response
        let response = future_response.get()
            .ok_or_else(|| "Failed to get response".to_string())?
            .map_err(|()| "Response future was cancelled".to_string())?
            .map_err(|e| format!("HTTP request failed: {:?}", e))?;

        // Get status code
        let status = response.status();

        // Get response headers
        let response_headers = response.headers();
        let content_type = response_headers.get(&"content-type".to_string());

        // Build response message
        let mut message = format!("HTTP GET https://httpbin.org/get\n");
        message.push_str(&format!("Status: {}\n", status));

        if !content_type.is_empty() {
            if let Ok(ct_str) = String::from_utf8(content_type[0].clone()) {
                message.push_str(&format!("Content-Type: {}\n", ct_str));
            }
        }

        message.push_str("\nWASI HTTP test successful!\n");

        Ok((Some(message.into_bytes()),))
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
