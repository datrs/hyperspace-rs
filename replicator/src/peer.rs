use anyhow::Result;
use async_std::sync::{Arc, Mutex};
use async_std::task::{self, JoinHandle};
use futures::future::Either;
use futures::future::FutureExt;
use futures::stream::StreamExt;
use hypercore::bitfield::Bitfield;
use hypercore::{Event, Feed, Node, Proof, Signature};
use hypercore_protocol::schema::*;
use hypercore_protocol::{Channel, Message};
use merkle_tree_stream::Node as NodeTrait;
use std::convert::TryInto;

use super::RemotePublicKey;

#[derive(Debug)]
pub struct PeeredFeed {
    pub(crate) feed: Arc<Mutex<Feed>>,
    stats: Vec<Arc<Mutex<Stats>>>,
    tasks: Vec<JoinHandle<Result<()>>>,
}

impl PeeredFeed {
    pub fn new(feed: Arc<Mutex<Feed>>) -> Self {
        Self {
            feed,
            stats: vec![],
            tasks: vec![],
        }
    }

    pub async fn add_peer(&mut self, id: RemotePublicKey, channel: Channel) {
        let stats = Stats {
            id,
            ..Default::default()
        };
        let stats = Arc::new(Mutex::new(stats));
        let mut peer = Peer::new(self.feed.clone(), channel, stats.clone());
        let task = task::spawn(async move { peer.run().await });
        self.tasks.push(task);
        self.stats.push(stats);
    }

    pub async fn join_all(&mut self) {
        futures::future::join_all(self.tasks.iter_mut()).await;
    }

    pub async fn stats(&mut self) -> Vec<Stats> {
        let futs = self.stats.iter_mut().map(|m| m.lock());
        let futs = futs.map(|s| s.map(|s| s.clone()));
        futures::future::join_all(futs).await
    }
}

#[derive(Debug)]
pub struct Peer {
    pub channel: Channel,
    pub feed: Arc<Mutex<Feed>>,
    pub uploading: bool,
    pub downloading: bool,
    pub remote_want: bool,
    pub remote_ack: bool,
    pub remote_length: u64,
    pub want_bitfield: Bitfield,
    pub remote_bitfield: Bitfield,
    pub stats: Arc<Mutex<Stats>>,
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    id: RemotePublicKey,
    downloaded_blocks: u64,
    downloaded_bytes: u64,
    uploaded_blocks: u64,
    uploaded_bytes: u64,
}

impl Stats {
    pub(crate) fn add_downloaded(&mut self, blocks: u64, bytes: u64) {
        self.downloaded_blocks += blocks;
        self.downloaded_bytes += bytes;
    }
    pub(crate) fn add_uploaded(&mut self, blocks: u64, bytes: u64) {
        self.uploaded_blocks += blocks;
        self.uploaded_bytes += bytes;
    }
}

impl Peer {
    pub fn new(feed: Arc<Mutex<Feed>>, channel: Channel, stats: Arc<Mutex<Stats>>) -> Self {
        Self {
            feed,
            channel,
            uploading: false,
            downloading: false,
            want_bitfield: Bitfield::new().0,
            remote_bitfield: Bitfield::new().0,
            remote_ack: false,
            remote_want: false,
            remote_length: 0,
            stats,
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut feed_events = self.feed.lock().await.subscribe();
        // eprintln!("enter peer-feed loop");
        loop {
            match futures::future::select(self.channel.next(), feed_events.next()).await {
                Either::Left((Some(message), _)) => self.on_message(message).await?,
                Either::Right((Some(event), _)) => self.on_feed_event(event).await?,
                _ => return Ok(()),
            }
        }
    }

    async fn on_feed_event(&mut self, event: Event) -> Result<()> {
        // eprintln!("feed event {:?}", event);
        match event {
            Event::Download(index) => self.send_have(index, None).await?,
            Event::Append => {
                let len = self.feed.lock().await.len();
                self.send_have(len - 1, None).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn on_message(&mut self, message: Message) -> Result<()> {
        // eprintln!("onmessage {:?}", message);
        match message {
            Message::Open(_) => self.on_open().await?,
            Message::Want(want) => self.on_want(want).await?,
            Message::Have(have) => self.on_have(have).await?,
            Message::Request(request) => self.on_request(request).await?,
            Message::Data(data) => self.on_data(data).await?,
            _ => {}
        }
        Ok(())
    }

    async fn send_have(&mut self, start: u64, length: Option<u64>) -> std::io::Result<()> {
        self.channel
            .have(Have {
                start,
                length,
                bitfield: None,
                ack: None,
            })
            .await
    }

    async fn send_wants(&mut self, index: u64) -> Result<()> {
        let len = 1024 * 1024;
        let j = index / len;
        self.want_bitfield.set(j, true);
        self.channel
            .want(Want {
                start: j * len,
                length: Some(len),
            })
            .await?;
        Ok(())

        // var len = 1024 * 1024
        // var j = Math.floor(index / len)
        // if (this.wants.get(j)) return false
        // this.wants.set(j, true)
        // this.inflightWants++
        // this.feed.ifAvailable.wait()
        // this.stream.want({start: j * len, length: len})
        // return true
    }

    async fn on_open(&mut self) -> Result<()> {
        self.send_wants(0).await
    }

    async fn on_have(&mut self, have: Have) -> Result<()> {
        // eprintln!("onhave {:?}", have);
        let mut feed = self.feed.lock().await;
        let mut wants = vec![];
        if let Some(bitfield) = have.bitfield {
            let mut bitfield = parse_bitfield(bitfield);
            for (i, val) in bitfield.iter().enumerate() {
                if val == true && !feed.has(i as u64) {
                    wants.push(i as u64);
                }
            }
        } else {
            let len = have.length.or(Some(1)).unwrap();
            let start = have.start;
            let end = start + len;
            for i in start..end {
                if feed.has(i as u64) == false {
                    wants.push(i as u64);
                }
            }
        }

        // eprintln!("WANT at len {} wants {:?}", feed.len(), wants);

        for idx in wants {
            self.channel
                .request(Request {
                    index: idx as u64,
                    bytes: None,
                    hash: None,
                    nodes: None,
                })
                .await?;
        }
        Ok(())
    }

    async fn on_want(&mut self, want: Want) -> Result<()> {
        // if !self.uploading {
        //     return Ok(());
        // }
        // if want.start & 8191 {
        //     return Ok(());
        // }
        // if let Some(length) == want.length {
        //     if length & 8191 {
        //         return Ok(());
        //     }
        // }
        let feed = self.feed.lock().await;
        let len = feed.len();
        if !self.remote_want {
            if len > 0 {
                self.channel
                    .have(Have {
                        start: len - 1,
                        length: None,
                        bitfield: None,
                        ack: None,
                    })
                    .await?;
            }
        }
        self.remote_want = true;
        let length = want.length.or(Some(len)).unwrap();
        let rle = feed
            .bitfield()
            .compress(want.start as usize, length as usize)?
            .to_vec();
        self.channel
            .have(Have {
                start: want.start,
                length: Some(length),
                bitfield: Some(rle),
                ack: None,
            })
            .await
            .map_err(|e| e.into())
    }

    async fn on_data(&mut self, data: Data) -> Result<()> {
        let mut feed = self.feed.lock().await;
        let signature = match data.signature {
            Some(bytes) => {
                let signature: Signature = (&bytes[..]).try_into()?;
                Some(signature)
            }
            None => None,
        };
        let nodes = data
            .nodes
            .iter()
            .map(|n| Node::new(n.index, n.hash.clone(), n.size))
            .collect();
        let proof = Proof {
            index: data.index,
            nodes,
            signature,
        };

        let value: Option<&[u8]> = data.value.as_ref().map(|v| v.as_slice());
        feed.put(data.index, value, proof.clone()).await?;

        self.stats
            .lock()
            .await
            .add_downloaded(1, data.value.map_or(0, |v| v.len() as u64));

        if self.remote_ack {
            self.channel
                .have(Have {
                    start: data.index,
                    length: Some(1),
                    bitfield: None,
                    ack: Some(true),
                })
                .await?;
        }
        // eprintln!("STATS {:?}", self.stats);
        Ok(())
    }

    async fn on_request(&mut self, request: Request) -> Result<()> {
        let index = request.index;
        let mut feed = self.feed.lock().await;
        let value = feed.get(index).await?;
        let proof = feed.proof(index, false).await?;
        let nodes = proof
            .nodes
            .iter()
            .map(|node| data::Node {
                index: NodeTrait::index(node),
                hash: NodeTrait::hash(node).to_vec(),
                size: NodeTrait::len(node),
            })
            .collect();
        let message = Data {
            index,
            value: value.clone(),
            nodes,
            signature: proof.signature.map(|s| s.to_bytes().to_vec()),
        };
        self.channel.data(message).await?;
        self.stats
            .lock()
            .await
            .add_uploaded(1, value.map_or(0, |v| v.len() as u64));
        // eprintln!("STATS {:?}", self.stats);
        Ok(())
    }
}

fn parse_bitfield(bitfield: Vec<u8>) -> sparse_bitfield::Bitfield {
    let mut sp_bitfield = sparse_bitfield::Bitfield::new(1024);
    let buf = bitfield_rle::decode(bitfield).unwrap();
    for (idx, byte) in buf.iter().enumerate() {
        sp_bitfield.set_byte(idx, *byte);
    }
    sp_bitfield
}
