use std::{
    env::current_exe, fmt::{Debug, Display}, fs::{read, File}, io::Write, path::Path, sync::{atomic::{AtomicUsize, Ordering::SeqCst}, LazyLock}
};

use atomic_write_file::{unix::OpenOptionsExt, AtomicWriteFile};
use bincode::config::standard;
use dashmap::DashMap;
use jane_eyre::eyre::{self, bail, Context as _};
use rayon::{iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator as _}, Scope, ThreadPool, ThreadPoolBuilder};
use serde::{de::Visitor, Deserialize, Serialize};
use tracing::{debug, warn};

use crate::{
    path::{DynamicPath, POSTS_PATH_ROOT},
    render_markdown, FilteredPost, Thread, UnsafePost,
};

pub static HASHER: LazyLock<blake3::Hasher> = LazyLock::new(|| {
    let mut hasher = blake3::Hasher::new();
    let exe = current_exe().expect("failed to get path to executable");
    hasher
        .update_mmap_rayon(exe)
        .expect("failed to hash executable");
    hasher.update(hasher.finalize().as_bytes());
    hasher
});

static FILTERED_POST_CACHE: LazyLock<DashMap<Id, FilteredPost>> = LazyLock::new(DashMap::new);
struct Context {
    output_cache: MemoryCache<Id, Vec<u8>>,
    derivation_cache: MemoryCache<Id, Derivation>,
    output_writer_pool: ThreadPool,
    derivation_writer_pool: ThreadPool,
}
struct ContextGuard<'ctx, 'scope> {
    output_cache: &'ctx MemoryCache<Id, Vec<u8>>,
    derivation_cache: &'ctx MemoryCache<Id, Derivation>,
    output_writer_scope: &'ctx Scope<'scope>,
    derivation_writer_scope: &'ctx Scope<'scope>,
}
impl Context {
    fn new() -> Self {
        Self {
            output_writer_pool: ThreadPoolBuilder::new().thread_name(|i| format!("outWriter{i}")).build().expect("failed to build thread pool"),
            derivation_writer_pool: ThreadPoolBuilder::new().thread_name(|i| format!("drvWriter{i}")).build().expect("failed to build thread pool"),
            output_cache: MemoryCache::new("output"),
            derivation_cache: MemoryCache::new("derivation"),
        }
    }
    fn run<R: Send>(fun: impl FnOnce(ContextGuard) -> R + Send) -> R {
        Self::new().scope(fun)
    }
    fn scope<R: Send>(&self, fun: impl FnOnce(ContextGuard) -> R + Send) -> R {
        self.output_writer_pool.scope(move |output_writer_scope| {
            self.derivation_writer_pool.scope(move |derivation_writer_scope| {
                fun(ContextGuard {
                    output_cache: &self.output_cache,
                    derivation_cache: &self.derivation_cache,
                    output_writer_scope,
                    derivation_writer_scope,
                })
            })
        })
    }
}
struct MemoryCache<K, V> {
    inner: DashMap<K, V>,
    label: &'static str,
    hits: AtomicUsize,
    read_misses: AtomicUsize,
    read_write_misses: AtomicUsize,
    write_write_misses: AtomicUsize,
}
impl<K: Eq + std::hash::Hash, V> Debug for MemoryCache<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MemoryCache {} (len {}, hits {}, reads {}, read writes {}, write writes {})", self.label, self.inner.len(), self.hits.load(SeqCst), self.read_misses.load(SeqCst), self.read_write_misses.load(SeqCst), self.write_write_misses.load(SeqCst))
    }
}
impl<K: Eq + std::hash::Hash + Debug, V: Clone> MemoryCache<K, V> {
    fn new(label: &'static str) -> Self {
        Self {
            inner: DashMap::new(),
            label,
            hits: AtomicUsize::new(0),
            read_misses: AtomicUsize::new(0),
            read_write_misses: AtomicUsize::new(0),
            write_write_misses: AtomicUsize::new(0),
        }
    }
    fn get_or_insert_as_read(&self, key: K, default: impl FnOnce(&K) -> eyre::Result<V>) -> eyre::Result<V> {
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
    fn get_or_insert_as_write(&self, key: K, read: impl FnOnce(&K) -> eyre::Result<V>, write: impl FnOnce(&K) -> eyre::Result<V>) -> eyre::Result<V> {
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

pub fn hash_bytes(bytes: impl AsRef<[u8]>) -> blake3::Hash {
    HASHER.clone().update(bytes.as_ref()).finalize()
}

pub fn hash_file(path: impl AsRef<Path>) -> eyre::Result<blake3::Hash> {
    let mut hasher = HASHER.clone();
    hasher.update_mmap_rayon(path)?;

    Ok(hasher.finalize())
}

pub fn parse_hash_hex(input: &str) -> eyre::Result<blake3::Hash> {
    Ok(blake3::Hash::from_hex(input)?)
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct Hash(blake3::Hash);
impl PartialOrd for Hash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Hash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}
impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            let hash = self.0.to_hex();
            write!(f, "{}...", &hash.as_str()[0..13])
        } else {
            write!(f, "{}", self.0.to_hex().as_str())
        }
    }
}
impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.to_hex().as_str())
    }
}
impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(HashVisitor)
    }
}
struct HashVisitor;
impl<'de> Visitor<'de> for HashVisitor {
    type Value = Hash;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string that is 64 hex digits")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let result = blake3::Hash::from_hex(v)
            .map_err(|e| E::custom(format!("failed to parse hash: {e:?}")))?;
        Ok(Hash(result))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct Id(Hash);
trait ComputeId {
    fn compute_id(&self) -> Id;
}
impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
enum Builder {
    ReadFile {
        path: DynamicPath,
        hash: Hash,
    },
    RenderMarkdown {
        file: Box<Derivation>,
    },
    FilteredPost {
        file: Box<Derivation>,
    },
    Thread {
        post: Box<Derivation>,
        references: Vec<Derivation>,
    },
}
impl Display for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Builder::ReadFile { path, hash } => f
                .debug_struct("ReadFile")
                .field("path", &UseDisplay(path))
                .field("hash", &UseDisplay(hash))
                .finish(),
            Builder::RenderMarkdown { file } => f
                .debug_struct("RenderMarkdown")
                .field("file", &UseDisplay(&**file))
                .finish(),
            Builder::FilteredPost { file } => f
                .debug_struct("FilteredPost")
                .field("file", &UseDisplay(&**file))
                .finish(),
            Builder::Thread { post, references } => f
                .debug_struct("Thread")
                .field("post", &UseDisplay(&**post))
                .field("references", &VecDisplay(references))
                .finish(),
        }
    }
}
impl ComputeId for Builder {
    fn compute_id(&self) -> Id {
        let result = bincode::serde::encode_to_vec(self, standard())
            .expect("guaranteed by derive Serialize");
        Id(Hash(blake3::hash(&result)))
    }
}
#[derive(Debug)]
struct RenderMarkdownInput {
    file: Vec<u8>,
}
#[derive(Debug)]
struct FilteredPostInput {
    file: Vec<u8>,
}
#[derive(Debug)]
struct ThreadInput {
    post: FilteredPost,
    references: Vec<FilteredPost>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Derivation {
    output: Id,
    builder: Builder,
}
impl Display for Derivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Derivation({} -> {})", self.output.0, self.builder)
    }
}
struct UseDisplay<'d, D: Display>(&'d D);
struct VecDisplay<'d, D: Display>(&'d [D]);
impl<'d, D: Display> Debug for UseDisplay<'d, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl<'d, D: Display> Debug for VecDisplay<'d, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|value| UseDisplay(value)))
            .finish()
    }
}
mod private {
    use bincode::config::standard;
    use jane_eyre::eyre;
    use tracing::warn;

    use crate::cache::{atomic_writer, Builder, ComputeId as _, ContextGuard, Derivation};

    impl Derivation {
        pub fn instantiate(ctx: &ContextGuard, builder: Builder) -> eyre::Result<Self> {
            let output = builder.compute_id();
            Self { output, builder }.store(ctx)
        }

        fn store(self, ctx: &ContextGuard) -> eyre::Result<Self> {
            ctx.derivation_cache.get_or_insert_as_write(self.id(), |id| Self::load(ctx, *id), |id| {
                let path = Self::derivation_path(id);
                let self_for_write = self.clone();
                ctx.derivation_writer_scope.spawn(move |_| {
                    let result = || -> eyre::Result<()> {
                        let mut file = atomic_writer(path)?;
                        bincode::serde::encode_into_std_write(self_for_write, &mut file, standard())?;
                        file.commit()?;
                        Ok(())
                    }();
                    if let Err(error) = result {
                        warn!(?error, "failed to write derivation");
                    }
                });
                Ok(self)
            })
        }
    }
}
impl Derivation {
    fn read_file(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let hash = Hash(blake3::hash(&read(&path)?));
        Self::instantiate(ctx, Builder::ReadFile { path, hash })
    }

    fn render_markdown(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        Self::instantiate(ctx, Builder::RenderMarkdown {
            file: Self::read_file(ctx, path)?.into(),
        })
    }

    fn filtered_post(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let DynamicPath::Posts(posts_path) = &path else {
            bail!("path is not a posts path")
        };
        let file = if posts_path.is_markdown_post() {
            Self::render_markdown(ctx, path)?
        } else {
            Self::read_file(ctx, path)?
        };
        Self::instantiate(ctx, Builder::FilteredPost {
            file: file.into(),
        })
    }

    fn thread(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let post_derivation = Self::filtered_post(ctx, path)?;
        // TODO: can we avoid realise() during evaluation?
        // (probably not, because it’s like we’re forced to do an IFD in this situation?)
        let post = if let Some(post) = FILTERED_POST_CACHE.get(&post_derivation.id()) {
            post.clone()
        } else {
            let post = Self::load(ctx, post_derivation.id())?.output(ctx)?;
            bincode::serde::decode_from_slice(&post, standard())?.0
        };
        let references = post.meta.front_matter.references
            .par_iter()
            .map(|path| Self::filtered_post(ctx, path.to_dynamic_path()))
            .collect::<eyre::Result<Vec<_>>>()?;
        Self::instantiate(ctx, Builder::Thread {
            post: post_derivation.into(),
            references,
        })
    }

    fn id(&self) -> Id {
        self.output
    }

    fn derivation_path(id: &Id) -> String {
        format!("cache/{id}.drv")
    }

    fn output_path(&self) -> String {
        format!("cache/{}.out", self.id())
    }

    fn needs(&self) -> Vec<&Derivation> {
        match &self.builder {
            Builder::ReadFile { .. } => vec![],
            Builder::RenderMarkdown { file } => vec![&**file],
            Builder::FilteredPost { file } => vec![&**file],
            Builder::Thread { post, references } => {
                let mut result = vec![&**post];
                result.extend(references.iter());
                result
            }
        }
    }

    fn load(ctx: &ContextGuard, id: Id) -> eyre::Result<Self> {
        ctx.derivation_cache.get_or_insert_as_read(id, |id| {
            Ok(bincode::serde::decode_from_std_read(
                &mut File::open(Self::derivation_path(id))?,
                standard(),
            )?)
        })
    }

    fn output(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        ctx.output_cache.get_or_insert_as_read(self.id(), |_id| {
            Ok(read(self.output_path())?)
        })
    }

    fn realise(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        ctx.output_cache.get_or_insert_as_write(self.id(), |_id| Ok(read(self.output_path())?), |_id| {
            debug!("building {self}");
            let result = (|| {
                let content = match &self.builder {
                    Builder::ReadFile { path, hash } => {
                        let output = read(path)?;
                        let actual_hash = Hash(blake3::hash(&output));
                        if &actual_hash != hash {
                            bail!("hash mismatch! expected {hash}, actual {actual_hash}");
                        }
                        output
                    }
                    Builder::RenderMarkdown { file } => {
                        let input = RenderMarkdownInput {
                            file: Self::load(ctx, file.id())?.output(ctx)?,
                        };
                        let unsafe_markdown = input.file;
                        render_markdown(str::from_utf8(&unsafe_markdown)?).into_bytes()
                    }
                    Builder::FilteredPost { file } => {
                        let input = FilteredPostInput {
                            file: Self::load(ctx, file.id())?.output(ctx)?,
                        };
                        let unsafe_html = input.file;
                        let unsafe_html = str::from_utf8(&unsafe_html)?;
                        let post = UnsafePost::with_html(unsafe_html);
                        let post = FilteredPost::filter(post)?;
                        let output = bincode::serde::encode_to_vec(&post, standard())?;
                        FILTERED_POST_CACHE.insert(self.id(), post);
                        output
                    }
                    Builder::Thread { post, references } => {
                        let load_filtered_post_cached = |id| -> eyre::Result<_> {
                            if let Some(post) = FILTERED_POST_CACHE.get(&id) {
                                Ok(post.clone())
                            } else {
                                let post = Self::load(ctx, id)?.output(ctx)?;
                                Ok(bincode::serde::decode_from_slice(&post, standard())?.0)
                            }
                        };
                        let input = ThreadInput {
                            post: load_filtered_post_cached(post.id())?,
                            references: references
                                .iter()
                                .map(|post| load_filtered_post_cached(post.id()))
                                .collect::<eyre::Result<_>>()?,
                        };
                        let thread = Thread::new(input.post, input.references);
                        bincode::serde::encode_to_vec(&thread, standard())?
                    }
                };
                let output_path = self.output_path();
                let content_for_write = content.clone();
                ctx.output_writer_scope.spawn(move |_| {
                    if let Err(error) = atomic_write(output_path, content_for_write) {
                        warn!(?error, "failed to write derivation output");
                    }
                });
                Ok(content)
            })();
            result.wrap_err_with(|| format!("failed to realise derivation: {self:?}"))
        })
    }
}

pub async fn test() -> eyre::Result<()> {
    Context::run(|ctx| -> eyre::Result<()> {
        let top_level_post_paths = POSTS_PATH_ROOT.read_dir_flat()?;
        eprintln!("building filtered posts");
        top_level_post_paths
            .par_iter()
            .map(|path| -> eyre::Result<()> {
                build(&ctx, &Derivation::filtered_post(&ctx, path.to_dynamic_path())?)
            })
            .collect::<eyre::Result<Vec<_>>>()?;
        eprintln!();
        eprintln!("building threads");
        top_level_post_paths
            .par_iter()
            .map(|path| -> eyre::Result<()> {
                build(&ctx, &Derivation::thread(&ctx, path.to_dynamic_path())?)
            })
            .collect::<eyre::Result<Vec<_>>>()?;
        eprintln!();
        eprintln!("waiting for thread pools");
        Ok(())
    })?;

    Ok(())
}

fn build(ctx: &ContextGuard, derivation: &Derivation) -> eyre::Result<()> {
    let _needs = derivation
        .needs()
        .into_par_iter()
        .map(|dependency| build(ctx, dependency))
        .collect::<eyre::Result<Vec<_>>>()?;
    derivation.realise(ctx)?;
    Ok(())
}

fn atomic_writer(path: impl AsRef<Path>) -> eyre::Result<AtomicWriteFile> {
    Ok(AtomicWriteFile::options()
        .preserve_mode(false)
        .preserve_owner(false)
        .try_preserve_owner(false)
        .open(path)?)
}

fn atomic_write(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> eyre::Result<()> {
    let mut file = atomic_writer(path)?;
    file.write_all(content.as_ref())?;
    file.commit()?;

    Ok(())
}
