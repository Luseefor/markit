use std::collections::VecDeque;

/// Small byte-budgeted LRU. Keys include the complete source, so collisions never
/// return another document. Oversized results are used but not retained.
pub(crate) struct Cache<K, V> {
    entries: VecDeque<(K, V, usize)>,
    weight: usize,
    limit: usize,
}

impl<K: PartialEq, V: Clone> Cache<K, V> {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            weight: 0,
            limit,
        }
    }
    pub fn get(&mut self, key: &K) -> Option<V> {
        let index = self
            .entries
            .iter()
            .position(|(candidate, _, _)| candidate == key)?;
        let entry = self.entries.remove(index)?;
        let value = entry.1.clone();
        self.entries.push_front(entry);
        Some(value)
    }
    pub fn insert(&mut self, key: K, value: V, weight: usize) {
        if weight > self.limit {
            return;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|(candidate, _, _)| candidate == &key)
        {
            if let Some((_, _, old)) = self.entries.remove(index) {
                self.weight -= old;
            }
        }
        while self.weight + weight > self.limit || self.entries.len() >= 128 {
            if let Some((_, _, old)) = self.entries.pop_back() {
                self.weight -= old;
            } else {
                break;
            }
        }
        self.weight += weight;
        self.entries.push_front((key, value, weight));
    }
}
