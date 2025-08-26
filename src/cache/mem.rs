use dashmap::DashMap;
use jane_eyre::eyre;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};
use tracing::debug;

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::str::FromStr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;

use crate::cache::Id;

pub struct MemoryCache<K, V> {
    inner: DashMap<K, V>,
    label: &'static str,
    hits: AtomicUsize,
    read_misses: AtomicUsize,
    read_write_misses: AtomicUsize,
    write_write_misses: AtomicUsize,
}

impl<K: Eq + Hash, V> Debug for MemoryCache<K, V> {
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

impl<K: Eq + Hash + Debug + Ord + Send, V: Clone + Send> MemoryCache<K, V> {
    pub fn new(label: &'static str) -> Self {
        Self {
            inner: DashMap::new(),
            label,
            hits: AtomicUsize::new(0),
            read_misses: AtomicUsize::new(0),
            read_write_misses: AtomicUsize::new(0),
            write_write_misses: AtomicUsize::new(0),
        }
    }
    pub fn encodable(self) -> BTreeMap<K, V> {
        self.inner.into_par_iter().collect()
    }
    pub fn get_or_insert_as_read(
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
    pub fn get_or_insert_as_write(
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
            debug!(target: "autost::cache::memory", ?self, ?key, "write");
            self.write_write_misses.fetch_add(1, SeqCst);
            write(&key)?
        };
        self.inner.insert(key, value.clone());
        Ok(value)
    }
}
impl<V: Clone + Debug + Send> MemoryCache<Id, V> {
    pub fn encodable_sharded(self) -> BTreeMap<String, BTreeMap<Id, V>> {
        let mut encodable = self.encodable();
        let splits = (0..256).rev().map(|i| format!("{i:02x}")).map(|prefix| {
            (
                prefix.clone(),
                Id::from_str(&format!("{prefix:<64}").replace(" ", "0")).unwrap(),
            )
        });
        splits
            .into_iter()
            .map(|(name, key)| (name.to_owned(), encodable.split_off(&key)))
            .collect()
    }
}
