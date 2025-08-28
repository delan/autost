use bincode::config::standard;
use bincode::{Decode, Encode};
use jane_eyre::eyre::{self, eyre};
use tracing::debug;

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::mem::{replace, take};
use std::ops::Range;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{LazyLock, OnceLock, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::cache::Id;

pub const PACK_COUNT: usize = 4096;
pub const PACK_INDICES: Range<usize> = 0..PACK_COUNT;
pub static PACK_NAMES: LazyLock<Vec<String>> =
    LazyLock::new(|| PACK_INDICES.map(|i| format!("{i:03x}")).collect());

pub type CacheShard<K, V> = HashMap<K, Lazy<V>>;

pub struct MemoryCache<K, V> {
    inner: Box<[RwLock<CacheShard<K, V>>; PACK_COUNT]>,
    label: &'static str,
    dirty: Box<[AtomicBool; PACK_COUNT]>,
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

impl<V: Clone + Debug + Decode<()> + Encode + Send + Sync> MemoryCache<Id, V> {
    pub fn new(label: &'static str) -> Self {
        let mut inner = vec![];
        inner.resize_with(PACK_COUNT, RwLock::default);

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
    pub fn dirty(&self) -> &[AtomicBool; PACK_COUNT] {
        &self.dirty
    }
    pub fn take(&mut self, pack_index: usize) -> CacheShard<Id, V> {
        take(&mut self.write(pack_index))
    }
    pub fn restore(&mut self, pack_index: usize, pack: CacheShard<Id, V>) {
        let _ = replace(&mut *self.write(pack_index), pack);
    }
    pub fn read(&self, pack_index: usize) -> RwLockReadGuard<'_, CacheShard<Id, V>> {
        self.inner[pack_index].read().expect("poisoned")
    }
    pub fn write(&self, pack_index: usize) -> RwLockWriteGuard<'_, CacheShard<Id, V>> {
        self.inner[pack_index].write().expect("poisoned")
    }
    pub fn get_or_insert_as_read(
        &self,
        key: Id,
        default: impl FnOnce(&Id) -> eyre::Result<V>,
    ) -> eyre::Result<V> {
        debug!(?self, "query");
        let pack_index = key.pack_index();
        if let Some(lazy) = self.read(pack_index).get(&key) {
            self.hits.fetch_add(1, SeqCst);
            Ok(lazy.resolve()?.clone())
        } else {
            self.dirty[pack_index].store(true, SeqCst);
            self.read_misses.fetch_add(1, SeqCst);
            let value = default(&key)?;
            let mut pack = self.write(pack_index);
            pack.insert(key, Lazy::actual(value)?);
            let lazy = pack.get(&key).expect("guaranteed by insert");
            Ok(lazy.resolve()?.clone())
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
        if let Some(lazy) = self.read(pack_index).get(&key) {
            self.hits.fetch_add(1, SeqCst);
            return Ok(lazy.resolve()?.clone());
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
        let mut pack = self.write(pack_index);
        pack.insert(key, Lazy::actual(value)?);
        let lazy = pack.get(&key).expect("guaranteed by insert");
        Ok(lazy.resolve()?.clone())
    }
}

#[derive(Debug, Clone)]
pub struct Lazy<T> {
    pub(super) content: Vec<u8>,
    value: OnceLock<Result<T, String>>,
}

impl<T: Decode<()> + Encode> Lazy<T> {
    pub fn raw(content: Vec<u8>) -> Self {
        Self {
            content,
            value: OnceLock::default(),
        }
    }

    pub fn actual(value: T) -> eyre::Result<Self> {
        let content = bincode::encode_to_vec(&value, standard())?;
        let value = Ok(value);

        Ok(Self {
            content,
            value: value.into(),
        })
    }

    pub fn resolve(&self) -> eyre::Result<&T> {
        let result = self.value.get_or_init(|| {
            let result = bincode::decode_from_slice(&self.content, standard());
            Ok(result.map_err(|error| error.to_string())?.0)
        });

        result
            .as_ref()
            .map_err(|error| eyre!("failed to decode cached value: {error:?}"))
    }
}

pub fn pack_indices() -> impl DoubleEndedIterator<Item = usize> + ExactSizeIterator {
    PACK_INDICES
}

pub fn pack_names() -> impl DoubleEndedIterator<Item = &'static String> + ExactSizeIterator {
    PACK_NAMES.iter()
}

pub fn dirty_bits() -> Box<[AtomicBool; PACK_COUNT]> {
    let mut dirty = vec![];
    dirty.resize_with(PACK_COUNT, AtomicBool::default);
    dirty.try_into().expect("guaranteed by definition")
}
