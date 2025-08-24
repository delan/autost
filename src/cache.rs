use std::{
    fmt::{Debug, Display},
    fs::{read, File},
    io::Write,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering::SeqCst},
};

use atomic_write_file::{unix::OpenOptionsExt, AtomicWriteFile};
use bincode::{
    config::standard,
    de::{BorrowDecoder, Decoder},
    enc::Encoder,
    error::DecodeError,
    BorrowDecode, Decode, Encode,
};
use dashmap::DashMap;
use jane_eyre::eyre::{self, bail, Context as _};
use rayon::{
    in_place_scope,
    iter::{IntoParallelRefIterator, ParallelIterator as _},
    Scope, ThreadPool, ThreadPoolBuilder,
};
use tracing::{debug, info, warn};

use crate::{
    path::{DynamicPath, POSTS_PATH_ROOT},
    render_markdown, FilteredPost, Thread, UnsafePost,
};

struct Context {
    output_writer_pool: ThreadPool,
    derivation_writer_pool: ThreadPool,
    read_file_derivation_cache: MemoryCache<Id, ReadFileDrv>,
    read_file_output_cache: MemoryCache<Id, Vec<u8>>,
    render_markdown_derivation_cache: MemoryCache<Id, RenderMarkdownDrv>,
    render_markdown_output_cache: MemoryCache<Id, String>,
    filtered_post_derivation_cache: MemoryCache<Id, FilteredPostDrv>,
    filtered_post_output_cache: MemoryCache<Id, FilteredPost>,
    thread_derivation_cache: MemoryCache<Id, ThreadDrv>,
    thread_output_cache: MemoryCache<Id, Thread>,
}
struct ContextGuard<'ctx, 'scope> {
    context: &'ctx Context,
    output_writer_scope: &'ctx Scope<'scope>,
    derivation_writer_scope: &'ctx Scope<'scope>,
}
impl Context {
    fn new() -> Self {
        let cpu_count = std::thread::available_parallelism()
            .expect("failed to get cpu count")
            .get();
        Self {
            output_writer_pool: ThreadPoolBuilder::new()
                .thread_name(|i| format!("outWriter{i}"))
                .num_threads(cpu_count * 4)
                .build()
                .expect("failed to build thread pool"),
            derivation_writer_pool: ThreadPoolBuilder::new()
                .thread_name(|i| format!("drvWriter{i}"))
                .num_threads(cpu_count * 4)
                .build()
                .expect("failed to build thread pool"),
            read_file_derivation_cache: MemoryCache::new("ReadFileDrv"),
            read_file_output_cache: MemoryCache::new("ReadFileOut"),
            render_markdown_derivation_cache: MemoryCache::new("RenderMarkdownDrv"),
            render_markdown_output_cache: MemoryCache::new("RenderMarkdownOut"),
            filtered_post_derivation_cache: MemoryCache::new("FilteredPostDrv"),
            filtered_post_output_cache: MemoryCache::new("FilteredPostOut"),
            thread_derivation_cache: MemoryCache::new("ThreadDrv"),
            thread_output_cache: MemoryCache::new("ThreadOut"),
        }
    }
    fn run<R: Send>(fun: impl FnOnce(ContextGuard) -> R + Send) -> R {
        Self::new().scope(fun)
    }
    fn scope<R: Send>(&self, fun: impl FnOnce(ContextGuard) -> R + Send) -> R {
        self.output_writer_pool.scope(move |output_writer_scope| {
            self.derivation_writer_pool
                .scope(move |derivation_writer_scope| {
                    fun(ContextGuard {
                        context: self,
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
    fn get_or_insert_as_read(
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
    fn get_or_insert_as_write(
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
impl<__Context> Decode<__Context> for Hash {
    fn decode<D: Decoder<Context = __Context>>(decoder: &mut D) -> Result<Self, DecodeError> {
        Ok(Self(blake3::Hash::from_bytes(Decode::decode(decoder)?)))
    }
}
impl<'__de, __Context> BorrowDecode<'__de, __Context> for Hash {
    fn borrow_decode<D: BorrowDecoder<'__de, Context = __Context>>(
        decoder: &mut D,
    ) -> Result<Self, DecodeError> {
        Ok(Self(
            blake3::Hash::from_slice(BorrowDecode::borrow_decode(decoder)?)
                .map_err(|e| DecodeError::OtherString(e.to_string()))?,
        ))
    }
}
impl Encode for Hash {
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
    type Output: Clone + Decode<()> + Encode;
    fn function_name() -> &'static str;
    fn id(&self) -> Id;
    fn derivation_cache(ctx: &Context) -> &MemoryCache<Id, Self>;
    fn output_cache(ctx: &Context) -> &MemoryCache<Id, Self::Output>;
    /// only to be called by [`Derivation::realise_self_only()`]. do not call this method elsewhere.
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output>;
    /// implementations should call `dep.realise_recursive_debug(ctx)` for each dependency, then call `self.realise_self_only(ctx)`.
    /// in other words, the default impl where `Self` has no dependencies should be: `self.realise_self_only(ctx)`
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output>;

    // provided methods below
    fn derivation_path(id: &Id) -> String {
        format!("cache/{id}.drv")
    }
    fn output_path(&self) -> String {
        format!("cache/{}.out", self.id())
    }
    fn output(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        Self::output_cache(ctx.context).get_or_insert_as_read(self.id(), |_id| {
            Ok(bincode::decode_from_std_read(
                &mut File::open(self.output_path())?,
                standard(),
            )?)
        })
    }
    /// same as [`Derivation::realise_recursive()`], but traced at info level.
    #[tracing::instrument(level = "info", name = "build", skip_all, fields(function = %Self::function_name(), id = %self.id()))]
    fn realise_recursive_info(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        info!("building");
        self.realise_recursive(ctx)
    }
    /// same as [`Derivation::realise_recursive()`], but traced at debug level.
    #[tracing::instrument(level = "debug", name = "build", skip_all, fields(function = %Self::function_name(), id = %self.id()))]
    fn realise_recursive_debug(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        debug!("building");
        self.realise_recursive(ctx)
    }
    fn realise_self_only(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        Self::output_cache(ctx.context).get_or_insert_as_write(
            self.id(),
            |_id| {
                Ok(bincode::decode_from_std_read(
                    &mut File::open(self.output_path())?,
                    standard(),
                )?)
            },
            |_id| {
                debug!("building {self}");
                let result = (|| -> eyre::Result<_> {
                    let content = self.compute_output(ctx)?;
                    let output_path = self.output_path();
                    let content_for_write = bincode::encode_to_vec(&content, standard())?;
                    ctx.output_writer_scope.spawn(move |_| {
                        if let Err(error) = atomic_write(output_path, content_for_write) {
                            warn!(?error, "failed to write derivation output");
                        }
                    });
                    Ok(content)
                })();
                result.wrap_err_with(|| format!("failed to realise derivation: {self:?}"))
            },
        )
    }
}
impl Derivation for ReadFileDrv {
    type Output = Vec<u8>;
    fn function_name() -> &'static str {
        "ReadFile"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache(ctx: &Context) -> &MemoryCache<Id, Self> {
        &ctx.read_file_derivation_cache
    }
    fn output_cache(ctx: &Context) -> &MemoryCache<Id, Self::Output> {
        &ctx.read_file_output_cache
    }
    fn compute_output(&self, _ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        let output = read(&self.inner.path)?;
        let expected_hash = self.inner.hash;
        let actual_hash = Hash(blake3::hash(&output));
        if actual_hash != expected_hash {
            bail!("hash mismatch! expected {expected_hash}, actual {actual_hash}");
        }
        Ok(output)
    }
    #[tracing::instrument(skip_all, fields(id = %self.id()))]
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        self.realise_self_only(ctx)
    }
}
impl Derivation for RenderMarkdownDrv {
    type Output = String;
    fn function_name() -> &'static str {
        "RenderMarkdown"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache(ctx: &Context) -> &MemoryCache<Id, Self> {
        &ctx.render_markdown_derivation_cache
    }
    fn output_cache(ctx: &Context) -> &MemoryCache<Id, Self::Output> {
        &ctx.render_markdown_output_cache
    }
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        let unsafe_markdown = ReadFileDrv::load(ctx, self.inner.file.id())?.output(ctx)?;
        Ok(render_markdown(str::from_utf8(&unsafe_markdown)?))
    }
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        self.inner.file.realise_recursive_debug(ctx)?;
        self.realise_self_only(ctx)
    }
}
impl Derivation for FilteredPostDrv {
    type Output = FilteredPost;
    fn function_name() -> &'static str {
        "FilteredPost"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache(ctx: &Context) -> &MemoryCache<Id, Self> {
        &ctx.filtered_post_derivation_cache
    }
    fn output_cache(ctx: &Context) -> &MemoryCache<Id, Self::Output> {
        &ctx.filtered_post_output_cache
    }
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        let unsafe_html = match &self.inner {
            DoFilteredPost::Html(file) => {
                str::from_utf8(&ReadFileDrv::load(ctx, file.id())?.output(ctx)?)?.to_owned()
            }
            DoFilteredPost::Markdown(file) => {
                RenderMarkdownDrv::load(ctx, file.id())?.output(ctx)?
            }
        };
        let post = UnsafePost::with_html(&unsafe_html);
        let post = FilteredPost::filter(post)?;
        Ok(post)
    }
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        match &self.inner {
            DoFilteredPost::Html(file) => {
                file.realise_recursive_debug(ctx)?;
            }
            DoFilteredPost::Markdown(file) => {
                file.realise_recursive_debug(ctx)?;
            }
        };
        self.realise_self_only(ctx)
    }
}
impl Derivation for ThreadDrv {
    type Output = Thread;
    fn function_name() -> &'static str {
        "Thread"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache(ctx: &Context) -> &MemoryCache<Id, Self> {
        &ctx.thread_derivation_cache
    }
    fn output_cache(ctx: &Context) -> &MemoryCache<Id, Self::Output> {
        &ctx.thread_output_cache
    }
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        let post = FilteredPostDrv::load(ctx, self.inner.post.id())?.output(ctx)?;
        let references = self
            .inner
            .references
            .iter()
            .map(|post| FilteredPostDrv::load(ctx, post.id())?.output(ctx))
            .collect::<eyre::Result<_>>()?;
        let thread = Thread::new(post, references);
        Ok(thread)
    }
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
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
        let result =
            bincode::encode_to_vec(self, standard()).expect("guaranteed by derive Serialize");
        Id(Hash(blake3::hash(&result)))
    }
}
impl DerivationInner for DoReadFile {}
impl DerivationInner for DoRenderMarkdown {}
impl DerivationInner for DoFilteredPost {}
impl DerivationInner for DoThread {}

impl Display for DoReadFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadFile")
            .field("path", &UseDisplay(&self.path))
            .field("hash", &UseDisplay(&self.hash))
            .finish()
    }
}
impl Display for DoRenderMarkdown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderMarkdown")
            .field("file", &UseDisplay(&self.file))
            .finish()
    }
}
impl Display for DoFilteredPost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DoFilteredPost::Html(file) => f
                .debug_struct("FilteredPost")
                .field("file", &UseDisplay(file))
                .finish(),
            DoFilteredPost::Markdown(file) => f
                .debug_struct("FilteredPost")
                .field("file", &UseDisplay(file))
                .finish(),
        }
    }
}
impl Display for DoThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thread")
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

    use crate::cache::{atomic_writer, ContextGuard, Derivation, DerivationInner, Drv};

    impl<Inner: DerivationInner> Drv<Inner>
    where
        Self: Derivation,
    {
        pub fn instantiate(ctx: &ContextGuard, inner: Inner) -> eyre::Result<Self> {
            let output = inner.compute_id();
            Self { output, inner }.store(ctx)
        }

        fn store(self, ctx: &ContextGuard) -> eyre::Result<Self> {
            Self::derivation_cache(ctx.context).get_or_insert_as_write(
                self.id(),
                |id| Self::load(ctx, *id),
                |id| {
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
                },
            )
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
        Self::instantiate(
            ctx,
            DoRenderMarkdown {
                file: ReadFileDrv::new(ctx, path)?,
            },
        )
    }
}
impl FilteredPostDrv {
    fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let DynamicPath::Posts(posts_path) = &path else {
            bail!("path is not a posts path")
        };
        let inner = if posts_path.is_markdown_post() {
            DoFilteredPost::Markdown(RenderMarkdownDrv::new(ctx, path)?)
        } else {
            DoFilteredPost::Html(ReadFileDrv::new(ctx, path)?)
        };
        Self::instantiate(ctx, inner)
    }
}
impl ThreadDrv {
    fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let post_derivation = FilteredPostDrv::new(ctx, path)?;
        // effectively an IFD
        let post = post_derivation.realise_recursive(ctx)?;
        let references = post
            .meta
            .front_matter
            .references
            .par_iter()
            .map(|path| FilteredPostDrv::new(ctx, path.to_dynamic_path()))
            .collect::<eyre::Result<Vec<_>>>()?;
        Self::instantiate(
            ctx,
            DoThread {
                post: post_derivation,
                references,
            },
        )
    }
}

impl<Inner: DerivationInner> Drv<Inner>
where
    Self: Derivation,
{
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
