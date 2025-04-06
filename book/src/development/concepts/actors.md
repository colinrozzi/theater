# Actors

The fundamental unit of computation in the system is the Actor. Each actor is essentially a process. Abilities are exposed to actors through a set of interfaces. The interfaces are hooks into the Theater runtime that allow actors to interact with the host system. Actors are only required to implement the message-server interface. This interface is the way that actors receive messages from the host system.

Each actor has a set of handlers as specified in the actor's manifest.
