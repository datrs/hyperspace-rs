use async_std::net::{TcpListener, TcpStream};
use async_std::task::{self, JoinHandle};
use futures::channel::mpsc;
use futures::future::Either;
use futures::sink::SinkExt;
use futures::StreamExt;
use hypercore_replicator::{Replicator, ReplicatorEvent};
use hyperswarm_dht::{DhtConfig, HyperDht, HyperDhtEvent, QueryOpts};
use log::*;
use std::convert::TryInto;
use std::io;
use std::net::SocketAddr;

use crate::Opts;

#[derive(Debug)]
struct NetworkStatus {
    topic: Vec<u8>,
    announce: bool,
    lookup: bool,
}

pub async fn swarm(mut replicator: Replicator, opts: Opts) -> io::Result<()> {
    let port = opts.port;

    let (mut configure_tx, mut peer_rx, swarm_task) = dht(opts).await?;
    let mut replicator_events = replicator.subscribe().await;

    let accept_task = task::spawn(accept_task(replicator.clone(), port));

    let connect_task = task::spawn(async move {
        while let Some(peers) = peer_rx.next().await {
            // eprintln!("GOT PEERS {:?}", peers);
            // TODO: Connect over utp.
            for addr in peers {
                let socket = TcpStream::connect(addr).await;
                match socket {
                    Ok(stream) => {
                        replicator.add_stream(stream, true).await;
                    }
                    Err(err) => error!("Error connecting to peer {}: {}", addr, err),
                }
            }
        }
    });

    // TODO: Don't announce all feeds.
    let configure_task = task::spawn(async move {
        while let Some(event) = replicator_events.next().await {
            match event {
                ReplicatorEvent::Feed(discovery_key) => {
                    let status = NetworkStatus {
                        topic: discovery_key,
                        announce: true,
                        lookup: true,
                    };
                    configure_tx.send(status).await.unwrap();
                }
                _ => {}
            }
        }
    });

    swarm_task.await;
    connect_task.await;
    configure_task.await;
    accept_task.await.unwrap();
    Ok(())
}

type ConfigTx = mpsc::Sender<NetworkStatus>;
// type ConfigRx = mpsc::Receiver<NetworkStatus>;
// type PeersTx = mpsc::Sender<Vec<SocketAddr>>;
type PeersRx = mpsc::Receiver<Vec<SocketAddr>>;

async fn dht(opts: Opts) -> io::Result<(ConfigTx, PeersRx, JoinHandle<()>)> {
    let (mut peer_tx, peer_rx) = mpsc::channel(100);
    let (configure_tx, mut configure_rx) = mpsc::channel::<NetworkStatus>(100);

    let config = DhtConfig::default();
    let config = if opts.bootstrap.len() > 0 {
        config.set_bootstrap_nodes(&opts.bootstrap[..])
    } else {
        config
    };
    let config = config
        .bind(opts.address)
        .await
        .expect("Failed to create dht with socket");
    let mut node: HyperDht = HyperDht::with_config(config).await?;

    let port = opts.port;
    let task = task::spawn(async move {
        loop {
            match futures::future::select(node.next(), configure_rx.next()).await {
                // DHT event
                Either::Left((Some(event), _)) => {
                    debug!("swarm event: {:?}", event);
                    match event {
                        HyperDhtEvent::Bootstrapped { .. } => {}
                        HyperDhtEvent::AnnounceResult { .. } => {}
                        HyperDhtEvent::LookupResult { lookup, .. } => {
                            let remotes = lookup.remotes().cloned().collect::<Vec<_>>();
                            peer_tx.send(remotes).await.unwrap();
                        }
                        HyperDhtEvent::UnAnnounceResult { .. } => {}
                        _ => {}
                    }
                }
                // Configure event
                Either::Right((Some(status), _)) => {
                    debug!("configure network: {:?}", status);
                    if status.announce {
                        let opts: QueryOpts = (&status.topic[..]).try_into().unwrap();
                        let opts = opts.port(port);
                        debug!("announce: {:?}", opts);
                        node.announce(opts);
                    }
                    if status.lookup {
                        let opts: QueryOpts = (&status.topic[..]).try_into().unwrap();
                        let opts = opts.port(port);
                        debug!("lookup: {:?}", opts);
                        node.lookup(opts);
                    }
                }
                _ => {}
            }
        }
    });
    Ok((configure_tx, peer_rx, task))
}

pub async fn accept_task(mut replicator: Replicator, port: u32) -> io::Result<()> {
    let address = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&address).await?;
    info!(
        "accpeting peer connections on tcp://{}",
        listener.local_addr()?
    );
    let mut incoming = listener.incoming();
    while let Some(Ok(stream)) = incoming.next().await {
        let peer_addr = stream.peer_addr().unwrap().to_string();
        info!("new peer connection from {}", peer_addr);
        replicator.add_stream(stream, false).await;
    }
    Ok(())
}

pub async fn run_bootstrap_node() -> io::Result<(SocketAddr, JoinHandle<()>)> {
    // ephemeral node used for bootstrapping
    let mut bs =
        HyperDht::with_config(DhtConfig::default().empty_bootstrap_nodes().ephemeral()).await?;

    let bs_addr = bs.local_addr()?;

    let task = async_std::task::spawn(async move {
        loop {
            // process each incoming message
            bs.next().await;
        }
    });
    Ok((bs_addr, task))
}
