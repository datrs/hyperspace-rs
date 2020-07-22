use anyhow::Result;
use async_std::net::{TcpListener, TcpStream};
use async_std::sync::{Arc, Mutex};
use async_std::task;
use futures::stream::StreamExt;
use hypercore_protocol::discovery_key;
use log::*;
use std::env;

use corestore::{replicate_corestore, Corestore};
use hypercore_replicator::Replicator;

fn main() -> Result<()> {
    env_logger::from_env(env_logger::Env::default().default_filter_or("info")).init();
    task::block_on(async_main())
}

fn usage() {
    println!("usage: cargo run -- <storage> <client|server>");
    std::process::exit(1);
}

fn keys() -> Vec<Vec<u8>> {
    let keys = [
        "f22d5e2f38acbe6b1050ec9d11a35c4e49113a50b83607d784e31d71318fb001",
        "48b3f23d7f2944b736d2a97643f0cecb98753d63c595dd80b28be4dfd748e929",
    ];
    keys.iter().map(|k| hex::decode(k).unwrap()).collect()
}

fn fmtkey<K>(key: K) -> String
where
    K: AsRef<[u8]>,
{
    pretty_hash::fmt(key.as_ref()).unwrap()
}

async fn async_main() -> Result<()> {
    if env::args().len() != 3 {
        usage();
    }
    let storage = env::args().nth(1).unwrap();
    let mode = env::args().nth(2).unwrap();

    eprintln!("start. storage {} mode {}", storage, mode);

    let corestore = Corestore::open(storage).await?;
    let corestore = Arc::new(Mutex::new(corestore));

    {
        let feed = corestore.lock().await.get_by_name("first").await?;
        let mut feed = feed.lock().await;
        feed.append("foo".as_bytes()).await?;
        eprintln!("get 0: {:?}", feed.get(0).await?);
    }

    {
        let mut corestore = corestore.lock().await;
        for key in keys().iter() {
            let feed = corestore.get_by_key(&key).await?;
            let feed = feed.lock().await;
            info!(
                "added feed: key {} dkey {} len {}",
                fmtkey(feed.public_key().as_bytes()),
                fmtkey(discovery_key(feed.public_key().as_bytes())),
                feed.len()
            );
        }
    }

    let mut replicator = Replicator::new();
    let task = task::spawn(replicate_corestore(corestore.clone(), replicator.clone()));

    let network_task = match mode.as_ref() {
        "client" => task::spawn(task_client(replicator.clone())),
        "server" => task::spawn(task_server(replicator.clone())),
        _ => panic!(usage()),
    };

    timeout(200).await;
    let stats = replicator.stats().await;
    eprintln!("STATS: {:?}", stats);

    task.await;
    match replicator.join_all().await {
        Ok(_) => info!("Replicator task closed without error"),
        Err(err) => info!("Replicator task closed with error: {}", err),
    }
    match network_task.await {
        Ok(_) => info!("Network task closed without error"),
        Err(err) => info!("Network task closed with error: {}", err),
    }
    Ok(())
}

pub async fn task_client(mut replicator: Replicator) -> Result<()> {
    let address = "127.0.0.1:9999";
    eprintln!("now connect");
    let socket = TcpStream::connect(address).await?;
    eprintln!("client connected to server");
    replicator.add_stream(socket, true).await;
    Ok(())
}

pub async fn task_server(mut replicator: Replicator) -> Result<()> {
    let address = "127.0.0.1:9999";
    let listener = TcpListener::bind(&address).await?;
    log::info!("listening on {}", listener.local_addr()?);
    let mut incoming = listener.incoming();
    while let Some(Ok(stream)) = incoming.next().await {
        let _peer_addr = stream.peer_addr().unwrap().to_string();
        replicator.add_stream(stream, false).await;
    }
    Ok(())
}

async fn timeout(ms: u64) {
    let _ = async_std::future::timeout(
        std::time::Duration::from_millis(ms),
        futures::future::pending::<()>(),
    )
    .await;
}
