use std::{
    env::current_exe, fmt::{Debug, Display}, fs::{read, File}, io::Write, path::Path, sync::{atomic::{AtomicUsize, Ordering::SeqCst}, LazyLock}
};

use atomic_write_file::{unix::OpenOptionsExt, AtomicWriteFile};
use bincode::{config::standard, de::{BorrowDecoder, Decoder}, enc::Encoder, error::DecodeError, BorrowDecode, Decode, Encode};
use dashmap::DashMap;
use jane_eyre::eyre::{self, bail, Context as _};
use rayon::{in_place_scope, iter::{IntoParallelRefIterator, ParallelIterator as _}, Scope, ThreadPool, ThreadPoolBuilder};
use tracing::{debug, info, warn};

use crate::{
    path::{DynamicPath, POSTS_PATH_ROOT}, render_markdown, FilteredPost, Thread, UnsafePost
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
    output_writer_pool: ThreadPool,
    derivation_writer_pool: ThreadPool,
    read_file_derivation_cache: MemoryCache<Id, ReadFileDrv>,
    render_markdown_derivation_cache: MemoryCache<Id, RenderMarkdownDrv>,
    filtered_post_derivation_cache: MemoryCache<Id, FilteredPostDrv>,
    thread_derivation_cache: MemoryCache<Id, ThreadDrv>,
}
struct ContextGuard<'ctx, 'scope> {
    context: &'ctx Context,
    output_writer_scope: &'ctx Scope<'scope>,
    derivation_writer_scope: &'ctx Scope<'scope>,
}
impl Context {
    fn new() -> Self {
        let cpu_count = std::thread::available_parallelism().expect("failed to get cpu count").get();
        Self {
            output_cache: MemoryCache::new("output"),
            output_writer_pool: ThreadPoolBuilder::new().thread_name(|i| format!("outWriter{i}"))
                .num_threads(cpu_count * 4).build().expect("failed to build thread pool"),
            derivation_writer_pool: ThreadPoolBuilder::new().thread_name(|i| format!("drvWriter{i}"))
                .num_threads(cpu_count * 4).build().expect("failed to build thread pool"),
            read_file_derivation_cache: MemoryCache::new("ReadFileDrv"),
            render_markdown_derivation_cache: MemoryCache::new("RenderMarkdownDrv"),
            filtered_post_derivation_cache: MemoryCache::new("FilteredPostDrv"),
            thread_derivation_cache: MemoryCache::new("ThreadDrv"),
        }
    }
    fn run<R: Send>(fun: impl FnOnce(ContextGuard) -> R + Send) -> R {
        Self::new().scope(fun)
    }
    fn scope<R: Send>(&self, fun: impl FnOnce(ContextGuard) -> R + Send) -> R {
        self.output_writer_pool.scope(move |output_writer_scope| {
            self.derivation_writer_pool.scope(move |derivation_writer_scope| {
                fun(ContextGuard {
                    context: &self,
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
impl < __Context > Decode < __Context > for Hash
{
    fn decode<D: Decoder<Context = __Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        Ok(Self(blake3::Hash::from_bytes(Decode::decode(decoder)?)))
    }
}
impl < '__de, __Context >   BorrowDecode < '__de, __Context >
for Hash
{
    fn borrow_decode<D: BorrowDecoder<'__de, Context = __Context>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        Ok(Self(blake3::Hash::from_slice(BorrowDecode::borrow_decode(decoder)?).map_err(|e| DecodeError::OtherString(e.to_string()))?))
    }
}
impl  Encode for Hash
{
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), bincode::error::EncodeError> {
        Encode::encode(self.0.as_bytes(), encoder)
    }
}

#[derive(Clone, Copy, Debug, Decode, Encode, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct Id(Hash);
impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

trait Derivation: Debug + Display + Sized {
    fn function_name() -> &'static str;
    fn id(&self) -> Id;
    fn derivation_path(id: &Id) -> String {
        format!("cache/{id}.drv")
    }
    fn output_path(&self) -> String {
        format!("cache/{}.out", self.id())
    }
    fn output(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        ctx.context.output_cache.get_or_insert_as_read(self.id(), |_id| {
            Ok(read(self.output_path())?)
        })
    }
    /// same as [`Derivation::realise_recursive()`], but traced at info level.
    #[tracing::instrument(level = "info", name = "build", skip_all, fields(function = %Self::function_name(), id = %self.id()))]
    fn realise_recursive_info(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        info!("building");
        self.realise_recursive(ctx)
    }
    /// same as [`Derivation::realise_recursive()`], but traced at debug level.
    #[tracing::instrument(level = "debug", name = "build", skip_all, fields(function = %Self::function_name(), id = %self.id()))]
    fn realise_recursive_debug(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        debug!("building");
        self.realise_recursive(ctx)
    }
    fn realise_self_only(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        ctx.context.output_cache.get_or_insert_as_write(self.id(), |_id| Ok(read(self.output_path())?), |_id| {
            debug!("building {self}");
            let result = (|| -> eyre::Result<_> {
                let content = self.compute_output(ctx)?;
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

    fn derivation_cache<'ctx>(ctx: &'ctx Context) -> &'ctx MemoryCache<Id, Self>;
    /// only to be called by [`Derivation::realise_self_only()`]. do not call this method elsewhere.
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>>;
    /// implementations should call `dep.realise_recursive_debug(ctx)` for each dependency, then call `self.realise_self_only(ctx)`.
    /// in other words, the default impl where `Self` has no dependencies should be: `self.realise_self_only(ctx)`
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>>;
}
impl Derivation for ReadFileDrv {
    fn function_name() -> &'static str {
        "ReadFile"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache<'ctx>(ctx: &'ctx Context) -> &'ctx MemoryCache<Id, Self> {
        &ctx.read_file_derivation_cache
    }
    fn compute_output(&self, _ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        let output = read(&self.inner.path)?;
        let expected_hash = self.inner.hash;
        let actual_hash = Hash(blake3::hash(&output));
        if actual_hash != expected_hash {
            bail!("hash mismatch! expected {expected_hash}, actual {actual_hash}");
        }
        Ok(output)
    }
    #[tracing::instrument(skip_all, fields(id = %self.id()))]
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        self.realise_self_only(ctx)
    }
}
impl Derivation for RenderMarkdownDrv {
    fn function_name() -> &'static str {
        "RenderMarkdown"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache<'ctx>(ctx: &'ctx Context) -> &'ctx MemoryCache<Id, Self> {
        &ctx.render_markdown_derivation_cache
    }
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        let input = RenderMarkdownInput {
            file: ReadFileDrv::load(ctx, self.inner.file.id())?.output(ctx)?,
        };
        let unsafe_markdown = input.file;
        Ok(render_markdown(str::from_utf8(&unsafe_markdown)?).into_bytes())
    }
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        self.inner.file.realise_recursive_debug(ctx)?;
        self.realise_self_only(ctx)
    }
}
impl Derivation for FilteredPostDrv {
    fn function_name() -> &'static str {
        "FilteredPost"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache<'ctx>(ctx: &'ctx Context) -> &'ctx MemoryCache<Id, Self> {
        &ctx.filtered_post_derivation_cache
    }
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        let input = FilteredPostInput {
            file: match &self.inner {
                DoFilteredPost::Html(file) => ReadFileDrv::load(ctx, file.id())?.output(ctx)?,
                DoFilteredPost::Markdown(file) => RenderMarkdownDrv::load(ctx, file.id())?.output(ctx)?,
            },
        };
        let unsafe_html = input.file;
        let unsafe_html = str::from_utf8(&unsafe_html)?;
        let post = UnsafePost::with_html(unsafe_html);
        let post = FilteredPost::filter(post)?;
        let output = bincode::encode_to_vec(&post, standard())?;
        FILTERED_POST_CACHE.insert(self.id(), post);
        Ok(output)
    }
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        match &self.inner {
            DoFilteredPost::Html(file) => file.realise_recursive_debug(ctx)?,
            DoFilteredPost::Markdown(file) => file.realise_recursive_debug(ctx)?,
        };
        self.realise_self_only(ctx)
    }
}
impl Derivation for ThreadDrv {
    fn function_name() -> &'static str {
        "Thread"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache<'ctx>(ctx: &'ctx Context) -> &'ctx MemoryCache<Id, Self> {
        &ctx.thread_derivation_cache
    }
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        let load_filtered_post_cached = |id| -> eyre::Result<_> {
            if let Some(post) = FILTERED_POST_CACHE.get(&id) {
                Ok(post.clone())
            } else {
                let post = FilteredPostDrv::load(ctx, id)?.output(ctx)?;
                Ok(bincode::decode_from_slice(&post, standard())?.0)
            }
        };
        let input = ThreadInput {
            post: load_filtered_post_cached(self.inner.post.id())?,
            references: self.inner.references
                .iter()
                .map(|post| load_filtered_post_cached(post.id()))
                .collect::<eyre::Result<_>>()?,
        };
        let thread = Thread::new(input.post, input.references);
        Ok(bincode::encode_to_vec(&thread, standard())?)
    }
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Vec<u8>> {
        in_place_scope(|scope| {
            scope.spawn(move |_| {
                self.inner.post.realise_recursive_debug(ctx).unwrap();
            });
            for post in self.inner.references.iter() {
                scope.spawn(move |_| {
                    post.realise_recursive_debug(ctx).unwrap();
                });
            }
        });
        self.realise_self_only(ctx)
    }
}

trait DerivationInner: Clone + Debug + Display + Send + Decode<()> + Encode + 'static {
    fn compute_id(&self) -> Id {
        let result = bincode::encode_to_vec(self, standard())
            .expect("guaranteed by derive Serialize");
        Id(Hash(blake3::hash(&result)))
    }
}
impl DerivationInner for DoReadFile {}
impl DerivationInner for DoRenderMarkdown {}
impl DerivationInner for DoFilteredPost {}
impl DerivationInner for DoThread {}

impl Display for DoReadFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f
            .debug_struct("ReadFile")
            .field("path", &UseDisplay(&self.path))
            .field("hash", &UseDisplay(&self.hash))
            .finish()
    }
}
impl Display for DoRenderMarkdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f
            .debug_struct("RenderMarkdown")
            .field("file", &UseDisplay(&self.file))
            .finish()
    }
}
impl Display for DoFilteredPost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DoFilteredPost::Html(file) =>
                f
                    .debug_struct("FilteredPost")
                    .field("file", &UseDisplay(file))
                    .finish(),
            DoFilteredPost::Markdown(file) =>
                f
                    .debug_struct("FilteredPost")
                    .field("file", &UseDisplay(file))
                    .finish(),
        }
    }
}
impl Display for DoThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f
            .debug_struct("Thread")
            .field("post", &UseDisplay(&self.post))
            .field("references", &VecDisplay(&self.references))
            .finish()
    }
}

type ReadFileDrv = Drv<DoReadFile>;
type RenderMarkdownDrv = Drv<DoRenderMarkdown>;
type FilteredPostDrv = Drv<DoFilteredPost>;
type ThreadDrv = Drv<DoThread>;

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
struct DoReadFile {
    path: DynamicPath,
    hash: Hash,
}
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
struct DoRenderMarkdown {
    file: ReadFileDrv,
}
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
enum DoFilteredPost {
    Html(ReadFileDrv),
    Markdown(RenderMarkdownDrv),
}
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
struct DoThread {
    post: FilteredPostDrv,
    references: Vec<FilteredPostDrv>,
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
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
struct Drv<Inner> {
    output: Id,
    inner: Inner,
}
impl<Inner: Display> Display for Drv<Inner> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Derivation({} -> {})", self.output.0, self.inner)
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

    use crate::cache::{atomic_writer, ContextGuard, Drv, Derivation, DerivationInner};

    impl<Inner: DerivationInner> Drv<Inner> where Self: Derivation {
        pub fn instantiate(ctx: &ContextGuard, inner: Inner) -> eyre::Result<Self> {
            let output = inner.compute_id();
            Self { output, inner }.store(ctx)
        }

        fn store(self, ctx: &ContextGuard) -> eyre::Result<Self> {
            Self::derivation_cache(ctx.context).get_or_insert_as_write(self.id(), |id| Self::load(ctx, *id), |id| {
                let path = Self::derivation_path(id);
                let self_for_write = self.clone();
                ctx.derivation_writer_scope.spawn(move |_| {
                    let result = || -> eyre::Result<()> {
                        let mut file = atomic_writer(path)?;
                        bincode::encode_into_std_write(self_for_write, &mut file, standard())?;
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

impl ReadFileDrv {
    fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let hash = Hash(blake3::hash(&read(&path)?));
        Self::instantiate(ctx, DoReadFile { path, hash })
    }
}
impl RenderMarkdownDrv {
    fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        Self::instantiate(ctx, DoRenderMarkdown {
            file: ReadFileDrv::new(ctx, path)?.into(),
        })
    }
}
impl FilteredPostDrv {
    fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let DynamicPath::Posts(posts_path) = &path else {
            bail!("path is not a posts path")
        };
        let inner = if posts_path.is_markdown_post() {
            DoFilteredPost::Markdown(RenderMarkdownDrv::new(ctx, path)?.into())
        } else {
            DoFilteredPost::Html(ReadFileDrv::new(ctx, path)?.into())
        };
        Self::instantiate(ctx, inner)
    }
}
impl ThreadDrv {
    fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let post_derivation = FilteredPostDrv::new(ctx, path)?;
        let post = if let Some(post) = FILTERED_POST_CACHE.get(&post_derivation.id()) {
            post.clone()
        } else {
            // effectively an IFD
            let post = post_derivation.realise_recursive(ctx)?;
            bincode::decode_from_slice(&post, standard())?.0
        };
        let references = post.meta.front_matter.references
            .par_iter()
            .map(|path| FilteredPostDrv::new(ctx, path.to_dynamic_path()))
            .collect::<eyre::Result<Vec<_>>>()?;
        Self::instantiate(ctx, DoThread {
            post: post_derivation.into(),
            references,
        })
    }
}

impl<Inner: DerivationInner> Drv<Inner> where Self: Derivation {
    fn load(ctx: &ContextGuard, id: Id) -> eyre::Result<Self> {
        Self::derivation_cache(ctx.context).get_or_insert_as_read(id, |id| {
            Ok(bincode::decode_from_std_read(
                &mut File::open(Self::derivation_path(id))?,
                standard(),
            )?)
        })
    }
}

pub async fn test() -> eyre::Result<()> {
    Context::run(|ctx| -> eyre::Result<()> {
        let top_level_post_paths = POSTS_PATH_ROOT.read_dir_flat()?;
        eprintln!("building threads");
        top_level_post_paths
            .par_iter()
            .map(|path| -> eyre::Result<()> {
                ThreadDrv::new(&ctx, path.to_dynamic_path())?.realise_recursive_info(&ctx)?;
                Ok(())
            })
            .collect::<eyre::Result<Vec<_>>>()?;
        eprintln!();
        eprintln!("waiting for thread pools");
        Ok(())
    })?;

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
