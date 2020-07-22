# hyperspace

A Rust daemon for Hypercore Protocol.

* **[hyperspace-server](server)**: Main binary. Runs a Hypercore storage and networking daemon and can be talked with over a RPC socket.
* **[hyperspace-client](client)**: Client library to talk with a hyperspace server.
* **[corestore](corestore)**: Manage storage for hypercores. A port of [corestore](https://github.com/andrewosh/corestore).
* **[hypercore-replicator](replicator)**: Replicate Hypercores with [hypercore-protocol](https://github.com/Frando/hypercore-protocol-rs). Manages channels for many feeds and peer connections.
* **[hyperspace-common](common)**: Modules shared between server and client. RPC codegen, utilities.
