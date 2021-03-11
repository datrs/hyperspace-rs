use anyhow::Result;
use async_std::sync::{Arc, Mutex, RwLock, RwLockReadGuard};
use blake2_rfc::blake2b::Blake2b;
use ed25519_dalek::{PublicKey, SecretKey};
use futures::channel::mpsc;
use futures::sink::SinkExt;
use hypercore::{storage_disk, Feed};
use hypercore_replicator::discovery_key;
use log::*;
use rand::rngs::{OsRng, StdRng};
use rand::SeedableRng;
use rand_core::RngCore;
use random_access_disk::RandomAccessDisk;
use random_access_storage::RandomAccess;
use std::collections::hash_map::Values;
use std::convert::TryInto;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use crate::feed_map::FeedMap;

const MASTER_KEY_FILENAME: &str = "master_key";
const NAMESPACE: &str = "corestore";

/// A hypercore public key
pub type Key = [u8; 32];
/// A feed name
pub type Name = Vec<u8>;
/// A feed that can be shared between threads
pub type ArcFeed = Arc<Mutex<Feed>>;

/// Corestore events
#[derive(Debug, Clone)]
pub enum Event {
    /// A new feed has been opened
    Feed(Arc<Mutex<Feed>>),
}

#[derive(Clone)]
pub struct Corestore {
    inner: Arc<RwLock<InnerCorestore>>,
}

/// A store for hypercore feeds.
impl Corestore {
    /// Open a corestore from disk
    pub async fn open<P>(storage_path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let inner = InnerCorestore::open(storage_path).await?;
        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
        })
    }

    /// Subscribe to events from this corestore
    pub async fn subscribe(&mut self) -> mpsc::Receiver<Event> {
        self.inner.write().await.subscribe().await
    }

    /// Get a feed by its public key
    pub async fn get_by_key(&mut self, key: Key) -> Result<ArcFeed> {
        self.inner.write().await.get_by_key(key).await
    }

    /// Get or create a writable feed by name
    pub async fn get_by_name<T>(&mut self, name: T) -> Result<ArcFeed>
    where
        T: AsRef<str>,
    {
        self.inner.write().await.get_by_name(name).await
    }

    /// Get a feed by its discovery key
    ///
    /// This only works if the feed is found on disk
    pub async fn get_by_dkey(&mut self, dkey: Key) -> Result<Option<ArcFeed>> {
        self.inner.write().await.get_by_dkey(dkey).await
    }

    /// Iterate over all feeds.
    ///
    /// Note: A lock on the corestore is kept until the iterator is dropped,
    /// so don't use this across `await`s.
    ///
    /// Example:
    /// ```
    /// # #[async_std::main]
    /// # async fn main() -> anyhow::Result<()> {
    /// let corestore = corestore::Corestore::open("/tmp/foo").await?;
    /// for feed in &corestore.feeds().await {
    ///     // Do somethings with feed.
    ///     // Don't await things here!
    ///     // Instead, clone the feed if you need to keep a reference.
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn feeds(&self) -> FeedsIterator<'_> {
        let inner = self.inner.read().await;
        FeedsIterator::new(inner)
    }
}

/// An iterator over feeds in a corestore.
///
/// Note: Because this is tied to a read lock, the iterator is only
/// available on a reference to the iterator. See [Corestore::feeds].
pub struct FeedsIterator<'a> {
    inner: RwLockReadGuard<'a, InnerCorestore>,
}

impl<'a, 'b: 'a> IntoIterator for &'b FeedsIterator<'a> {
    type Item = &'a ArcFeed;
    type IntoIter = Values<'a, u64, ArcFeed>;

    fn into_iter(self) -> Values<'a, u64, ArcFeed> {
        self.inner.feeds()
    }
}

impl<'a> FeedsIterator<'a> {
    fn new(inner: RwLockReadGuard<'a, InnerCorestore>) -> Self {
        Self { inner }
    }
}

struct InnerCorestore {
    master_key: Key,
    feeds: FeedMap,
    #[allow(dead_code)]
    name: String,
    storage_path: PathBuf,
    subscribers: Vec<mpsc::Sender<Event>>,
}

impl InnerCorestore {
    /// Open a corestore from disk
    pub async fn open<P>(storage_path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let storage_path = storage_path.as_ref().to_path_buf();
        Self::with_storage_path(storage_path).await
    }

    async fn with_storage_path(path: PathBuf) -> Result<Self> {
        let master_key_path = path.join(MASTER_KEY_FILENAME);
        let mut master_key_storage = random_access_disk(master_key_path).await?;
        let master_key: Key = match master_key_storage.read(0, 32).await {
            Ok(key) => key.try_into().unwrap(),
            Err(_) => {
                let key = generate_key();
                // TODO: Map err, not unwrap.
                master_key_storage.write(0, &key[..32]).await.unwrap();
                key
            }
        };
        Ok(Self {
            master_key,
            feeds: FeedMap::new(),
            name: "default".into(),
            storage_path: path,
            subscribers: vec![],
        })
    }

    async fn emit(&mut self, event: Event) {
        for sender in self.subscribers.iter_mut() {
            sender.send(event.clone()).await.unwrap();
        }
    }

    /// Subscribe to events from this corestore
    pub async fn subscribe(&mut self) -> mpsc::Receiver<Event> {
        let (send, recv) = mpsc::channel(100);
        self.subscribers.push(send);
        recv
    }

    /// Get a feed by its public key
    pub async fn get_by_key(&mut self, key: Key) -> Result<ArcFeed> {
        self.open_feed(Some(key), None).await
    }

    /// Get or create a writable feed by name
    pub async fn get_by_name<T>(&mut self, name: T) -> Result<ArcFeed>
    where
        T: AsRef<str>,
    {
        let name = name.as_ref().as_bytes().to_vec();
        self.open_feed(None, Some(name)).await
    }

    /// Get a feed by its discovery key
    ///
    /// This only works if the feed is found on disk
    pub async fn get_by_dkey(&mut self, dkey: Key) -> Result<Option<ArcFeed>> {
        if let Some(feed) = self.feeds.get_dkey(&dkey) {
            return Ok(Some(feed.clone()));
        }

        // Check if the feed exists on disk.
        let key = {
            let mut key_storage = self.feed_storage(&dkey, "key").await?;
            key_storage.read(0, 32).await
        };
        let feed = match key {
            Ok(key) => Some(self.get_by_key(key.try_into().unwrap()).await?),
            Err(_) => None,
        };
        Ok(feed)
    }

    /// Iterate over all feeds
    ///
    /// Returns an iterator of `ArcFeed`s
    pub fn feeds(&self) -> Values<u64, ArcFeed> {
        self.feeds.iter()
    }

    async fn open_feed(&mut self, key: Option<Key>, name: Option<Name>) -> Result<ArcFeed> {
        if let Some(feed) = key.as_ref().map(|k| self.feeds.get_key(&k)).flatten() {
            return Ok((*feed).clone());
        }
        if let Some(feed) = name.as_ref().map(|n| self.feeds.get_name(&n)).flatten() {
            return Ok((*feed).clone());
        }
        debug!(
            "open feed key {:?} name {:?} dkey {:?}",
            key.as_ref().map(|k| hex::encode(k)),
            name,
            key.as_ref().map(|k| hex::encode(discovery_key(k)))
        );
        // When opening a feed by key without a name, check if a name exists.
        let name = match (&key, name) {
            (_, Some(name)) => Some(name),
            (None, None) => None,
            (Some(key), None) => {
                let dkey = discovery_key(&key[..]);
                self.read_name(&dkey).await?
            }
        };

        let (public_key, secret_key, generated_name) = match (key, name) {
            (_, Some(name)) => self.generate_key_pair(Some(name)),
            (None, None) => self.generate_key_pair(None),
            (Some(key), None) => (pubkey_from_bytes(&key)?, None, None),
        };

        let dkey = discovery_key(public_key.as_bytes());

        if let Some(name) = &generated_name {
            self.write_name_if_empty(&dkey, name).await?;
        }

        let dir = self.feed_path(&dkey);
        let feed_storage = storage_disk(&dir).await?;
        let builder = Feed::builder(public_key.clone(), feed_storage);
        let builder = if let Some(secret_key) = secret_key {
            builder.secret_key(secret_key)
        } else {
            builder
        };

        let feed = builder.build().await?;

        debug!(
            "open feed, key: {}",
            hex::encode(feed.public_key().as_bytes())
        );

        let arc_feed = self.feeds.insert(feed, generated_name);
        self.emit(Event::Feed(arc_feed.clone())).await;
        Ok(arc_feed)
    }

    fn feed_path(&self, dkey: &Key) -> PathBuf {
        let hdkey = hex::encode(dkey);
        let mut path = self.storage_path.clone();
        path.push(&hdkey[..2]);
        path.push(&hdkey[2..4]);
        path.push(&hdkey);
        path
    }

    async fn feed_storage(&self, dkey: &Key, name: &str) -> Result<RandomAccessDisk> {
        let mut path = self.feed_path(dkey);
        path.push(name);
        random_access_disk(path).await
    }

    async fn name_storage(&self, dkey: &Key) -> Result<RandomAccessDisk> {
        self.feed_storage(&dkey, "name").await
    }

    async fn read_name(&self, dkey: &Key) -> Result<Option<Name>> {
        let mut name_storage = self.name_storage(dkey).await?;
        let len = name_storage.len().await.unwrap_or(0);
        if len > 0 {
            // TODO: Map error.
            let name = name_storage.read(0, len).await.unwrap();
            Ok(Some(name))
        } else {
            Ok(None)
        }
    }
    async fn write_name_if_empty(&self, dkey: &Key, name: &Name) -> Result<bool> {
        let mut name_storage = self.name_storage(dkey).await?;
        let len = name_storage.len().await.unwrap_or(0);
        if len > 0 {
            Ok(false)
        } else {
            // TODO: Map error.
            name_storage.write(0, name).await.unwrap();
            Ok(true)
        }
    }

    fn derive_secret(&self, namespace: &Name, name: &Name) -> Key {
        derive_key(&self.master_key, namespace, name)
    }

    fn generate_key_pair(
        &self,
        name: Option<Name>,
    ) -> (PublicKey, Option<SecretKey>, Option<Name>) {
        let name = name.or_else(|| Some(generate_key().to_vec())).unwrap();
        let seed = self.derive_secret(&NAMESPACE.as_bytes().to_vec(), &name);
        let secret_key = SecretKey::from_bytes(&seed).unwrap();
        let public_key: PublicKey = (&secret_key).into();
        (public_key, Some(secret_key), Some(name))
    }
}

fn pubkey_from_bytes(bytes: &Key) -> Result<PublicKey> {
    PublicKey::from_bytes(bytes).map_err(|e| e.into())
}

fn derive_key(master_key: &[u8], ns: &[u8], name: &[u8]) -> Key {
    let mut hasher = Blake2b::with_key(32, master_key);
    hasher.update(ns);
    hasher.update(name);
    hasher.finalize().as_bytes().try_into().unwrap()
}

fn generate_key() -> Key {
    let mut rng = StdRng::from_rng(OsRng::default()).unwrap();
    let mut key = [0u8; 32];
    rng.fill_bytes(&mut key);
    key
}

// TODO: Make this a callback (for e.g. inmemory).
async fn random_access_disk(path: PathBuf) -> Result<RandomAccessDisk> {
    RandomAccessDisk::open(path).await
}
