use async_std::prelude::*;
use async_std::task;
use corestore::{replicate_corestore, Corestore};
use hypercore_replicator::Replicator;
use hyperspace_common::socket_path;
use log::*;

mod network;
mod options;
mod rpc;
pub use hyperswarm::run_bootstrap_node;
pub use options::Opts;

const STORAGE_DIR: &str = ".hyperspace-rs";

/// Shared application state.
#[derive(Clone)]
pub struct State {
    corestore: Corestore,
    replicator: Replicator,
}

// pub struct Server {
//     opts: Opts
// }

// impl Server {
//     pub fn with_opts(opts: Opts) -> Self {
//         Self { opts }
//     }
// }

/// Open the server and start listening
///
/// This will run a few things in parallel tasks:
/// - a corestore that stores hypercore feeds
/// - the hyperswarm dht, waiting for incoming peer connections
/// - an hrpc socket with corestore and hypercore services
pub async fn listen(opts: Opts) -> anyhow::Result<()> {
    debug!("start server with {:?}", opts);
    let storage = opts
        .storage
        .clone()
        .unwrap_or_else(|| dirs::home_dir().unwrap().join(STORAGE_DIR));
    let socket_path = socket_path(opts.host.clone());

    // Open a corestore and wrap in Arc<Mutex>
    let corestore = Corestore::open(storage).await?;

    // Create a replicator
    let replicator = Replicator::new();
    // Our application state that can be passed around.
    let state = State {
        corestore: corestore.clone(),
        replicator: replicator.clone(),
    };

    // Add all feeds in the corestore to the replicator
    let task1 = task::spawn(replicate_corestore(corestore, replicator.clone()));
    // Join the hyperswarm DHT on the discovery keys, add all
    // incoming connections to the replicator.
    let task2 = network::run(replicator, opts);
    // Open the RPC socket and wait for incoming connections.
    let task3 = task::spawn(rpc::run_rpc(socket_path, state));
    task1.try_join(task2).try_join(task3).await?;
    Ok(())
}
