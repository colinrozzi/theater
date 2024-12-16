warning: unused import: `crate::chain_emitter::CHAIN_EMITTER`
 --> src/chain.rs:1:5
  |
1 | use crate::chain_emitter::CHAIN_EMITTER;
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: `#[warn(unused_imports)]` on by default

warning: unused import: `crate::logging::ChainEventType`
 --> src/chain.rs:2:5
  |
2 | use crate::logging::ChainEventType;
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused imports: `ActorInput` and `ActorOutput`
 --> src/chain.rs:3:13
  |
3 | use crate::{ActorInput, ActorOutput};
  |             ^^^^^^^^^^  ^^^^^^^^^^^

warning: unused import: `chrono::Utc`
 --> src/chain.rs:4:5
  |
4 | use chrono::Utc;
  |     ^^^^^^^^^^^

warning: unused import: `anyhow::Result`
 --> src/state.rs:1:5
  |
1 | use anyhow::Result;
  |     ^^^^^^^^^^^^^^

warning: unused variable: `new_state`
  --> src/state.rs:31:37
   |
31 |     pub fn verify_transition(&self, new_state: &Value) -> bool {
   |                                     ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_new_state`
   |
   = note: `#[warn(unused_variables)]` on by default

warning: field `name` is never read
  --> src/lib.rs:79:5
   |
74 | pub struct ActorProcess {
   |            ------------ field in this struct
...
79 |     name: String,
   |     ^^^^
   |
   = note: `#[warn(dead_code)]` on by default

warning: method `verify_transition` is never used
  --> src/state.rs:31:12
   |
11 | impl ActorState {
   | --------------- method in this implementation
...
31 |     pub fn verify_transition(&self, new_state: &Value) -> bool {
   |            ^^^^^^^^^^^^^^^^^

warning: `theater` (lib) generated 8 warnings (run `cargo fix --lib -p theater` to apply 5 suggestions)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.23s
     Running `target/debug/theater --manifest ../actors/llm-chat/browser-ui/actor.toml`
[2m2024-12-15T20:05:17.598566Z[0m [32m INFO[0m main ThreadId(01) [2msrc/store.rs[0m[2m:[0m[2m33[0m[2m:[0m [STORE] Initializing store with HTTP handler on port 8080 and HTTP server on port 8081
[2m2024-12-15T20:05:17.802761Z[0m [32m INFO[0m main ThreadId(01) [2msrc/capabilities.rs[0m[2m:[0m[2m157[0m[2m:[0m [WASM] Initializing browser UI actor
[2m2024-12-15T20:05:17.803108Z[0m [32m INFO[0m main ThreadId(01) [2msrc/main.rs[0m[2m:[0m[2m38[0m[2m:[0m Event server starting on port 3030
[2m2024-12-15T20:05:17.803126Z[0m [32m INFO[0m main ThreadId(01) [2msrc/main.rs[0m[2m:[0m[2m41[0m[2m:[0m Actor 'browser-ui' initialized successfully!
[2m2024-12-15T20:05:17.803141Z[0m [32m INFO[0m main ThreadId(01) [2msrc/main.rs[0m[2m:[0m[2m44[0m[2m:[0m Actor started at 2024-12-15 20:05:17.803139 UTC
[2m2024-12-15T20:05:17.803545Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(08) [2msrc/http.rs[0m[2m:[0m[2m114[0m[2m:[0m [HTTP] HTTP server started
Event server listening on port 3030
[2m2024-12-15T20:05:17.803946Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(06) [1mServer::run[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/server.rs[0m[2m:[0m[2m138[0m[2m:[0m listening on http://127.0.0.1:3030 [2m[3maddr[0m[2m=[0m127.0.0.1:3030[0m
[2m2024-12-15T20:05:17.803968Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(08) [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/tide-0.16.0/src/server.rs[0m[2m:[0m[2m212[0m[2m:[0m Server listening on http://127.0.0.1:8080    
[2m2024-12-15T20:05:17.803968Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(05) [2msrc/http_server.rs[0m[2m:[0m[2m118[0m[2m:[0m HTTP-SERVER starting on port 8081
[2m2024-12-15T20:05:21.327117Z[0m [32m INFO[0m                 main ThreadId(01) [2msrc/main.rs[0m[2m:[0m[2m47[0m[2m:[0m Shutting down...
