# hyperspace-rs

An *exploration towards a* Rust daemon for Hypercore storage and replication.

* **[hyperspace-server](server)**: Main binary. Runs a Hypercore storage and networking daemon and can be talked with over a RPC socket.
* **[hyperspace-client](client)**: Client library to talk with a hyperspace server.
* **[corestore](corestore)**: Manage storage for hypercores. A port of [corestore](https://github.com/andrewosh/corestore).
* **[hypercore-replicator](replicator)**: Replicate Hypercores with [hypercore-protocol](https://github.com/Frando/hypercore-protocol-rs). Manages channels for many feeds and peer connections.
* **[hyperspace-common](common)**: Modules shared between server and client. RPC codegen, utilities.

### Example

This example starts two `hyperspace-server` instances and runs a hyperswarm DHT node on localhost. It then uses `hyperspace-client` to create and append a hypercore on the first instance. Then, again with `hyperspace-client`, the same feed is requested from the second server instance, which should replicate it because the two servers find each other over the localhost DHT. All in Rust, no Node involved!

```bash
# The commands need to be run in seperate terminals each.

# 1. Start a DHT node
cargo run --bin hyperspace-server -- --dht -a 127.0.0.1:3401

# 2. Start server 1
cargo run --bin hyperspace-server -- -s /tmp/hs1 -h hs1 -a 127.0.0.1:3402 -b 127.0.0.1:3401 -p 7001

# 3. Start server 2
cargo run --bin hyperspace-server -- -s /tmp/hs2 -h hs2 -a 127.0.0.1:3403 -b 127.0.0.1:3401 -p 7002

# 4. Write to a feed on server 1
cargo run --bin hyperspace-client -- -h hs1 -n afeed write
# the feed's key will be printed
# (type something and press enter, it will be appended to the feed)

# 5. Read from the feed from server 2
cargo run --bin hyperspace-client -- -h hs2 -k KEY_FROM_ABOVE read
