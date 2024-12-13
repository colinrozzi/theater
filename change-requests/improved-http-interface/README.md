# Enhanced HTTP Interface for Theater Actors

## Current State
The current HTTP interface requires manual JSON serialization and type conversion. HTTP requests and responses are handled through generic types and manual string conversions.

## Proposed Enhancement

### 1. Type-Safe Request/Response Handling
```rust
pub struct Request {
    method: Method,
    path: String,
    query_params: HashMap<String, String>,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

pub struct Response {
    status: u16,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    // ... other methods
}
```

### 2. Middleware Support
```rust
pub trait Middleware {
    async fn handle(&self, req: Request, next: Next) -> Response;
}

pub struct Router {
    routes: Vec<Route>,
    middleware: Vec<Box<dyn Middleware>>,
}
```

### 3. Routing DSL
```rust
impl Actor {
    fn setup_routes(&mut self, router: &mut Router) {
        router
            .get("/api/data", Self::handle_get_data)
            .post("/api/data", Self::handle_post_data)
            .middleware(logging::Logger::new())
            .middleware(auth::Authenticator::new());
    }
}
```

### 4. Content Type Helpers
```rust
impl Response {
    fn json<T: Serialize>(data: &T) -> Response { ... }
    fn text(content: &str) -> Response { ... }
    fn html(content: &str) -> Response { ... }
}

impl Request {
    async fn json<T: DeserializeOwned>(&self) -> Result<T> { ... }
    async fn text(&self) -> Result<String> { ... }
}
```

## Benefits
1. Type safety and better error handling
2. More intuitive API for developers
3. Built-in middleware support
4. Cleaner routing setup
5. Common response type helpers

## Implementation Notes
1. Keep backward compatibility
2. Introduce new types gradually
3. Add conversion traits for existing types
4. Document migration path

## Migration Strategy
1. Add new interfaces alongside existing ones
2. Provide migration guide
3. Add examples using new interface
4. Deprecate old interface in future major version