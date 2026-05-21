use std::collections::HashMap;

use hako::image::StatefulProtocol;

const MAX_SIZE: usize = 10;

pub struct ImageProtocolCache {
    map: HashMap<usize, StatefulProtocol>,
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
        }
    }

    pub fn insert(&mut self, index: usize, protocol: StatefulProtocol) {
        self.map.insert(index, protocol);

        if self.map.len() > MAX_SIZE {
            let furthest = self
                .map
                .keys()
                .copied()
                .max_by_key(|&i| i.abs_diff(index))
                .unwrap();
            self.map.remove(&furthest);
        }
    }

    pub fn take(&mut self, index: usize) -> Option<StatefulProtocol> {
        self.map.remove(&index)
    }

    pub fn contains(&self, index: usize) -> bool {
        self.map.contains_key(&index)
    }
}
