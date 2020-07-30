# hyperspace-rs

<div align="center">
  <!-- Crates version -->
  <a href="https://crates.io/crates/{{PROJECTNAME}}">
    <img src="https://img.shields.io/crates/v/{{PROJECTNAME}}.svg?style=flat-square"
    alt="Crates.io version" />
  </a>
  <!-- Downloads -->
  <a href="https://crates.io/crates/{{PROJECTNAME}}">
    <img src="https://img.shields.io/crates/d/{{PROJECTNAME}}.svg?style=flat-square"
      alt="Download" />
  </a>
  <!-- docs.rs docs -->
  <a href="https://docs.rs/{{PROJECTNAME}}">
    <img src="https://img.shields.io/badge/docs-latest-blue.svg?style=flat-square"
      alt="docs.rs docs" />
  </a>
</div>


An exploration towards a Rust daemon for Hypercore storage and replication.

This repo contains a few different crates that together work towards an implementation of the [hyperspace](https://github.com/hyperspace-org/hyperspace) server (orignally written in Node.js). Hyperspace is a small server that stores and manages [Hypercores](https://github.com/datrs/hypercore), and can replicate Hypercores to peers found over the [Hyperswarm](https://github.com/mattsse/hyperswarm-dht) distributed hash table.

### Crates

All are still work in progress and in many parts unfinished.

* **[hyperspace-server](server)**: Runs a Hypercore storage and networking daemon that can be talked with over an RPC socket through [hrpc](https://github.com/Frando/hrpc-rs). *Library and binary*
* **[hyperspace-client](client)**: A client that talks with a hyperspace server over [hrpc](https://github.com/Frando/hrpc-rs) and exposes a `RemoteHypercore` and `RemoteCorestore`. *Libray and binary*
* **[corestore](corestore)**: Manage storage for hypercores. A port of [corestore](https://github.com/andrewosh/corestore). *Library*
* **[hypercore-replicator](replicator)**: Replicate Hypercores with [hypercore-protocol](https://github.com/Frando/hypercore-protocol-rs). Manages channels for many feeds and peer connections. *Library*
* **[hyperspace-common](common)**: Modules shared between server and client. RPC codegen, utilities. *Library*

*Note: None of these crates are published on [crates.io](https://crates.io/) yet*

### Notes

- The modules here use a Hypercore branch with two unmerged PRs that [add an event emitter](https://github.com/datrs/hypercore/pull/116) and [remove the generic type argument on the `Feed` struct](https://github.com/datrs/hypercore/pull/113))
- The [hyperswarm-dht](https://github.com/mattsse/hyperswarm-dht), which the hyperswarm-server uses, is work in progress and may still break public API
- [hrpc](https://github.com/Frando/hrpc-rs) is not yet published on crates.io because it depends on an [unmerged PR in prost](https://github.com/danburkert/prost/pull/317)

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
```

### Contributing

This project, even though in early stages, is very open to contributions. If unsure, please find us in #datrs on IRC or open an issue first.
