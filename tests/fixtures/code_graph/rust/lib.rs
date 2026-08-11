//! Fixture crate used by the code-graph regression baseline.

use std::collections::HashMap;

pub const MAX_ENTRIES: usize = 128;

pub type Registry = HashMap<String, Entry>;

/// A single cache entry.
pub struct Entry {
    pub key: String,
    pub hits: u32,
}

pub enum Status {
    Fresh,
    Stale,
}

pub trait Store {
    fn get(&self, key: &str) -> Option<&Entry>;
    fn put(&mut self, entry: Entry);
}

pub struct MemoryStore {
    entries: Registry,
}

impl Store for MemoryStore {
    fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    fn put(&mut self, entry: Entry) {
        self.entries.insert(entry.key.clone(), entry);
    }
}

pub fn build_store() -> MemoryStore {
    MemoryStore {
        entries: Registry::new(),
    }
}

pub fn warm_cache(store: &mut MemoryStore, keys: &[String]) -> usize {
    let mut count = 0;
    for key in keys {
        if store.get(key).is_none() {
            store.put(Entry {
                key: key.clone(),
                hits: 0,
            });
            count += 1;
        }
    }
    count
}

pub mod util {
    pub fn normalize(raw: &str) -> String {
        raw.trim().to_lowercase()
    }
}
