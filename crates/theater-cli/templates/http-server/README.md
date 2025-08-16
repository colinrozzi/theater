# {{project_name}}

A basic HTTP server built with [Theater](https://github.com/colinrozzi/theater) WebAssembly actors.

## 🚀 Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (1.81.0 or newer)
- [Theater CLI](https://github.com/colinrozzi/theater) installed
- `cargo component` installed (`cargo install cargo-component`)

### Running the Server

```bash
# Build the WebAssembly component
cargo component build --release

# Start the server
theater start manifest.toml
```

The server will start on **http://localhost:8080**

### Available Endpoints

- `GET /` - Welcome page with server information
- `GET /health` - JSON health check response

## 🏗️ Project Structure

```
{{project_name}}/
├── src/
│   └── lib.rs              # HTTP server implementation
├── wit/
│   ├── world.wit          # WebAssembly Interface Types definitions
│   └── deps/              # Theater framework dependencies
├── Cargo.toml             # Rust dependencies
├── manifest.toml          # Theater actor configuration
└── README.md             # This file
```

## 🔧 Development

### Adding New Routes

To add new endpoints, modify the `handle_request` function in `src/lib.rs`:

```rust
let response = match (request.method.as_str(), request.uri.as_str()) {
    ("GET", "/") => generate_welcome_response(),
    ("GET", "/health") => generate_health_response(),
    ("GET", "/api/users") => generate_users_response(),  // Add new route
    _ => generate_404_response(),
};
```

Then implement your response function:

```rust
fn generate_users_response() -> HttpResponse {
    let json_body = r#"{"users":["alice","bob"]}"#;
    HttpResponse {
        status: 200,
        headers: vec![("Content-Type".to_string(), "application/json".to_string())],
        body: Some(json_body.as_bytes().to_vec()),
    }
}
```

### Building and Testing

```bash
# Build the component
cargo component build --release

# Start the server
theater start manifest.toml

# Test the endpoints
curl http://localhost:8080/
curl http://localhost:8080/health
```

### Monitoring the Actor

```bash
# List running actors
theater list

# Inspect the actor (get the actor ID from theater list)
theater inspect <actor-id>

# View actor events
theater events <actor-id>

# Stop the actor
theater stop <actor-id>
```

## 🎯 Next Steps

This template provides a foundation for building HTTP APIs and web applications. Consider adding:

- **Database integration** - Connect to databases using Theater's capabilities
- **Authentication** - Add middleware for API authentication
- **WebSocket support** - Enable real-time features
- **File serving** - Serve static files using Theater's filesystem handlers
- **Request validation** - Add input validation and error handling
- **Logging** - Enhanced request/response logging
- **CORS** - Cross-origin resource sharing support

## 📚 Learn More

- [Theater Documentation](https://github.com/colinrozzi/theater)
- [WebAssembly Component Model](https://github.com/WebAssembly/component-model)
- [WIT (WebAssembly Interface Types)](https://github.com/WebAssembly/wit-bindgen)

## 🤝 Contributing

Feel free to extend this template and share your improvements!

---

*Built with ❤️ using Theater WebAssembly actors*
