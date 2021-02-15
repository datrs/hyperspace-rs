use async_std::prelude::*;
use async_std::sync::{Arc, Mutex};
use async_std::task;
use corestore::{replicate_corestore, Corestore};
use hypercore_replicator::Replicator;
use hyperspace_common::socket_path;
use log::*;

// use std::io::Result;

mod network;
mod options;
mod rpc;
pub use options::Opts;

const STORAGE_DIR: &str = ".hyperspace-rs";

/// Shared application state.
#[derive(Clone)]
pub struct State {
    corestore: Arc<Mutex<Corestore>>,
    replicator: Replicator,
}

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

    if opts.dht {
        let (addr, task) = network::run_bootstrap_node(opts).await?;
        info!("bootstrap node address: {}", addr);
        task.await;
        std::process::exit(1);
    }

    // Open a corestore and wrap in Arc<Mutex>
    let corestore = Corestore::open(storage).await?;
    let corestore = Arc::new(Mutex::new(corestore));

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
