warning: unused variable: `target`
   --> src/lib.rs:166:36
    |
166 |     pub fn send_message(&mut self, target: &str, msg: Value) -> Result<()> {
    |                                    ^^^^^^ help: if this is intentional, prefix it with an underscore: `_target`
    |
    = note: `#[warn(unused_variables)]` on by default

warning: `theater` (lib) generated 1 warning
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.49s
     Running `target/debug/theater --manifest ../actors/llm-chat/browser-ui/actor.toml`
[2m2024-12-15T14:42:07.369080Z[0m [32m INFO[0m main ThreadId(01) [2msrc/store.rs[0m[2m:[0m[2m33[0m[2m:[0m [STORE] Initializing store with HTTP handler on port 8080 and HTTP server on port 8081
[2m2024-12-15T14:42:07.585087Z[0m [32m INFO[0m main ThreadId(01) [2msrc/capabilities.rs[0m[2m:[0m[2m157[0m[2m:[0m [WASM] Initializing browser UI actor

[CHAIN] Event at 2024-12-15T14:42:07.585460+00:00
----------------------------------
CHAIN COMMIT #d2d5fc59b68e323a8a21987e97e5ab1e
TIMESTAMP: 2024-12-15T14:42:07.585430+00:00
ACTOR: unknown
TYPE: Init
DATA:
{
  "event": {
    "StateChange": {
      "new_state": {
        "active_polls": {},
        "last_update": 0
      },
      "old_state": null,
      "timestamp": "2024-12-15T14:42:07.585236Z"
    }
  },
  "parent": null
}
----------------------------------

[2m2024-12-15T14:42:07.585571Z[0m [32m INFO[0m main ThreadId(01) [2msrc/main.rs[0m[2m:[0m[2m38[0m[2m:[0m Event server starting on port 3030
[2m2024-12-15T14:42:07.585586Z[0m [32m INFO[0m main ThreadId(01) [2msrc/main.rs[0m[2m:[0m[2m41[0m[2m:[0m Actor 'browser-ui' initialized successfully!
[2m2024-12-15T14:42:07.585598Z[0m [32m INFO[0m main ThreadId(01) [2msrc/main.rs[0m[2m:[0m[2m44[0m[2m:[0m Actor started at 2024-12-15 14:42:07.585597 UTC
[2m2024-12-15T14:42:07.585866Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(07) [2msrc/http.rs[0m[2m:[0m[2m114[0m[2m:[0m [HTTP] HTTP server started
Event server listening on port 3030
[2m2024-12-15T14:42:07.586312Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(07) [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/tide-0.16.0/src/server.rs[0m[2m:[0m[2m212[0m[2m:[0m Server listening on http://127.0.0.1:8080    
[2m2024-12-15T14:42:07.586311Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(08) [2msrc/http_server.rs[0m[2m:[0m[2m118[0m[2m:[0m HTTP-SERVER starting on port 8081
[2m2024-12-15T14:42:07.586351Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(06) [1mServer::run[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/server.rs[0m[2m:[0m[2m138[0m[2m:[0m listening on http://127.0.0.1:3030 [2m[3maddr[0m[2m=[0m127.0.0.1:3030[0m
[2m2024-12-15T14:42:08.143600Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(04) [1mrequest[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/filters/trace.rs[0m[2m:[0m[2m301[0m[2m:[0m processing request [2m[3mmethod[0m[2m=[0mGET [3mpath[0m[2m=[0m/events/ws [3mversion[0m[2m=[0mHTTP/1.1 [3mremote.addr[0m[2m=[0m127.0.0.1:54655[0m
[2m2024-12-15T14:42:08.144384Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(04) [1mrequest[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/filters/trace.rs[0m[2m:[0m[2m268[0m[2m:[0m finished processing with status [3mstatus[0m[2m=[0m101 [3merror[0m[2m=[0mNone [2m[3mmethod[0m[2m=[0mGET [3mpath[0m[2m=[0m/events/ws [3mversion[0m[2m=[0mHTTP/1.1 [3mremote.addr[0m[2m=[0m127.0.0.1:54655[0m
[2m2024-12-15T14:42:08.929705Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(04) [1mrequest[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/filters/trace.rs[0m[2m:[0m[2m301[0m[2m:[0m processing request [2m[3mmethod[0m[2m=[0mGET [3mpath[0m[2m=[0m/events/history [3mversion[0m[2m=[0mHTTP/1.1 [3mremote.addr[0m[2m=[0m127.0.0.1:54659 [3mreferer[0m[2m=[0mhttp://localhost:3000/[0m
[2m2024-12-15T14:42:08.930011Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(04) [1mrequest[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/filters/trace.rs[0m[2m:[0m[2m247[0m[2m:[0m finished processing with success [3mstatus[0m[2m=[0m200 [2m[3mmethod[0m[2m=[0mGET [3mpath[0m[2m=[0m/events/history [3mversion[0m[2m=[0mHTTP/1.1 [3mremote.addr[0m[2m=[0m127.0.0.1:54659 [3mreferer[0m[2m=[0mhttp://localhost:3000/[0m
[2m2024-12-15T14:42:08.931540Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(07) [1mrequest[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/filters/trace.rs[0m[2m:[0m[2m301[0m[2m:[0m processing request [2m[3mmethod[0m[2m=[0mGET [3mpath[0m[2m=[0m/events/history [3mversion[0m[2m=[0mHTTP/1.1 [3mremote.addr[0m[2m=[0m127.0.0.1:54668 [3mreferer[0m[2m=[0mhttp://localhost:3000/[0m
[2m2024-12-15T14:42:08.931693Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(07) [1mrequest[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/filters/trace.rs[0m[2m:[0m[2m247[0m[2m:[0m finished processing with success [3mstatus[0m[2m=[0m200 [2m[3mmethod[0m[2m=[0mGET [3mpath[0m[2m=[0m/events/history [3mversion[0m[2m=[0mHTTP/1.1 [3mremote.addr[0m[2m=[0m127.0.0.1:54668 [3mreferer[0m[2m=[0mhttp://localhost:3000/[0m
[2m2024-12-15T14:42:08.931972Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(07) [1mrequest[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/filters/trace.rs[0m[2m:[0m[2m301[0m[2m:[0m processing request [2m[3mmethod[0m[2m=[0mGET [3mpath[0m[2m=[0m/events/ws [3mversion[0m[2m=[0mHTTP/1.1 [3mremote.addr[0m[2m=[0m127.0.0.1:54669[0m
[2m2024-12-15T14:42:08.932172Z[0m [32m INFO[0m tokio-runtime-worker ThreadId(07) [1mrequest[0m: [2m/Users/colinrozzi/.cargo/registry/src/index.crates.io-6f17d22bba15001f/warp-0.3.7/src/filters/trace.rs[0m[2m:[0m[2m268[0m[2m:[0m finished processing with status [3mstatus[0m[2m=[0m101 [3merror[0m[2m=[0mNone [2m[3mmethod[0m[2m=[0mGET [3mpath[0m[2m=[0m/events/ws [3mversion[0m[2m=[0mHTTP/1.1 [3mremote.addr[0m[2m=[0m127.0.0.1:54669[0m
