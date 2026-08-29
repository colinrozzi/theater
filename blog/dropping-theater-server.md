# Dropping Theater Server

Today we are dropping theater server support from the runtime.

For a long time, there has been a special admin "hook" into the system that has had direct access to the core runtime's types and loop, and has been the main way of interfacing with the system. Now that our actors are mature enough, and our handlers have reached functionality, this is no longer necessary. Going forward, all control planes and runtime functionality should be reachable directly from actors themselves. Any hooks into the system should come from the actor level, not directly into the core.

To accompany this change, we are moving the runtime interface from serving the actor's own abilities, to the actors hook into the runtime. Right now this only provides the ability to listen for actor spawns, but in the future will have much deeper introspection into real time metrics and events within the runtime.

Also, we have reworked the supervisor interface to provide us with new global tools, and accompanying permissions.
