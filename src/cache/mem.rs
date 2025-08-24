use dashmap::DashMap;
use jane_eyre::eyre;
use tracing::{debug, warn};

use std::fmt::Debug;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;

pub(crate) struct MemoryCache<K, V> {
    pub(crate) inner: DashMap<K, V>,
    pub(crate) label: &'static str,
    pub(crate) hits: AtomicUsize,
    pub(crate) read_misses: AtomicUsize,
    pub(crate) read_write_misses: AtomicUsize,
    pub(crate) write_write_misses: AtomicUsize,
}

impl<K: Eq + std::hash::Hash, V> Debug for MemoryCache<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MemoryCache {} (len {}, hits {}, reads {}, read writes {}, write writes {})",
            self.label,
            self.inner.len(),
            self.hits.load(SeqCst),
            self.read_misses.load(SeqCst),
            self.read_write_misses.load(SeqCst),
            self.write_write_misses.load(SeqCst)
        )
    }
}

impl<K: Eq + std::hash::Hash + Debug, V: Clone> MemoryCache<K, V> {
    pub(crate) fn new(label: &'static str) -> Self {
        Self {
            inner: DashMap::new(),
            label,
            hits: AtomicUsize::new(0),
            read_misses: AtomicUsize::new(0),
            read_write_misses: AtomicUsize::new(0),
            write_write_misses: AtomicUsize::new(0),
        }
    }
    pub(crate) fn get_or_insert_as_read(
        &self,
        key: K,
        default: impl FnOnce(&K) -> eyre::Result<V>,
    ) -> eyre::Result<V> {
        debug!(target: "autost::cache::memory", ?self, "query");
        if let Some(value) = self.inner.get(&key) {
            self.hits.fetch_add(1, SeqCst);
            Ok(value.clone())
        } else {
            self.read_misses.fetch_add(1, SeqCst);
            let value = default(&key)?;
            self.inner.insert(key, value.clone());
            Ok(value)
        }
    }
    pub(crate) fn get_or_insert_as_write(
        &self,
        key: K,
        read: impl FnOnce(&K) -> eyre::Result<V>,
        write: impl FnOnce(&K) -> eyre::Result<V>,
    ) -> eyre::Result<V> {
        debug!(target: "autost::cache::memory", ?self, "query");
        if let Some(value) = self.inner.get(&key) {
            self.hits.fetch_add(1, SeqCst);
            return Ok(value.clone());
        }
        let value = if let Ok(value) = read(&key) {
            self.read_write_misses.fetch_add(1, SeqCst);
            value
        } else {
            warn!(target: "autost::cache::memory", ?self, ?key, "write");
            self.write_write_misses.fetch_add(1, SeqCst);
            write(&key)?
        };
        self.inner.insert(key, value.clone());
        Ok(value)
    }
}
