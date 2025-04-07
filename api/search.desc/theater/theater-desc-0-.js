searchState.loadedDescShard("theater", 0, "Theater Actor System\nWebAssembly Memory Statistics\nWebAssembly Error Types\nActor Executor\nActor Handle\nActor Runtime\nActor Store\nEvent Chain System\nReturns the argument unchanged.\nReturns the argument unchanged.\nTheater ID System\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nTheater Runtime\nWebAssembly Integration for Theater\nActorError\nActorExecutor\nActorOperation\nCall a WebAssembly function in the actor\nCommunication channel to the actor was closed unexpectedly\nDefault timeout for actor operations (50 minutes)\nThe requested WebAssembly function was not found in the …\nRetrieve the actor’s event chain (audit log)\nRetrieve current metrics for the actor\nRetrieve the actor’s current state\nAn internal error occurred during execution\nInterval for updating metrics (1 second)\nOperation exceeded the maximum allowed execution time\nFailed to serialize or deserialize data\nInitiate actor shutdown\nActor is in the process of shutting down and cannot accept …\nParameter or return types did not match the WebAssembly …\nThe WebAssembly actor instance being executed\nPerform final cleanup when shutting down\nExecute a function call in the WebAssembly actor\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCollector for performance metrics\nCreate a new actor executor\nChannel for receiving operations to perform\nRun the actor executor\nFlag indicating whether shutdown has been initiated\nReceiver for system-wide shutdown signals\nChannel for sending commands back to the Theater runtime\nName of the function to call\nSerialized parameters for the function\nChannel to send the result back to the caller\nChannel to send metrics back to the caller\nChannel to confirm shutdown completion\nChannel to send chain events back to the caller\nChannel to send state back to the caller\nActorHandle\nCalls a function on the actor with the given name and …\nReturns the argument unchanged.\nRetrieves the event chain for the actor.\nRetrieves performance metrics for the actor.\nRetrieves the current state of the actor.\nCalls <code>U::from(self)</code>.\nCreates a new ActorHandle with the given operation channel.\nInitiates an orderly shutdown of the actor.\nActorRuntime\nActor failed to start with error message\nMaximum time to wait for graceful shutdown before forceful …\nResult of starting an actor\nActor successfully started\nHandle to the actor executor task\nUnique identifier for this actor\nReturns the argument unchanged.\nReturns the argument unchanged.\nHandles to the running handler tasks\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nController for graceful shutdown of all components\nStart a new actor runtime\nStop the actor runtime\nActorStore\nHandle to interact with the actor\nThe event chain that records all actor operations for …\nReturns the argument unchanged.\nGet all events in the chain\nGet the event chain\nGet the actor’s ID\nGet the most recent event\nGet the actor’s state\nGet the Theater command channel\nUnique identifier for the actor\nCalls <code>U::from(self)</code>.\nCreate a new ActorStore\nRecord an event in the chain\nSave the event chain to a file\nSet the actor’s state\nThe current state of the actor, stored as a binary blob\nChannel for sending commands to the Theater runtime\nVerify the integrity of the event chain\nChain Event\nState Chain\nThe identifier of the actor that owns this chain. This is …\nAdds a new typed event to the chain.\nHash of the most recent event in the chain, or None if the …\nThe actual payload of the event, typically serialized …\nOptional human-readable description of the event for …\nType identifier for the event, used to categorize and …\nThe ordered sequence of events in this chain, from oldest …\nReturns the argument unchanged.\nReturns the argument unchanged.\nGets all events in the chain as an ordered slice.\nGets the most recent event in the chain.\nCryptographic hash of this event’s content, used as its …\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCreates a new empty state chain for an actor.\nHash of the parent event, or None if this is the first …\nSaves the entire state chain to a JSON file.\nChannel for sending events to the Theater runtime. This is …\nUnix timestamp (in seconds) when the event was created.\nVerifies the integrity of the entire event chain.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nLoads a manifest configuration from a TOML file.\nLoads a manifest configuration from a TOML string.\nLoads a manifest configuration from a byte vector.\nChecks if the actor implements a specific interface.\nGets the primary interface implemented by the actor.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nConverts the manifest to a fixed byte representation.\nLoads the initial state data for the actor.\nGets the name of the actor.\nChain Event Data\nEvent Data\nFile system access events, such as reading or writing …\nHTTP-related events, including requests, responses, and …\nActor-to-actor messaging events for communication between …\nRuntime lifecycle events, such as initialization, state …\nContent store access events for the key-value storage …\nSupervision events related to actor parent-child …\nTheater runtime system events for the global runtime …\nTimer and scheduling events for time-based operations.\nWebAssembly execution events related to the WASM VM.\nThe specific event data payload, containing …\nGets the human-readable description of the event, if …\nOptional human-readable description of the event for …\nGets the event type identifier string.\nThe type identifier for this event, used for filtering and …\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nUnix timestamp (in seconds) when the event was created.\nConverts the typed event data to a generic chain event.\nSerializes the event data to JSON.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nTheaterId\nGets the underlying UUID.\nReturns the argument unchanged.\nGenerates a new random TheaterId.\nCalls <code>U::from(self)</code>.\nParses a TheaterId from a string.\nAn actor in the runtime\nActor Channel Close\nActor Channel Initiated\nActor Channel Message\nActor Channel Open Request\nRecord an actor error\nActor Message\nActor Request\nActor Send\nActor Status\nClose a channel\nRequest to close a channel\nChannel Identifier\nNotification of a new channel\nSend a message on a channel\nMessage on an established channel\nOpen a communication channel\nRequest to open a new channel\nChannel Participant\nAn external client (like CLI)\nActor has experienced an error or crash\nGet actor events\nGet actor metrics\nGet actor state\nGet actor status\nGet all actors\nGet channel status\nList active channels\nList child actors\nRecord a new event\nCreate a new content store\nRegister a new channel\nRequest-response interaction\nRestart an actor\nResume an existing actor\nActor is active and processing messages\nOne-way message\nSend a message to an actor\nSpawn a new actor\nStop an actor\nActor has been stopped gracefully\nSubscribe to actor events\nTheater Command\nGet the channel ID as a string\nThe unique ID for this channel\nThe ID of the channel to send on\nThe ID of the channel to close\nThe unique ID for this channel\nRequest data (serialized parameters)\nMessage data (serialized parameters)\nInitial message data (may contain authentication/metadata)\nMessage data\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nThe initial message sent on the channel\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCreate a new channel ID\nChannel to send the response back to the requester\nChannel to receive the result of the open request\nThe participant who opened the channel\nConvert a command to a loggable string\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nDefault timeout for waiting for a component to shutdown …\nController that can broadcast shutdown signals to multiple …\nReceiver that can wait for shutdown signals\nA signal indicating that a component should shutdown\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCreate a new ShutdownController and a ShutdownReceiver\nSignal all receivers to shutdown\nGet a new receiver for this controller\nWait for a shutdown signal to be received\nA reference to content in the store\nA label that references content in the store\nCalculate total size of all content in the store\nHelper method to recursively collect labels\nCheck if content exists in the store\nCheck if the label exists\nCheck if content exists\nCheck if content exists in the store (synchronous version)\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCreate a ContentRef by hashing content\nCreate a label from a string\nRetrieve content by its reference\nGet content reference by label\nRetrieve content from the filesystem\nGet content by label\nGet the ContentRef associated with this label, if any\nGet the hash as a string\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nAttach a label to content (replaces any existing content …\nList all content references in the store\nList all labels recursively, including nested directories …\nGet the label name\nCreate a new ContentRef from a hash\nCreate a new label\nCreate a new store with the given base path\nRemove this label\nRemove a specific content reference from a label If the …\nRemove a label\nSet the ContentRef for this label\nStore content and return its ContentRef\nStore content in the filesystem and ensure it exists\nStore content in the filesystem synchronously\nStore content synchronously and return its ContentRef\nConvert to a path for storage within a base directory\nConvert to a path for storage within a base directory\nActorProcess\nTheaterRuntime\nUnique identifier for the actor\nMap of active actors indexed by their ID\nOptional channel to send channel events back to the server\nMap of active communication channels\nSet of child actor IDs\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nChannel for sending messages to the actor\nPath to the actor’s manifest\nCreates a new TheaterRuntime with the given communication …\nChannel for sending operations to the actor\nTask handle for the running actor\nStarts the runtime’s main event loop, processing …\nController for graceful shutdown\nSpawns a new actor from a manifest with optional …\nCurrent status of the actor\nStops the entire runtime and all actors gracefully.\nStops an actor and its children gracefully.\nMap of event subscriptions for actors\nReceiver for commands to the runtime\nSender for commands to the runtime\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nMerge two optional initial states, with the override state …\nResolve a reference to a byte array. A reference can be a …\nActor WebAssembly Component\nWebAssembly Actor Instance\nWebAssembly Memory Statistics\nType-Safe WebAssembly Function Interface\nWebAssembly Error Types\nCalls a registered WebAssembly function with the given …\nFinds a function export in the WebAssembly component by …\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nChecks if a specific function has been registered in this …\nGets the unique identifier of this actor instance.\nInstantiates the WebAssembly component, creating a …\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCreates a new <code>ActorComponent</code> from a manifest configuration …\nRegisters a WebAssembly function with parameters and …")