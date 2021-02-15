use async_std::channel::{self, Receiver, Sender};
use async_std::net::{TcpListener, TcpStream};
use async_std::prelude::FutureExt;
use async_std::stream::{Stream, StreamExt};
use async_std::task::{self, JoinHandle};
use futures::future::Either;
use hypercore_replicator::{Replicator, ReplicatorEvent};
use hyperswarm_dht::{DhtConfig, HyperDht, HyperDhtEvent, QueryOpts};
use log::*;
use std::convert::TryInto;
use std::io::{self, Result};
use std::net::SocketAddr;

use crate::Opts;

#[derive(Debug)]
pub struct NetworkStatus {
    topic: Vec<u8>,
    announce: bool,
    lookup: bool,
}

pub type PeerInfoList = Vec<SocketAddr>;

pub async fn run(mut replicator: Replicator, opts: Opts) -> io::Result<()> {
    let tcp_listener = create_tcp_listener(opts.port).await?;
    let local_port = tcp_listener.local_addr()?.port();

    // Accept incoming connections on the tcp listener and forward them to the replicator.
    let accept_task = task::spawn(accept_loop(replicator.clone(), tcp_listener));

    let (peer_tx, peer_rx) = channel::bounded::<PeerInfoList>(100);
    let (configure_tx, configure_rx) = channel::bounded::<NetworkStatus>(100);

    // Connect to the DHT, listen for configure messages to announce/lookup topics on the DHT, and
    // forward found peers in the peer channel.
    let dht_task = task::spawn(dht_loop(opts, local_port, peer_tx, configure_rx));

    // Listen for found peers from the DHT and connect (over TCP) and forward the streams to the
    // replicator.
    let connect_task = task::spawn(connect_loop(replicator.clone(), peer_rx));

    // Listen for new feed events from the replicator and forward them to the DHT.
    // TODO: Don't announce all feeds.
    let replicator_events = replicator.subscribe().await;
    let configure_task = task::spawn(configure_loop(replicator_events, configure_tx));

    connect_task
        .try_join(configure_task)
        .try_join(accept_task)
        .try_join(dht_task)
        .await?;
    Ok(())
}

// type ConfigTx = Sender<NetworkStatus>;
type ConfigRx = Receiver<NetworkStatus>;
type PeerInfoTx = Sender<PeerInfoList>;
type PeerInfoRx = Receiver<PeerInfoList>;
// type PeerTx = Sender<TcpStream>;
// type PeerRx = Receiver<TcpStream>;

pub async fn configure_loop<S>(
    mut replicator_events: S,
    configure_tx: Sender<NetworkStatus>,
) -> Result<()>
where
    S: Stream<Item = ReplicatorEvent> + Unpin,
{
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
    Ok(())
}

pub async fn create_tcp_listener(port: Option<u32>) -> Result<TcpListener> {
    // If a port wasn't set in the options, use port 0 which lets the operating system assign a
    let port = port.unwrap_or(0);

    let address = format!("127.0.0.1:{}", port);
    TcpListener::bind(&address).await
}

pub async fn accept_loop(mut replicator: Replicator, listener: TcpListener) -> io::Result<()> {
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

pub async fn connect_loop(mut replicator: Replicator, mut peer_rx: PeerInfoRx) -> Result<()> {
    while let Some(peers) = peer_rx.next().await {
        // TODO: Connect over utp if tcp fails.
        for addr in peers {
            info!("Connecting to peer {}", addr);
            let tcp_socket = TcpStream::connect(addr).await;
            // TODO: Also connect via UTP.
            // .race(UtpStream::connect(addr));
            match tcp_socket {
                Ok(stream) => {
                    info!("Connected to peer {}", addr);
                    replicator.add_stream(stream, true).await;
                }
                Err(err) => {
                    error!("Error connecting to peer {}: {}", addr, err);
                }
            }
        }
    }
    Ok(())
}

async fn dht_loop(
    opts: Opts,
    local_port: u16,
    peer_tx: PeerInfoTx,
    mut configure_rx: ConfigRx,
) -> io::Result<()> {
    let config = DhtConfig::default();
    let config = if opts.bootstrap.len() > 0 {
        config.set_bootstrap_nodes(&opts.bootstrap[..])
    } else {
        config
    };
    let config = config.ephemeral();

    let config = if let Some(address) = opts.address {
        config
            .bind(address)
            .await
            .expect("Failed to create dht with socket")
    } else {
        config
    };

    // .bind(opts.address)
    // .await
    // .ephemeral()
    // .expect("Failed to create dht with socket");
    debug!("Init DHT: {:?}", config);
    let mut node: HyperDht = HyperDht::with_config(config).await?;
    debug!("Local address: {:?}", node.local_addr());

    loop {
        let event = node.next().await;
        debug!("swarm event: {:?}", event);
        if let Some(HyperDhtEvent::Bootstrapped { .. }) = event {
            debug!("DHT bootstrapped!");
            break;
        }
    }
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
                        eprintln!("REMOTES: {:?}", remotes);
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
                    let opts = opts.port(local_port as u32);
                    let opts = opts.local_addr("127.0.0.1");
                    debug!("announce: {:?}", opts);
                    node.announce(opts);
                }
                if status.lookup {
                    let opts: QueryOpts = (&status.topic[..]).try_into().unwrap();
                    // let opts = opts.port(port);
                    debug!("lookup: {:?}", opts);
                    node.lookup(opts);
                }
            }
            _ => {}
        }
    }
    // Ok(())
}

pub async fn run_bootstrap_node(opts: Opts) -> io::Result<(SocketAddr, JoinHandle<()>)> {
    let config = DhtConfig::default().empty_bootstrap_nodes();
    let config = if let Some(address) = opts.address {
        config
            .bind(address)
            .await
            .expect("Failed to create dht with socket")
    } else {
        config
    };
    // ephemeral node used for bootstrapping
    let mut node = HyperDht::with_config(config).await?;

    let local_addr = node.local_addr()?;

    let task = async_std::task::spawn(async move {
        loop {
            // process each incoming message
            debug!("wait for next event");
            let event = node.next().await;
            debug!("event: {:?}", event);
        }
    });
    Ok((local_addr, task))
}

// struct BootstrapNode {}

// impl BootstrapNode {
//     async fn run() {}
// }
