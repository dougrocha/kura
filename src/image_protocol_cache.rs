use std::collections::HashMap;

use hako::image::StatefulProtocol;

const MAX_SIZE: usize = 10;

pub struct ImageProtocolCache {
    map: HashMap<String, StatefulProtocol>,
    /// Insertion order for eviction
    order: Vec<String>,
}

impl Default for ImageProtocolCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageProtocolCache {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub fn insert(&mut self, hash: String, protocol: StatefulProtocol) {
        if !self.map.contains_key(&hash) {
            self.order.push(hash.clone());
        }
        self.map.insert(hash, protocol);

        while self.map.len() > MAX_SIZE {
            let evict = self.order.remove(0);
            self.map.remove(&evict);
        }
    }

    pub fn take(&mut self, hash: &str) -> Option<StatefulProtocol> {
        if let Some(protocol) = self.map.remove(hash) {
            self.order.retain(|h| h != hash);
            Some(protocol)
        } else {
            None
        }
    }

    pub fn contains(&self, hash: &str) -> bool {
        self.map.contains_key(hash)
    }
}
