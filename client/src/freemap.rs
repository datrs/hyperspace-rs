use parking_lot::RwLock;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;

#[derive(Clone)]
pub struct NamedMap<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    inner: Arc<RwLock<InnerMap<K, V>>>,
}

#[derive(Default)]
struct InnerMap<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    map: HashMap<u64, V>,
    name_id: HashMap<K, u64>,
    id_name: HashMap<u64, Vec<K>>,
    free: VecDeque<u64>,
}

impl<K, V> InnerMap<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            name_id: HashMap::new(),
            id_name: HashMap::new(),
            free: VecDeque::new(),
        }
    }
    pub fn insert(&mut self, value: V) -> u64 {
        let id = if let Some(id) = self.free.pop_front() {
            id
        } else {
            self.map.len() as u64 + 1
        };
        self.map.insert(id, value);
        id
    }

    pub fn insert_with_name(&mut self, value: V, name: K) -> u64 {
        let id = self.insert(value);
        self.set_name(id, name);
        id
    }

    pub fn set_name(&mut self, id: u64, key: K) {
        self.name_id.insert(key.clone(), id);
        self.id_name
            .entry(id)
            .and_modify(|names| names.push(key.clone()))
            .or_insert_with(|| vec![key]);
    }

    // pub fn remove(&mut self, id: &u64) -> Option<V> {
    //     if let Some(item) = self.map.remove(id) {
    //         if let Some(names) = self.id_name.get(id) {
    //             for name in names {
    //                 self.name_id.remove(name);
    //             }
    //             self.id_name.remove(id);
    //         }
    //         self.free.push_back(*id);
    //         Some(item)
    //     } else {
    //         None
    //     }
    // }
}

impl<K, V> NamedMap<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    pub fn new() -> Self {
        let inner = InnerMap::new();
        let inner = Arc::new(RwLock::new(inner));
        Self { inner }
    }

    // pub fn insert(&mut self, value: V) -> u64 {
    //     self.inner.write().insert(value)
    // }

    pub fn insert_with_name(&mut self, value: V, name: K) -> u64 {
        self.inner.write().insert_with_name(value, name)
    }

    pub fn get(&self, id: u64) -> Option<V> {
        let inner = self.inner.read();
        let value = inner.map.get(&id).map(|v| v.clone());
        value
    }

    pub fn get_by_name(&self, key: &K) -> Option<V> {
        let inner = self.inner.read();
        let id = inner.name_id.get(&key);
        match id {
            None => None,
            Some(id) => inner.map.get(id).map(|v| v.clone()),
        }
    }

    pub fn set_name(&mut self, id: u64, key: K) {
        let mut inner = self.inner.write();
        inner.set_name(id, key);
    }

    // pub fn remove(&mut self, id: &u64) -> Option<V> {
    //     self.inner.write().remove(id)
    // }
}
