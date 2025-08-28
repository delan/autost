use jane_eyre::eyre;
use tracing::debug;

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::mem::{replace, take};
use std::ops::Range;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{LazyLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::cache::Id;

pub const PACK_INDICES: Range<usize> = 0..4096;
pub static PACK_NAMES: LazyLock<Vec<String>> =
    LazyLock::new(|| PACK_INDICES.map(|i| format!("{i:03x}")).collect());

pub struct MemoryCache<K, V> {
    inner: Box<[RwLock<HashMap<K, V>>; 4096]>,
    label: &'static str,
    dirty: Box<[AtomicBool; 4096]>,
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

impl<V: Clone + Debug + Send + Sync> MemoryCache<Id, V> {
    pub fn new(label: &'static str) -> Self {
        let mut inner = vec![];
        inner.resize_with(4096, RwLock::default);

        Self {
            inner: inner.try_into().expect("guaranteed by receiver"),
            label,
            dirty: dirty_bits(),
            hits: AtomicUsize::new(0),
            read_misses: AtomicUsize::new(0),
            read_write_misses: AtomicUsize::new(0),
            write_write_misses: AtomicUsize::new(0),
        }
    }
    pub fn dirty(&self) -> &[AtomicBool; 4096] {
        &self.dirty
    }
    pub fn take(&mut self, pack_index: usize) -> HashMap<Id, V> {
        take(&mut self.write(pack_index))
    }
    pub fn restore(&mut self, pack_index: usize, pack: HashMap<Id, V>) {
        let _ = replace(&mut *self.write(pack_index), pack);
    }
    pub fn read(&self, pack_index: usize) -> RwLockReadGuard<'_, HashMap<Id, V>> {
        self.inner[pack_index].read().expect("poisoned")
    }
    pub fn write(&self, pack_index: usize) -> RwLockWriteGuard<'_, HashMap<Id, V>> {
        self.inner[pack_index].write().expect("poisoned")
    }
    pub fn get_or_insert_as_read(
        &self,
        key: Id,
        default: impl FnOnce(&Id) -> eyre::Result<V>,
    ) -> eyre::Result<V> {
        debug!(?self, "query");
        let pack_index = key.pack_index();
        if let Some(value) = self.read(pack_index).get(&key) {
            self.hits.fetch_add(1, SeqCst);
            Ok(value.clone())
        } else {
            self.dirty[pack_index].store(true, SeqCst);
            self.read_misses.fetch_add(1, SeqCst);
            let value = default(&key)?;
            self.write(pack_index).insert(key, value.clone());
            Ok(value)
        }
    }
    pub fn get_or_insert_as_write(
        &self,
        key: Id,
        read: impl FnOnce(&Id) -> eyre::Result<V>,
        write: impl FnOnce(&Id) -> eyre::Result<V>,
    ) -> eyre::Result<V> {
        debug!(?self, "query");
        let pack_index = key.pack_index();
        if let Some(value) = self.read(pack_index).get(&key) {
            self.hits.fetch_add(1, SeqCst);
            return Ok(value.clone());
        }
        self.dirty[pack_index].store(true, SeqCst);
        let value = if let Ok(value) = read(&key) {
            self.read_write_misses.fetch_add(1, SeqCst);
            value
        } else {
            debug!(?self, ?key, "write");
            self.write_write_misses.fetch_add(1, SeqCst);
            write(&key)?
        };
        self.write(pack_index).insert(key, value.clone());
        Ok(value)
    }
}

pub fn pack_indices() -> impl DoubleEndedIterator<Item = usize> + ExactSizeIterator {
    PACK_INDICES
}

pub fn pack_names() -> impl DoubleEndedIterator<Item = &'static String> + ExactSizeIterator {
    PACK_NAMES.iter()
}

pub fn dirty_bits() -> Box<[AtomicBool; 4096]> {
    let mut dirty = vec![];
    dirty.resize_with(4096, AtomicBool::default);
    dirty.try_into().expect("guaranteed by definition")
}
