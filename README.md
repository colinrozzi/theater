# Theater

A WebAssembly actor runtime for reproducible, isolated, and observable programs.

Every run in Theater produces a chain, created by hashing all of the information that crosses the wasm sandbox. Right now, this chain is mostly used for debugging, but there are many exciting 

An actor's chain can be used for many things. First and currently most important, debugging. When an actor fails, you have a complete and reproducible record of everything that led up to that failure. Here is a sample run:

```

```
