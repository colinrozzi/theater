Right now I am looking to make it so there is a resolving event at the end of a chain, so that people consuming the chain know the actor has been shut down.
Now, I am not entirely sure how to do this, but it is bringing up some interesting questions.

First off, we have a terminate actor thing built, but we also have the ability to save the actor's chain, which is a blocking operation. So, we could definitely get into the case where we get a fatal or unexpected error in the actorwhich leads to the actor being frozen, and therefore unable to respond to the request to save the chain, meaning the termination command might fail because the actor failed. Obviously not intended, the termination command should be a kind of force exit that will be used exactly in the case where the actor is not responding.

leaving that aside, the real issue is that the actor's have ownership over their chain, when the chain should really be shared with the runtime. The chain is under the purview of the runtime, it needs it to save it and to push it around to other entities, so the runtime should hold a way of accessing / owning the chain and the chain should be able to live on past the actor's lifetime.

