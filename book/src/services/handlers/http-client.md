# HTTP Client Handler

The HTTP Client Handler enables actors to make HTTP requests to external services while maintaining Theater's state verification and security principles. This handler allows actors to interact with external APIs, fetch resources, and communicate with web services.

## Overview

The HTTP Client Handler implements the `ntwk:theater/http-client` interface, providing a way for actors to:

1. Send HTTP requests to external services
2. Process HTTP responses
3. Record all HTTP interactions in the state chain
4. Handle errors in a consistent way

## Configuration

To use the HTTP Client Handler, add it to your actor's manifest:

```toml
[[handlers]]
type = "http-client"
config = {}
```

Currently, the HTTP Client Handler doesn't require any specific configuration parameters.

## Interface

The HTTP Client Handler is defined using the following WIT interface:

```wit
interface http-client {
    use types.{json};
    use http-types.{http-request, http-response};

    send-http: func(req: http-request) -> result<http-response, string>;
}
```

### HTTP Request Structure

The `HttpRequest` type has the following structure:

```rust
struct HttpRequest {
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}
```

* `method`: The HTTP method (GET, POST, PUT, DELETE, etc.)
* `uri`: The target URL
* `headers`: A list of HTTP headers as key-value pairs
* `body`: Optional request body as bytes

### HTTP Response Structure

The `HttpResponse` type has the following structure:

```rust
struct HttpResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}
```

* `status`: The HTTP status code
* `headers`: A list of response headers as key-value pairs
* `body`: Optional response body as bytes

## Making HTTP Requests

To make an HTTP request, actors call the `send-http` function with an `HttpRequest` object:

```rust
let request = HttpRequest {
    method: "GET".to_string(),
    uri: "https://api.example.com/data".to_string(),
    headers: vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Authorization".to_string(), "Bearer token123".to_string()),
    ],
    body: None,
};

match http_client::send_http(request) {
    Ok(response) => {
        // Process response
        println!("Status: {}", response.status);
        if let Some(body) = response.body {
            // Handle response body
        }
    },
    Err(error) => {
        // Handle error
        println!("Request failed: {}", error);
    }
}
```

## State Chain Integration

Every HTTP request and response is recorded in the actor's state chain, creating a verifiable history of all external interactions. The chain events include:

1. **HttpClientRequestCall**: Records when a request is made, including:
   - HTTP method
   - Target URL
   - Headers count
   - Body size

2. **HttpClientRequestResult**: Records the result of a request, including:
   - Status code
   - Headers count
   - Body size
   - Success indicator

3. **Error**: Records any errors that occur during the request, including:
   - Operation type
   - URL path
   - Error message

This state chain integration ensures that all external interactions are:
- Traceable
- Verifiable
- Reproducible
- Auditable

## Error Handling

The HTTP Client Handler provides detailed error information for various failure scenarios:

1. **Invalid Method**: When an invalid HTTP method is specified
2. **Network Errors**: When network issues prevent the request from completing
3. **Timeout Errors**: When the request times out
4. **Parser Errors**: When response parsing fails

All errors are returned as strings and are also recorded in the state chain.

## Security Considerations

When using the HTTP Client Handler, consider the following security aspects:

1. **URL Validation**: Validate URLs before making requests to prevent SSRF attacks
2. **Sensitive Data**: Be careful with sensitive data in requests, as they are recorded in the state chain
3. **Authentication**: Use secure methods for authentication in external APIs
4. **TLS Verification**: The handler performs TLS verification by default
5. **Timeouts**: Set appropriate timeouts for requests to prevent resource exhaustion

## Implementation Details

Under the hood, the HTTP Client Handler:

1. Converts the `HttpRequest` into a reqwest client request
2. Sets up headers, body, and method
3. Executes the request asynchronously
4. Processes the response into an `HttpResponse`
5. Records all operations in the state chain
6. Returns the response or error to the actor

The handler uses the reqwest crate for HTTP functionality, providing a robust and well-tested HTTP client implementation.

## Limitations

The current HTTP Client Handler implementation has some limitations:

1. **No Direct Streaming**: Large responses are loaded fully into memory
2. **No WebSocket Support**: For WebSocket connections, use a dedicated WebSocket client
3. **No Client Certificate Authentication**: TLS client certificates are not currently supported
4. **No Direct Proxy Configuration**: Proxy settings cannot be configured per-request

## Best Practices

1. **Error Handling**: Always handle errors from HTTP requests properly
2. **Response Size**: Be mindful of response sizes to avoid memory issues
3. **Request Rate**: Implement rate limiting for external API calls
4. **Timeout Handling**: Set appropriate timeouts for your use case
5. **Idempotency**: Design requests to be idempotent when possible
6. **Retries**: Implement retry logic for transient failures

## Examples

### Example 1: Simple GET Request

```rust
pub fn fetch_json_data() -> Result<serde_json::Value, String> {
    let request = HttpRequest {
        method: "GET".to_string(),
        uri: "https://api.example.com/data.json".to_string(),
        headers: vec![("Accept".to_string(), "application/json".to_string())],
        body: None,
    };
    
    let response = http_client::send_http(request)?;
    
    if response.status != 200 {
        return Err(format!("API returned status code: {}", response.status));
    }
    
    if let Some(body) = response.body {
        let json = serde_json::from_slice(&body)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
        Ok(json)
    } else {
        Err("Response body was empty".to_string())
    }
}
```

### Example 2: POST Request with JSON Body

```rust
pub fn create_resource(data: &CreateResourceRequest) -> Result<ResourceResponse, String> {
    let json_body = serde_json::to_vec(data)
        .map_err(|e| format!("Failed to serialize request: {}", e))?;
    
    let request = HttpRequest {
        method: "POST".to_string(),
        uri: "https://api.example.com/resources".to_string(),
        headers: vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Authorization".to_string(), format!("Bearer {}", get_token())),
        ],
        body: Some(json_body),
    };
    
    let response = http_client::send_http(request)?;
    
    match response.status {
        201 => {
            // Resource created successfully
            if let Some(body) = response.body {
                let resource: ResourceResponse = serde_json::from_slice(&body)
                    .map_err(|e| format!("Failed to parse response: {}", e))?;
                Ok(resource)
            } else {
                Err("Response body was empty".to_string())
            }
        },
        400..=499 => {
            // Client error
            Err(format!("Client error: {}", response.status))
        },
        500..=599 => {
            // Server error
            Err(format!("Server error: {}", response.status))
        },
        _ => {
            // Unexpected status code
            Err(format!("Unexpected status code: {}", response.status))
        }
    }
}
```

### Example 3: File Download

```rust
pub fn download_file(url: &str) -> Result<Vec<u8>, String> {
    let request = HttpRequest {
        method: "GET".to_string(),
        uri: url.to_string(),
        headers: vec![],
        body: None,
    };
    
    let response = http_client::send_http(request)?;
    
    if response.status != 200 {
        return Err(format!("Download failed with status: {}", response.status));
    }
    
    if let Some(body) = response.body {
        Ok(body)
    } else {
        Err("Download resulted in empty file".to_string())
    }
}
```

## Related Topics

- [HTTP Framework Handler](http-framework.md) - For creating HTTP servers
- [Store Handler](store.md) - For storing downloaded content
- [Message Server Handler](message-server.md) - For actor-to-actor communication
