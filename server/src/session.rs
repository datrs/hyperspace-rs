use async_std::sync::{Arc, Mutex};
use async_trait::async_trait;
use corestore::Corestore;
use corestore::Feed;
use hyperspace_common::client::Client;
use hyperspace_common::server;
use hyperspace_common::*;
use std::collections::HashMap;
use std::io::{self, Error, ErrorKind};

use crate::State;

#[derive(Clone)]
pub struct Session {
    client: Client,
    corestore: Arc<Mutex<Corestore>>,
    cores: Arc<Mutex<HashMap<u32, Arc<Mutex<Feed>>>>>,
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

#[async_trait]
impl server::Corestore for Session {
    async fn open(&mut self, req: OpenRequest) -> io::Result<OpenResponse> {
        eprintln!("req: {:?}", req);
        let feed = if let Some(name) = req.name {
            self.corestore
                .lock()
                .await
                .get_by_name(name)
                .await
                .map_err(map_err)?
        } else if let Some(key) = req.key {
            self.corestore
                .lock()
                .await
                .get_by_key(key)
                .await
                .map_err(map_err)?
        } else {
            return Err(Error::new(ErrorKind::Other, "Invalid parameters"));
        };

        self.cores.lock().await.insert(req.id, feed.clone());

        let feed = feed.lock().await;

        Ok(OpenResponse {
            key: feed.public_key().as_bytes().to_vec(),
            length: feed.len(),
            byte_length: feed.byte_len(),
            writable: feed.secret_key().is_some(),
            peers: vec![],
        })
    }
}
