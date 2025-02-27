# Description

The theater system runs and manages actors. Each of those actors has to be specified in a way that the theater system can understand. Up until now I have been doing this with local files, but i need to formalize this process and structure it better so that it can be more easily extended and maintained.
I just completed a change request to make it so manifests can be passed in as both a string or a path.

I would like to propose that we use a registry to manage the components, manifests, and initial state of the actors. This will allow us to have a single source of truth for all of the actors in the system. This will also allow us to easily add new actors to the system and manage the state of the actors in a more efficient way.
Anything that needs to be resolved from outside the system, be that an actor manifest, wasm component, or initial state should either be passed in in a string or a path.
We should have a special type of path that can be used for references to the registry. This will allow us to easily reference the registry from anywhere in the system.

registry::/{file type}/{file name}
ex: 
registry::manifests/actor1.toml
registry::components/actor1.wasm
registry::states/actor1.json

