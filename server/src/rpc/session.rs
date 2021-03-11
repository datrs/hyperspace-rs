use async_std::sync::{Arc, Mutex};
use async_std::task;
use async_std::{stream::StreamExt, sync::MutexGuard};
use async_trait::async_trait;
use corestore::{ArcFeed, Corestore, Feed, FeedEvent};
use hyperspace_common::client::Client;
use hyperspace_common::server;
use hyperspace_common::*;
// use log::*;
use std::collections::HashMap;
use std::convert::TryInto;
use std::io::{self, Error, ErrorKind};
use task::JoinHandle;

use crate::State;

#[derive(Clone)]
pub struct Session {
    client: Client,
    corestore: Corestore,
    cores: Arc<Mutex<HashMap<u32, FeedState>>>,
}

impl Session {
    pub fn new(rpc: &mut hrpc::Rpc, state: State) -> Self {
        let client = Client::new(rpc.client());
        let session = Self {
            client,
            corestore: state.corestore,
            cores: Arc::new(Mutex::new(HashMap::new())),
        };
        rpc.define_service(server::HypercoreService::new(session.clone()));
        rpc.define_service(server::CorestoreService::new(session.clone()));
        session
    }
}

fn map_err(err: anyhow::Error) -> io::Error {
    Error::new(ErrorKind::Other, err.to_string())
}

fn bad_id() -> io::Error {
    io::Error::new(ErrorKind::Other, "Bad ID")
}

impl Session {
    // async fn get_core(&mut self, id: u32) -> io::Result<&Arc<Mutex<Feed>>> {
    //     let feed = self.cores.lock().await.get(&id).ok_or(bad_id())?;
    //     Ok(feed)
    // }
}

#[async_trait]
impl server::Hypercore for Session {
    async fn get(&mut self, req: GetRequest) -> io::Result<GetResponse> {
        let cores = self.cores.lock().await;
        let feed = cores.get(&req.id).ok_or(bad_id())?;
        let mut feed = feed.lock().await;
        let block = feed.get(req.seq).await.map_err(map_err)?;
        Ok(GetResponse { block })
    }

    async fn append(&mut self, req: AppendRequest) -> io::Result<AppendResponse> {
        let cores = self.cores.lock().await;
        let feed = cores.get(&req.id).ok_or(bad_id())?;
        let mut feed = feed.lock().await;
        for block in req.blocks {
            feed.append(&block).await.map_err(map_err)?;
        }
        Ok(AppendResponse {
            length: feed.len(),
            byte_length: feed.byte_len(),
            seq: feed.len(), // TODO: What is seq vs length here?
        })
    }
}

struct FeedState {
    #[allow(dead_code)]
    id: u32,
    feed: ArcFeed,
    notify_task: Option<JoinHandle<io::Result<()>>>,
}

impl FeedState {
    fn new(id: u32, feed: ArcFeed) -> Self {
        Self {
            id,
            feed,
            notify_task: None,
        }
    }

    // async fn feed<'a>(&'a self) -> MutexGuard<'a, Feed> {
    //     self.feed.lock().await
    // }

    async fn lock<'a>(&'a self) -> MutexGuard<'a, Feed> {
        self.feed.lock().await
    }

    fn set_notify_task(&mut self, task: JoinHandle<io::Result<()>>) {
        self.notify_task = Some(task);
    }
}

async fn notify_loop(id: u64, feed: ArcFeed, mut client: Client) -> io::Result<()> {
    let mut events = feed.lock().await.subscribe();
    while let Some(event) = events.next().await {
        match event {
            FeedEvent::Append => {
                let (length, byte_length) = {
                    let feed = feed.lock().await;
                    (feed.len(), feed.byte_len())
                };
                client
                    .hypercore
                    .on_append(AppendEvent {
                        id,
                        length,
                        byte_length,
                    })
                    .await?;
            }
            FeedEvent::Download(seq) => {
                let byte_length = { feed.lock().await.byte_len() };
                client
                    .hypercore
                    .on_download(DownloadEvent {
                        id,
                        seq,
                        byte_length: Some(byte_length),
                    })
                    .await?;
            }
            _ => {}
        }
    }
    Ok(())
}

#[async_trait]
impl server::Corestore for Session {
    async fn open(&mut self, req: OpenRequest) -> io::Result<OpenResponse> {
        let feed = if let Some(name) = req.name {
            self.corestore.get_by_name(name).await.map_err(map_err)?
        } else if let Some(key) = req.key {
            let key: [u8; 32] = key
                .try_into()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "Invalid key length"))?;
            self.corestore.get_by_key(key).await.map_err(map_err)?
        } else {
            return Err(Error::new(ErrorKind::Other, "Invalid parameters"));
        };

        let mut state = FeedState::new(req.id, feed.clone());
        let notify_task = task::spawn(notify_loop(
            req.id as u64,
            feed.clone(),
            self.client.clone(),
        ));
        state.set_notify_task(notify_task);

        self.cores.lock().await.insert(req.id, state);

        let feed = feed.lock().await;

        Ok(OpenResponse {
            key: feed.public_key().as_bytes().to_vec(),
            discovery_key: None,
            length: feed.len(),
            byte_length: feed.byte_len(),
            writable: feed.secret_key().is_some(),
            peers: vec![],
        })
    }
}

#[async_trait]
impl server::Network for Session {
    async fn configure(
        &mut self,
        _request: ConfigureNetworkRequest,
    ) -> io::Result<NetworkStatusResponse> {
        Ok(NetworkStatusResponse { status: None })
    }
}
