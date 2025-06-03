# HTTP Framework Handler

The HTTP Framework Handler enables actors to serve HTTP requests, turning them into fully-functional web services. It provides a bridge between incoming HTTP requests and actor functions, allowing actors to respond to web traffic while maintaining Theater's state verification model.

## Overview

The HTTP Framework Handler implements the `theater:simple/http-framework` interface, providing:

1. A way for actors to receive and respond to HTTP requests
2. Conversion between HTTP requests and actor-friendly formats
3. Automatic state chain recording of all HTTP interactions
4. Comprehensive error handling

## Configuration

To use the HTTP Framework Handler, add it to your actor's manifest:

```toml
[[handlers]]
type = "http-framework"
config = {}
```

The HTTP Framework Handler works in conjunction with the built-in HTTP server capability in Theater, which routes requests to the appropriate actors based on path configurations.

## Interface

The HTTP Framework Handler is defined using the following WIT interface:

```wit
interface http-framework {
    use types.{state};
    use http-types.{http-request, http-response};

    handle-request: func(state: state, req: http-request) -> result<tuple<state, http-response>, string>;
}
```

### HTTP Request Structure

The `HttpRequest` type has the following structure:

```rust
struct HttpRequest {
    method: String,
    uri: String,
    path: String,
    query: Option<String>,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
}
```

* `method`: The HTTP method (GET, POST, PUT, DELETE, etc.)
* `uri`: The full request URI
* `path`: The path component of the URI
* `query`: Optional query string
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

## Handling HTTP Requests

To handle HTTP requests, actors implement the `handle-request` function:

```rust
fn handle_request(state: Option<Vec<u8>>, req: HttpRequest) -> Result<(Option<Vec<u8>>, HttpResponse), String> {
    // Process the request and update state
    let new_state = process_request(&state, &req)?;
    
    // Generate a response
    let response = HttpResponse {
        status: 200,
        headers: vec![
            ("Content-Type".to_string(), "application/json".to_string()),
        ],
        body: Some(b"Hello, world!".to_vec()),
    };
    
    Ok((new_state, response))
}
```

## Routing

The HTTP Framework Handler maps incoming HTTP requests to actor functions based on the request path. This is configured through the Theater system's HTTP server configuration.

For example, to route all requests to `/api/users` to a specific actor:

```toml
# System configuration
[[http_routes]]
path = "/api/users"
actor_id = "user-service-actor"

# In the actor's manifest
[[handlers]]
type = "http-framework"
config = {}
```

## State Chain Integration

Every HTTP request and response is recorded in the actor's state chain, creating a verifiable history of all web interactions. The chain events include:

1. **HttpFrameworkRequestCall**: Records when a request is received, including:
   - HTTP method
   - Path
   - Headers count
   - Body size

2. **HttpFrameworkRequestResult**: Records the result of processing a request, including:
   - Status code
   - Headers count
   - Body size
   - Processing time

3. **Error**: Records any errors that occur during request processing, including:
   - Operation type
   - Path
   - Error message

## Error Handling

The HTTP Framework Handler provides two layers of error handling:

1. **Framework-Level Errors**: Handled by the framework itself, such as:
   - Routing errors
   - Method not allowed
   - Actor not found
   - Malformed requests

2. **Actor-Level Errors**: Returned by the actor's `handle-request` function, which can:
   - Return a custom error response
   - Provide detailed error information
   - Choose appropriate HTTP status codes

If an actor returns an error, the framework generates a 500 Internal Server Error response with the error message in the body (in development mode only).

## Security Considerations

When using the HTTP Framework Handler, consider the following security aspects:

1. **Input Validation**: Always validate and sanitize all HTTP request data
2. **Authentication**: Implement proper authentication for protected endpoints
3. **Rate Limiting**: Consider rate limiting to prevent abuse
4. **Error Information**: Be careful about exposing error details in production
5. **CORS Policies**: Implement appropriate CORS headers for browser security
6. **Content Security**: Set proper content security policies

## Implementation Details

Under the hood, the HTTP Framework Handler:

1. Receives HTTP requests from the Theater HTTP server
2. Converts them to the `HttpRequest` format
3. Retrieves the current actor state
4. Calls the actor's `handle-request` function
5. Updates the actor's state with the new state
6. Converts the `HttpResponse` back to an HTTP response
7. Records all operations in the state chain
8. Returns the response to the client

## Best Practices

1. **RESTful Design**: Follow RESTful principles for API design
2. **Stateless Design**: Keep HTTP handlers as stateless as possible
3. **Error Handling**: Implement proper error handling with appropriate status codes
4. **Content Types**: Set appropriate Content-Type headers
5. **Validation**: Validate all incoming data
6. **Testing**: Test all endpoints with various input scenarios
7. **Documentation**: Document your API endpoints clearly

## Examples

### Example 1: Simple JSON API

```rust
fn handle_request(state: Option<Vec<u8>>, req: HttpRequest) -> Result<(Option<Vec<u8>>, HttpResponse), String> {
    // Parse the current state or initialize it
    let current_state: AppState = match state {
        Some(data) => serde_json::from_slice(&data).map_err(|e| e.to_string())?,
        None => AppState::default(),
    };
    
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/items") => {
            // Return all items
            let items_json = serde_json::to_vec(&current_state.items).map_err(|e| e.to_string())?;
            Ok((
                state,
                HttpResponse {
                    status: 200,
                    headers: vec![
                        ("Content-Type".to_string(), "application/json".to_string()),
                    ],
                    body: Some(items_json),
                }
            ))
        },
        ("POST", "/api/items") => {
            // Add a new item
            if let Some(body) = req.body {
                let new_item: Item = serde_json::from_slice(&body).map_err(|e| e.to_string())?;
                
                // Update state
                let mut new_state = current_state.clone();
                new_state.items.push(new_item);
                
                // Serialize new state
                let new_state_bytes = serde_json::to_vec(&new_state).map_err(|e| e.to_string())?;
                
                // Return success response
                Ok((
                    Some(new_state_bytes),
                    HttpResponse {
                        status: 201,
                        headers: vec![
                            ("Content-Type".to_string(), "application/json".to_string()),
                        ],
                        body: Some(b"{\"status\":\"created\"}".to_vec()),
                    }
                ))
            } else {
                // Return error for missing body
                Ok((
                    state,
                    HttpResponse {
                        status: 400,
                        headers: vec![
                            ("Content-Type".to_string(), "application/json".to_string()),
                        ],
                        body: Some(b"{\"error\":\"Missing request body\"}".to_vec()),
                    }
                ))
            }
        },
        _ => {
            // Return 404 for unmatched routes
            Ok((
                state,
                HttpResponse {
                    status: 404,
                    headers: vec![
                        ("Content-Type".to_string(), "application/json".to_string()),
                    ],
                    body: Some(b"{\"error\":\"Not found\"}".to_vec()),
                }
            ))
        }
    }
}
```

### Example 2: File Serving

```rust
fn handle_request(state: Option<Vec<u8>>, req: HttpRequest) -> Result<(Option<Vec<u8>>, HttpResponse), String> {
    // Only handle GET requests
    if req.method != "GET" {
        return Ok((
            state,
            HttpResponse {
                status: 405,
                headers: vec![
                    ("Content-Type".to_string(), "text/plain".to_string()),
                    ("Allow".to_string(), "GET".to_string()),
                ],
                body: Some(b"Method Not Allowed".to_vec()),
            }
        ));
    }
    
    // Extract the requested file path
    let path = req.path.trim_start_matches('/');
    
    // Use the filesystem handler to read the file
    match filesystem::read_file(path) {
        Ok(file_content) => {
            // Determine content type based on file extension
            let content_type = match path.split('.').last() {
                Some("html") => "text/html",
                Some("css") => "text/css",
                Some("js") => "application/javascript",
                Some("json") => "application/json",
                Some("png") => "image/png",
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("svg") => "image/svg+xml",
                _ => "application/octet-stream",
            };
            
            // Return the file content
            Ok((
                state,
                HttpResponse {
                    status: 200,
                    headers: vec![
                        ("Content-Type".to_string(), content_type.to_string()),
                    ],
                    body: Some(file_content),
                }
            ))
        },
        Err(_) => {
            // File not found
            Ok((
                state,
                HttpResponse {
                    status: 404,
                    headers: vec![
                        ("Content-Type".to_string(), "text/plain".to_string()),
                    ],
                    body: Some(b"File Not Found".to_vec()),
                }
            ))
        }
    }
}
```

## Related Topics

- [HTTP Client Handler](http-client.md) - For making HTTP requests from actors
- [Message Server Handler](message-server.md) - For actor-to-actor communication
- [File System Handler](filesystem.md) - For accessing the file system
