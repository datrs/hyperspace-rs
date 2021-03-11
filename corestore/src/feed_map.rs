use async_std::sync::{Arc, Mutex};
use hypercore::Feed;
use hypercore_replicator::discovery_key;
use std::collections::{hash_map::Values, HashMap};

use crate::corestore::{ArcFeed, Key, Name};

#[derive(Default)]
pub struct FeedMap {
    feeds: HashMap<u64, ArcFeed>,
    keys: HashMap<Key, u64>,
    dkeys: HashMap<Key, u64>,
    names: HashMap<Name, u64>,
    names_rev: HashMap<u64, Name>,
}

impl FeedMap {
    pub fn new() -> Self {
        Self {
            feeds: HashMap::new(),
            keys: HashMap::new(),
            dkeys: HashMap::new(),
            names: HashMap::new(),
            names_rev: HashMap::new(),
        }
    }
    pub fn get_name(&self, name: &Name) -> Option<&ArcFeed> {
        self.names.get(name).map(|id| self.feeds.get(id)).flatten()
    }
    pub fn get_key(&self, key: &Key) -> Option<&ArcFeed> {
        self.keys.get(key).map(|id| self.feeds.get(id)).flatten()
    }
    pub fn get_dkey(&self, dkey: &Key) -> Option<&ArcFeed> {
        self.dkeys.get(dkey).map(|id| self.feeds.get(id)).flatten()
    }

    pub fn insert(&mut self, feed: Feed, name: Option<Name>) -> ArcFeed {
        let key = feed.public_key().to_bytes();
        let dkey = discovery_key(&key);
        let feed = Arc::new(Mutex::new(feed));
        let id = self.feeds.len() as u64;
        self.feeds.insert(id, feed.clone());
        self.keys.insert(key, id);
        self.dkeys.insert(dkey, id);
        if let Some(name) = name {
            self.names.insert(name.clone(), id);
            self.names_rev.insert(id, name);
        }
        feed
    }

    pub fn iter(&self) -> Values<u64, ArcFeed> {
        self.feeds.values()
    }

    pub async fn _remove(&mut self, feed: ArcFeed) {
        let feed = feed.lock().await;
        let key = feed.public_key().to_bytes();
        let dkey = discovery_key(&key);
        let id = self.keys.get(&key);
        if id.is_none() {
            return;
        }
        let id = id.unwrap().clone();
        self.feeds.remove(&id);
        self.keys.remove(&key);
        self.dkeys.remove(&dkey);
        if let Some(name) = self.names_rev.get(&id) {
            self.names.remove(name);
            self.names_rev.remove(&id);
        }
    }
}
