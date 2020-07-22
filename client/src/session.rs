use crate::codegen;
use crate::codegen::client::Client;
use crate::codegen::*;
use crate::freemap::NamedMap;
use async_trait::async_trait;
use futures::channel::mpsc;
use futures::future;
use futures::future::FutureExt;
use futures::sink::SinkExt;
use hrpc::Rpc;
use parking_lot::{RwLock, RwLockReadGuard};
use std::fmt;
use std::future::Future;
use std::io::Result;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
// use std::pin::Pin;
// use std::task::{Context, Poll};
// use async_std::sync::{RwLock, RwLockReadGuard};

use crate::{ByteStream, ReadStream};

type Key = Vec<u8>;
type Sessions = NamedMap<String, RemoteHypercore>;

#[derive(Clone)]
pub struct RemoteCorestore {
    client: Client,
    sessions: Sessions,
    resource_counter: Arc<AtomicU64>,
}

impl RemoteCorestore {
    pub fn new(rpc: &mut Rpc) -> Self {
        let client = Client::new(rpc.client());
        let corestore = Self {
            client,
            sessions: NamedMap::new(),
            resource_counter: Arc::new(AtomicU64::new(1)),
        };
        rpc.define_service(codegen::server::HypercoreService::new(corestore.clone()));
        corestore
    }

    pub async fn open_by_key(&mut self, key: Key) -> Result<RemoteHypercore> {
        let hkey = hex::encode(&key);
        if let Some(mut core) = self.sessions.get_by_name(&hkey) {
            core.open().await?;
            return Ok(core);
        }
        let mut core = RemoteHypercore::new(&self, Some(key), None);
        let id = self.sessions.insert_with_name(core.clone(), hkey);
        core.set_id(id).await;
        core.open().await?;
        Ok(core)
    }

    pub async fn open_by_name(&mut self, name: impl ToString) -> Result<RemoteHypercore> {
        let name = name.to_string();
        if let Some(mut core) = self.sessions.get_by_name(&name) {
            core.open().await?;
            return Ok(core);
        }
        let mut core = RemoteHypercore::new(&self, None, Some(name.clone()));
        let id = self.sessions.insert_with_name(core.clone(), name);
        core.set_id(id).await;
        core.open().await?;

        let key = core.inner.read().key.clone();
        if let Some(key) = key {
            let hkey = hex::encode(key);
            self.sessions.set_name(id, hkey);
        }
        Ok(core)
    }
}

#[async_trait]
impl codegen::server::Hypercore for RemoteCorestore {
    async fn on_append(&mut self, req: AppendEvent) -> Result<()> {
        if let Some(core) = self.sessions.get(req.id) {
            core.on_append(req.length, req.byte_length).await
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct RemoteHypercore {
    inner: Arc<RwLock<InnerHypercore>>,
    client: Client,
    resource_counter: Arc<AtomicU64>,
}
impl fmt::Debug for RemoteHypercore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.inner.read())
    }
}

impl RemoteHypercore {
    pub(crate) fn new(corestore: &RemoteCorestore, key: Option<Key>, name: Option<String>) -> Self {
        let inner = InnerHypercore {
            key,
            name,
            ..Default::default()
        };
        let inner = Arc::new(RwLock::new(inner));
        Self {
            inner,
            client: corestore.client.clone(),
            resource_counter: corestore.resource_counter.clone(),
        }
    }

    async fn set_id(&mut self, id: u64) {
        self.inner.write().id = id;
    }

    // async fn id(&self) -> u64 {
    //     self.inner.read().id
    // }

    pub fn len(&self) -> u64 {
        self.inner.read().length
    }

    pub fn byte_length(&self) -> u64 {
        self.inner.read().byte_length
    }

    pub fn read(&self) -> RwLockReadGuard<'_, InnerHypercore> {
        self.inner.read()
    }

    pub async fn append(&mut self, blocks: Vec<Vec<u8>>) -> Result<()> {
        let id = self.inner.read().id;
        let res = self
            .client
            .hypercore
            .append(AppendRequest {
                id: id as u32,
                blocks,
            })
            .await?;
        let mut inner = self.inner.write();
        inner.length = res.length;
        inner.byte_length = res.byte_length;
        Ok(())
    }

    pub fn get<'a>(&'a mut self, seq: u64) -> impl Future<Output = Result<Option<Vec<u8>>>> + 'a {
        let id = self.inner.read().id;
        let resource_id = self.resource_counter.fetch_add(1, Ordering::SeqCst);
        let request = GetRequest {
            id: id as u32,
            seq,
            resource_id,
            wait: None,
            if_available: None,
            on_wait_id: None,
        };
        let future = self.client.hypercore.get(request);
        future.map(|result| result.map(|r| r.block))
    }

    pub async fn seek(&mut self, byte_offset: u64) -> Result<(u64, usize)> {
        let id = self.inner.read().id;
        let res = self
            .client
            .hypercore
            .seek(SeekRequest {
                id: id as u32,
                byte_offset,
                start: None,
                end: None,
                wait: None,
                if_available: None,
            })
            .await?;
        Ok((res.seq, res.block_offset as usize))
    }

    pub fn to_stream(&mut self, start: u64, end: Option<u64>, live: bool) -> ReadStream {
        ReadStream::new(self.clone(), start, end, live)
    }

    pub fn to_reader(&mut self) -> ByteStream {
        ByteStream::new(self.clone())
    }

    pub fn subscribe(&mut self) -> mpsc::Receiver<HypercoreEvent> {
        self.inner.write().subscribe()
    }

    // pub(crate) async fn emit(&mut self, event: HypercoreEvent) {
    //     self.inner.write().emit(event).await
    // }

    async fn open(&mut self) -> Result<()> {
        if self.inner.read().open {
            return Ok(());
        }
        let req = {
            let inner = self.inner.read();
            OpenRequest {
                id: inner.id as u32,
                key: inner.key.clone(),
                name: inner.name.clone(),
                weak: None,
            }
        };
        let res = self.client.corestore.open(req).await?;
        let mut inner = self.inner.write();
        inner.key = Some(res.key);
        inner.open = true;
        inner.length = res.length;
        inner.byte_length = res.byte_length;
        inner.writable = res.writable;
        Ok(())
    }

    pub(crate) async fn on_append(&self, length: u64, byte_length: u64) {
        let mut inner = self.inner.write();
        inner.length = length;
        inner.byte_length = byte_length;

        let event = HypercoreEvent::Append;
        let mut listeners = inner.listeners.clone();
        let futs = listeners.iter_mut().map(|s| s.send(event.clone()));
        let _ = future::join_all(futs).await;
    }
}

#[derive(Debug, Clone)]
pub enum HypercoreEvent {
    Append,
    Download,
}

#[derive(Default)]
pub struct InnerHypercore {
    pub id: u64,
    pub name: Option<String>,
    pub key: Option<Key>,
    pub length: u64,
    pub byte_length: u64,
    pub writable: bool,
    pub open: bool,
    pub listeners: Vec<mpsc::Sender<HypercoreEvent>>,
}

impl InnerHypercore {
    fn subscribe(&mut self) -> mpsc::Receiver<HypercoreEvent> {
        let (send, recv) = mpsc::channel(100);
        self.listeners.push(send);
        recv
    }
    // async fn emit(&mut self, event: HypercoreEvent) {
    //     let futs = self.listeners.iter_mut().map(|s| s.send(event.clone()));
    //     let _ = future::join_all(futs).await;
    // }
}

impl fmt::Debug for InnerHypercore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "RemoteHypercore {{ key {:?}, name {:?}, id {}, length {}, byte_length {}, writable {}, open {} }}",
            self.key.as_ref().and_then(|key| Some(hex::encode(key))),
            self.name,
            self.id,
            self.length,
            self.byte_length,
            self.writable,
            self.open
        )
    }
}

// pub struct GetFuture<'a> {
//     inner: hrpc::RequestFuture<'a, GetResponse>,
// }
// impl<'a> GetFuture<'a> {
//     fn new(client: &'a mut Client, request: GetRequest) -> Self {
//         let inner = client.hypercore.get(request);
//         Self { inner }
//     }
// }

// impl<'a> Future for GetFuture<'a> {
//     // type Output = Result<Vec<u8>>;
//     type Output = Result<Option<Vec<u8>>>;
//     fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
//         let result = futures::ready!(self.inner.poll_unpin(cx));
//         // let result = result.map(|r| r.block.unwrap_or_else(|| vec![]));
//         let result = result.map(|r| r.block);
//         Poll::Ready(result)
//     }
// }
// impl<'a> Unpin for GetFuture<'a> {}
