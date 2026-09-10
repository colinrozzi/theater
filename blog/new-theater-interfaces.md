We have made some big changes to theater over the past week.

We removed runtime state threading and our direct wasmtime dependency, and made some big architectural changes to supervision and monitoring. All of this fell at once, but it is in the goal of pushing logic out of the runtime to focus its surface area and empowering theater to be execution environment agnostic.

The most interesting changes going forward are the wasmtime dependency and the monitoring.

By abstracting over the execution environment, we are trying to bring theater to the browser and embedded. Theater actors provide system builders with reproducibility, traceability, and a stable execution model and there is no reason that should be limited to native. In the very near future, the code that runs on a server to sync with the network should be the same code running in the browser, with two different state interaction tools hooked in. This will allow for more localized and disparate interfaces, as all you will need to hook into the system will be the syncer, running locally. More interesting applications to come soon.

Supervision and monitoring has been something baked into the runtime in a somewhat hacky way for a long time. This iteration has removed the tracking of the supervision tree from the runtime, and consolidated the subscription to actor events and lifecycle events into the same concept. Now, an actor will establish either a link (tying lifecycles) or a monitor. Then, the initiating actor will receive in its handler all events the subscribed to actor produces. The initiating actor has the ability to filter out events in the handler (to save actor time), and to react to events.

More to come!
