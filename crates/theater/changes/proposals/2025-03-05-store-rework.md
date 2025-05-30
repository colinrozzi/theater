
I would like to rethink the way that we are using stores.
Right now we have a single store that can be used by any of the actors that is run by the theater_runtime. It serves as a method of persisting data across instances of an actor and across different actors, reducing the need for actors to communicate with each other directly.
while i like the idea of stores, and I like the patterns we use to access them and to store data in them, I do not like this single store approach.

I would like to propose we move to a store model where we have some store directory, containing any number of stores. Each store can be identified by a unique name, and can be accessed by anything that has that name.
