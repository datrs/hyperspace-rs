use async_std::os::unix::net::UnixStream;
use async_std::sync::{Arc, Mutex};
use async_std::task;
use corestore::{replicate_corestore, Corestore};
use hypercore_replicator::Replicator;
use hyperspace_common::socket_path;
use log::*;

// use std::io::Result;

mod network;
mod options;
mod session;
mod socket;
pub use options::Opts;

use session::Session;

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

    let corestore = Corestore::open(storage).await?;
    let corestore = Arc::new(Mutex::new(corestore));
    let replicator = Replicator::new();
    let state = State {
        corestore: corestore.clone(),
        replicator: replicator.clone(),
    };
    let mut tasks = vec![];
    tasks.push(task::spawn(replicate_corestore(
        corestore,
        replicator.clone(),
    )));
    tasks.push(task::spawn(network::swarm(replicator, opts)));
    tasks.push(task::spawn(socket::accept(
        socket_path,
        state,
        on_rpc_connection,
    )));
    futures::future::join_all(tasks).await;
    Ok(())
}

fn on_rpc_connection(state: State, stream: UnixStream) {
    info!("new connection from {:?}", stream.peer_addr().unwrap());
    let mut rpc = hrpc::Rpc::new();
    let _session = Session::new(&mut rpc, state);
    task::spawn(async move {
        rpc.connect(stream).await.unwrap();
    });
}
