# String Manifests

## Description

Right now, our actor starting system is built around manifest paths. This is unnecessarily restrictive and is causing problems, as I want to be able to create and reference a lot of these manifests, it's getting hard to manage all of them. Also, I want to be able to start actors from other actors, and having to go through the filesystem adds a layer of complexity that I don't want to deal with.

So, I would like to change the actor system to accept manifest strings instead of paths. This will allow me to create and reference manifests in code, and will make it easier to start actors from other actors.

## Problem

The current design forces all actor manifests to exist as files on the filesystem. This approach has several limitations:

1. **File Management Complexity**: As the number of manifests grows, managing them as individual files becomes difficult.
2. **Runtime Creation Restrictions**: Actors cannot be created dynamically at runtime without creating temporary files.
3. **Cross-Actor Creation**: When one actor needs to create other actors, it must have filesystem access and coordinate file paths.
4. **Testing Challenges**: Testing requires setting up and tearing down manifest files.
5. **Portability Issues**: The system is less portable since it depends on specific filesystem paths.
6. **Initial State Limitations**: The current approach also restricts the actor's initial state to be loaded from a file, preventing dynamic state generation.

## Proposed Solution

We should modify the system to accept manifest content as strings, in addition to the existing file path approach. We should also enhance the initial state handling to support multiple sources. This will involve:

1. Extending `ManifestConfig` to support creation from string content
2. Updating the actor creation APIs to accept either a path or a manifest string
3. Enhancing initial state handling to support inline JSON, paths, and potentially remote sources
4. Adding CLI support for viewing actor manifests
5. Preserving backward compatibility with the path-based approach

## Implementation Details

1. **Modify `ManifestConfig` class**:
   - Add a new static method `from_string(content: &str) -> Result<Self>` in `config.rs`
   - Update the existing `from_file` method to use the new `from_string` method internally

2. **Create new enums for manifest and state sources**:
   ```rust
   pub enum ManifestSource {
       Path(PathBuf),
       Content(String),
   }

   pub enum InitialStateSource {
       Path(PathBuf),
       Json(String),
       Remote(String), // For future use with URLs
   }
   ```

3. **Update the `ManifestConfig` struct**:
   ```rust
   pub struct ManifestConfig {
       // ... existing fields
       #[serde(default)]
       pub init_state: Option<InitialStateSource>,
       // ... other fields
   }
   ```

4. **Update `TheaterCommand` enum**:
   - Modify the `SpawnActor` variant to handle either path or content:
   ```rust
   SpawnActor {
       manifest: ManifestSource,
       parent_id: Option<TheaterId>,
       response_tx: oneshot::Sender<Result<TheaterId>>,
   }
   ```

5. **Update `TheaterRuntime::spawn_actor` method**:
   - Modify it to accept `ManifestSource` instead of `PathBuf`
   - Handle both path and content cases

6. **Update `load_init_state` method**:
   ```rust
   pub fn load_init_state(&self) -> anyhow::Result<Option<Vec<u8>>> {
       match &self.init_state {
           Some(InitialStateSource::Path(path)) => {
               let data = std::fs::read(path)?;
               Ok(Some(data))
           }
           Some(InitialStateSource::Json(json_str)) => {
               // Validate the JSON string is proper JSON
               serde_json::from_str::<serde_json::Value>(json_str)?;
               Ok(Some(json_str.as_bytes().to_vec()))
           }
           Some(InitialStateSource::Remote(url)) => {
               // Placeholder for future implementation
               Err(anyhow::anyhow!("Remote state sources not yet implemented"))
           }
           None => Ok(None),
       }
   }
   ```

7. **Update `ManagementCommand` enum**:
   ```rust
   pub enum ManagementCommand {
       // ... existing variants
       StartActorFromString {
           manifest: String,
       },
       GetActorManifest {
           id: TheaterId,
       },
       // ... other variants
   }
   ```

8. **Add `ManagementResponse` variant**:
   ```rust
   pub enum ManagementResponse {
       // ... existing variants
       ActorManifest {
           id: TheaterId,
           manifest: String,
       },
       // ... other variants
   }
   ```

9. **Update the CLI interface**:
   - Add new commands to view actor manifests
   - Add options to specify initial state as inline JSON
   ```rust
   /// View the manifest of a running actor
   View {
       /// The ID of the actor to view
       actor_id: String,
   },
   ```

10. **Store the original manifest**:
    - Enhance the `ActorProcess` struct to store the original manifest content
    ```rust
    pub struct ActorProcess {
        // ... existing fields
        pub manifest_content: String,
        // ... other fields
    }
    ```

## Benefits

1. **Simplified Actor Creation**: Actors can create other actors without filesystem operations
2. **Improved Testing**: Tests can create actors with in-memory manifests
3. **Dynamic Configuration**: Manifests can be generated or modified at runtime
4. **Reduced Filesystem Dependency**: Less reliance on managing files
5. **Enhanced Portability**: System becomes more portable across different environments
6. **Flexible Initial State**: Support for inline JSON enables dynamic state initialization
7. **Better Observability**: CLI support for viewing manifests improves debugging capabilities

## Backward Compatibility

This change will maintain backward compatibility:
- All existing file path-based APIs will continue to work
- Systems using the current approach won't need immediate changes
- Existing manifest files will be parsed correctly

## Implementation Plan

1. Create the source enums and update the `ManifestConfig` to support string content
2. Update the `TheaterRuntime` to handle both manifest and state sources
3. Implement manifest storage in the `ActorProcess` struct
4. Modify the server management interfaces to support string-based manifests and manifest retrieval
5. Update the CLI to support the new approaches and add the manifest viewing command
6. Add tests for string-based manifest handling and initial state handling
7. Update documentation to reflect the new capabilities

## Example Usage

### Current approach (still supported):
```rust
let actor_id = theater.spawn_actor(PathBuf::from("manifests/my-actor.toml"), None).await?;
```

### New string-based approach:
```rust
let manifest_content = r#"
name = "dynamic-actor"
component_path = "./components/my-component.wasm"
interface.implements = "ntwk:simple-actor/actor"
init_state = { json = '{"counter": 0, "message": "Hello, World!"}' }
"#;

let actor_id = theater.spawn_actor_from_string(manifest_content, None).await?;
```

### CLI for viewing manifests:
```bash
theater manifest view --actor-id 12345
```

## Considerations

1. **Component Path Resolution**: When using string manifests, we need to ensure component paths are correctly resolved relative to the current working directory or using absolute paths.

2. **Error Handling**: We should provide clear error messages when parsing fails, differentiating between file-related errors and content parsing errors.

3. **Validation**: Ensure the same validation rules apply to both file-based and string-based manifests.

4. **Security**: We should consider any security implications of allowing dynamic manifest creation, especially if exposed through APIs.

5. **Remote State Sources**: For future implementation, we'll need to consider authentication, caching, and error handling for remote state sources.

6. **Storage Implications**: Storing the original manifest content may increase memory usage but provides valuable debugging information.

## Conclusion

This change will make the actor system more flexible, easier to use, and better suited for dynamic environments while maintaining backward compatibility with existing code. The enhanced initial state handling and manifest viewing capabilities will improve the developer experience and enable new use cases.
