# Actor Model & Supervision for AI Agents

The Actor Model is the second pillar of Theater, providing a robust framework for organizing, communicating, and managing AI agents. Inspired by systems like Erlang/OTP, Theater implements actors with hierarchical supervision to build resilient and scalable agent architectures.

## Agents as Actors: Isolated Units of Intelligence

In Theater, each AI agent is implemented as an *actor*. Each agent is an independent entity characterized by:

1. **Private State**: An agent maintains its own internal state, which cannot be directly accessed or modified by other agents. This isolation ensures agent autonomy and prevents interference.
2. **Mailbox**: Each agent has a mailbox where incoming messages are queued, enabling asynchronous communication.
3. **Behavior**: An agent defines how it processes incoming messages, potentially changing its state, sending messages to other agents, or taking actions in the external world.
4. **Identity**: Agents have unique identifiers used to address messages to them.

Crucially, in Theater, each agent corresponds to a running [WebAssembly Component](./wasm-components.md), benefiting from the security and isolation guarantees provided by Wasm.

## Agent Communication via Asynchronous Message Passing

All interaction between agents in Theater occurs exclusively through asynchronous message passing.

- **No Shared Memory**: Agents do not share memory. To communicate, an agent sends an immutable message to another agent's address.
- **Asynchronous & Non-Blocking**: When an agent sends a message, it does not wait for the recipient to process it. This allows agents to work concurrently without blocking each other.
- **Location Transparency**: Agents communicate using addresses, without needing to know the physical location of the recipient agent. (Note: While Theater currently runs agents within a single process, the model allows for future distribution).
- **Explicit and Traceable**: All interactions are explicit message sends, which are captured by Theater's [Traceability](./traceability.md) system.

This communication style creates clear boundaries between agents, simplifies concurrency management, and provides a natural way to organize complex agent systems.

## Agent Isolation: The Core Benefit

The strict isolation provided by the Actor Model (agents having their own state and communicating only via messages) delivers several critical benefits for AI agent systems:

- **Fault Isolation**: If an agent encounters an error or behaves unexpectedly, the failure is contained within that agent. It does not directly affect the state or operation of other agents in the system.
- **Independent Lifecycle**: Agents can be started, stopped, restarted, or even upgraded independently without necessarily affecting unrelated parts of the system.
- **Capability Containment**: Each agent can be granted precisely the capabilities it needs, without sharing those capabilities with other agents.
- **State Management**: Because state is encapsulated, Theater can manage agent state persistence and recovery more easily. State can potentially be preserved across restarts or upgrades.

## Hierarchical Supervision for Reliable Agent Systems

Theater adopts Erlang/OTP's concept of hierarchical supervision to manage agent failures gracefully.

- **Supervision Trees**: Agents are organized into a tree structure where parent agents *supervise* their child agents.
- **Monitoring**: Supervisors monitor the health and behavior of their children.
- **Recovery Strategies**: When a child agent fails (e.g., crashes due to an unhandled error), its supervisor is notified and decides how to handle the failure based on a defined strategy. Common strategies include:
    * **Restart**: Restart the failed agent, potentially restoring its last known good state.
    * **Stop**: Terminate the failed agent permanently if it's deemed unrecoverable or non-essential.
    * **Escalate**: If the supervisor cannot handle the failure, it can fail itself, escalating the problem to *its* supervisor.
    * **Restart Siblings**: In some cases, a failure in one agent might require restarting other related agents (siblings).

This structure allows developers to define how the system should react to failures, building self-healing capabilities directly into the agent architecture. Error handling becomes a primary architectural concern, rather than an afterthought.

## Agent Patterns with the Actor Model

The Actor Model enables several powerful patterns for AI agent systems:

### 1. Specialized Agent Teams

Create teams of specialized agents that work together on complex tasks:

```
CoordinatorAgent
    ├── ResearchAgent
    ├── AnalysisAgent
    └── ReportGenerationAgent
```

Each agent focuses on what it does best, communicating results to the next agent in the workflow.

### 2. Agent Redundancy and Load Balancing

Create multiple instances of the same agent type to handle high workloads or provide redundancy:

```
RouterAgent
    ├── WorkerAgent-1
    ├── WorkerAgent-2
    ├── WorkerAgent-3
    └── WorkerAgent-4
```

The router distributes work across the workers and can easily spin up more workers as needed.

### 3. Progressive Agent Specialization

Create hierarchies of increasingly specialized agents:

```
GeneralCoordinatorAgent
    ├── ResearchTeamAgent
    │   ├── WebSearchAgent
    │   ├── AcademicDatabaseAgent
    │   └── PatentSearchAgent
    └── AnalysisTeamAgent
        ├── StatisticalAnalysisAgent
        ├── SentimentAnalysisAgent
        └── TrendIdentificationAgent
```

Each level in the hierarchy represents a more focused specialization.

## Benefits for AI Agent Systems

Integrating the Actor Model with supervision provides Theater with:

1. **Clear Agent Boundaries**: A natural model for defining the scope and capabilities of individual agents.
2. **Enhanced Fault Tolerance**: The ability to contain failures and automatically recover parts of the agent system.
3. **Scalability**: Agents can potentially be distributed across cores or machines to handle increased load.
4. **Resilience**: Systems can remain partially or fully operational even when individual agents fail.
5. **Modular Evolution**: Agents can often be developed, deployed, and updated independently, facilitating continuous improvement without system downtime.

Combined with WebAssembly components, the Actor Model allows Theater to manage complex, evolving agent systems within a structure designed for resilience and adaptability.
