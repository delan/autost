pub mod drv;
mod fs;
mod hash;
mod mem;
mod stats;

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{Debug, Display},
    fs::{create_dir_all, read, File},
    str::FromStr,
};

use bincode::{config::standard, Decode, Encode};
use jane_eyre::eyre::{self, Context as _};
use rayon::{
    iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelBridge, ParallelIterator as _},
    Scope, ThreadPool, ThreadPoolBuilder,
};
use tracing::{debug, info, warn};

use crate::{
    cache::{
        drv::{
            FilteredPostDrv, ReadFileDrv, RenderMarkdownDrv, RenderedThreadDrv, TagIndexDrv,
            ThreadDrv,
        },
        fs::atomic_write,
        mem::{pack_names, MemoryCache},
        stats::STATS,
    },
    command::{cache::Test, render::RenderedThread},
    path::{PostsPath, CACHE_PATH_ROOT, POSTS_PATH_ROOT},
    CachelessTagIndex, FilteredPost, TagIndex, Thread,
};

pub struct Context {
    use_packs: bool,
    did_load_packs: bool,
    compute_pool: ThreadPool,
    derivation_writer_pool: ThreadPool,
    output_writer_pool: ThreadPool,
    read_file_derivation_cache: MemoryCache<Id, ReadFileDrv>,
    read_file_output_cache: MemoryCache<Id, Vec<u8>>,
    render_markdown_derivation_cache: MemoryCache<Id, RenderMarkdownDrv>,
    render_markdown_output_cache: MemoryCache<Id, String>,
    filtered_post_derivation_cache: MemoryCache<Id, FilteredPostDrv>,
    filtered_post_output_cache: MemoryCache<Id, FilteredPost>,
    thread_derivation_cache: MemoryCache<Id, ThreadDrv>,
    thread_output_cache: MemoryCache<Id, Thread>,
    tag_index_derivation_cache: MemoryCache<Id, TagIndexDrv>,
    tag_index_output_cache: MemoryCache<Id, TagIndex>,
    rendered_thread_derivation_cache: MemoryCache<Id, RenderedThreadDrv>,
    rendered_thread_output_cache: MemoryCache<Id, RenderedThread>,
}
pub struct ContextGuard<'ctx, 'scope> {
    context: &'ctx Context,
    derivation_writer_scope: &'ctx Scope<'scope>,
    output_writer_scope: &'ctx Scope<'scope>,
}
#[derive(Debug, Default, Decode, Encode)]
struct CachePack {
    read_file_derivation_cache: BTreeMap<Id, ReadFileDrv>,
    read_file_output_cache: BTreeMap<Id, Vec<u8>>,
    render_markdown_derivation_cache: BTreeMap<Id, RenderMarkdownDrv>,
    render_markdown_output_cache: BTreeMap<Id, String>,
    filtered_post_derivation_cache: BTreeMap<Id, FilteredPostDrv>,
    filtered_post_output_cache: BTreeMap<Id, FilteredPost>,
    thread_derivation_cache: BTreeMap<Id, ThreadDrv>,
    thread_output_cache: BTreeMap<Id, Thread>,
    tag_index_derivation_cache: BTreeMap<Id, TagIndexDrv>,
    tag_index_output_cache: BTreeMap<Id, TagIndex>,
    rendered_thread_derivation_cache: BTreeMap<Id, RenderedThreadDrv>,
    rendered_thread_output_cache: BTreeMap<Id, RenderedThread>,
}
impl Context {
    pub fn new(use_packs: bool) -> Context {
        let cpu_count = std::thread::available_parallelism()
            .expect("failed to get cpu count")
            .get();
        let ctx = Self {
            use_packs,
            did_load_packs: false,
            compute_pool: ThreadPoolBuilder::new()
                .thread_name(|i| format!("compute{i}"))
                .num_threads(cpu_count)
                .build()
                .expect("failed to build thread pool"),
            derivation_writer_pool: ThreadPoolBuilder::new()
                .thread_name(|i| format!("drvWriter{i}"))
                .num_threads(cpu_count * 4)
                .build()
                .expect("failed to build thread pool"),
            output_writer_pool: ThreadPoolBuilder::new()
                .thread_name(|i| format!("outWriter{i}"))
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
            tag_index_derivation_cache: MemoryCache::new("TagIndexDrv"),
            tag_index_output_cache: MemoryCache::new("TagIndexOut"),
            rendered_thread_derivation_cache: MemoryCache::new("RenderedThreadDrv"),
            rendered_thread_output_cache: MemoryCache::new("RenderedThreadOut"),
        };
        ctx
    }

    pub fn run<R: Send>(mut self, fun: impl FnOnce(&ContextGuard) -> R + Send) -> eyre::Result<R> {
        create_dir_all(&*CACHE_PATH_ROOT)?;
        if self.use_packs {
            info!("reading cache packs");
            let packs = pack_names()
                .par_bridge()
                .map(|name| -> eyre::Result<_> {
                    Ok(read(CACHE_PATH_ROOT.join(&format!("{name}.pack"))?)?)
                })
                .filter_map(|pack| pack.ok())
                .collect::<Vec<_>>();
            let packs = packs
                .into_par_iter()
                .map(|pack| Ok(bincode::decode_from_slice(&pack, standard())?.0))
                .collect::<eyre::Result<Vec<CachePack>>>()?;
            for pack in packs {
                self.did_load_packs = true;
                self.read_file_derivation_cache
                    .extend(pack.read_file_derivation_cache);
                self.read_file_output_cache
                    .extend(pack.read_file_output_cache);
                self.render_markdown_derivation_cache
                    .extend(pack.render_markdown_derivation_cache);
                self.render_markdown_output_cache
                    .extend(pack.render_markdown_output_cache);
                self.filtered_post_derivation_cache
                    .extend(pack.filtered_post_derivation_cache);
                self.filtered_post_output_cache
                    .extend(pack.filtered_post_output_cache);
                self.thread_derivation_cache
                    .extend(pack.thread_derivation_cache);
                self.thread_output_cache.extend(pack.thread_output_cache);
                self.tag_index_derivation_cache
                    .extend(pack.tag_index_derivation_cache);
                self.tag_index_output_cache
                    .extend(pack.tag_index_output_cache);
                self.rendered_thread_derivation_cache
                    .extend(pack.rendered_thread_derivation_cache);
                self.rendered_thread_output_cache
                    .extend(pack.rendered_thread_output_cache);
            }
            info!("running workload");
        }
        let result = {
            let ctx = &self;
            ctx.output_writer_pool.scope(move |output_writer_scope| {
                ctx.derivation_writer_pool
                    .scope(move |derivation_writer_scope| {
                        // the compute pool scope is the innermost scope, so `in_place_scope()` will spawn tasks into it.
                        // but we ignore the `Scope` argument for the compute pool, because explicitly spawning tasks into
                        // it would fail with borrow checker errors.
                        ctx.compute_pool.scope(move |_compute_scope| {
                            fun(&ContextGuard {
                                context: ctx,
                                derivation_writer_scope,
                                output_writer_scope,
                            })
                        })
                    })
            })
        };
        if self.use_packs && !self.did_load_packs {
            info!("building cache packs");
            let mut packs: BTreeMap<String, CachePack> = BTreeMap::default();
            for (name, cache) in self.read_file_derivation_cache.encodable_sharded() {
                packs.entry(name).or_default().read_file_derivation_cache = cache;
            }
            for (name, cache) in self.read_file_output_cache.encodable_sharded() {
                packs.entry(name).or_default().read_file_output_cache = cache;
            }
            for (name, cache) in self.render_markdown_derivation_cache.encodable_sharded() {
                packs
                    .entry(name)
                    .or_default()
                    .render_markdown_derivation_cache = cache;
            }
            for (name, cache) in self.render_markdown_output_cache.encodable_sharded() {
                packs.entry(name).or_default().render_markdown_output_cache = cache;
            }
            for (name, cache) in self.filtered_post_derivation_cache.encodable_sharded() {
                packs
                    .entry(name)
                    .or_default()
                    .filtered_post_derivation_cache = cache;
            }
            for (name, cache) in self.filtered_post_output_cache.encodable_sharded() {
                packs.entry(name).or_default().filtered_post_output_cache = cache;
            }
            for (name, cache) in self.thread_derivation_cache.encodable_sharded() {
                packs.entry(name).or_default().thread_derivation_cache = cache;
            }
            for (name, cache) in self.thread_output_cache.encodable_sharded() {
                packs.entry(name).or_default().thread_output_cache = cache;
            }
            for (name, cache) in self.tag_index_derivation_cache.encodable_sharded() {
                packs.entry(name).or_default().tag_index_derivation_cache = cache;
            }
            for (name, cache) in self.tag_index_output_cache.encodable_sharded() {
                packs.entry(name).or_default().tag_index_output_cache = cache;
            }
            for (name, cache) in self.rendered_thread_derivation_cache.encodable_sharded() {
                packs
                    .entry(name)
                    .or_default()
                    .rendered_thread_derivation_cache = cache;
            }
            for (name, cache) in self.rendered_thread_output_cache.encodable_sharded() {
                packs.entry(name).or_default().rendered_thread_output_cache = cache;
            }
            info!("writing cache packs");
            self.derivation_writer_pool
                .scope(move |derivation_writer_scope| {
                    self.compute_pool.scope(move |_| {
                        packs
                            .into_par_iter()
                            .map(|(name, pack)| {
                                let content = bincode::encode_to_vec(pack, standard())?;
                                derivation_writer_scope.spawn(move |_| {
                                    let path =
                                        CACHE_PATH_ROOT.join(&format!("{name}.pack")).unwrap();
                                    atomic_write(path, content).unwrap();
                                });
                                Ok(())
                            })
                            .collect::<eyre::Result<Vec<_>>>()
                    })
                })?;
        }

        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, Decode, Encode, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Id(self::hash::Hash);
impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl FromStr for Id {
    type Err = eyre::Report;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(self::hash::Hash(blake3::Hash::from_hex(s)?)))
    }
}

pub trait Derivation: Debug + Display + Sized + Sync {
    type Output: Clone + Decode<()> + Encode + Send + Sync;
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
        debug!("realising");
        if let Ok(result) = self.output(ctx) {
            return Ok(result);
        }
        self.realise_recursive(ctx)
    }
    /// same as [`Derivation::realise_recursive()`], but traced at debug level.
    #[tracing::instrument(level = "info", name = "build", skip_all, fields(function = %Self::function_name(), id = %self.id()))]
    fn realise_recursive_debug(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        debug!("realising");
        if let Ok(result) = self.output(ctx) {
            return Ok(result);
        }
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
                info!(thread = std::thread::current().name(), function = %Self::function_name(), "building");
                debug!(%self);
                let result = (|| -> eyre::Result<_> {
                    let content = self.compute_output(ctx)?;
                    if !ctx.context.use_packs || ctx.context.did_load_packs {
                        let output_path = self.output_path();
                        let content_for_write = bincode::encode_to_vec(&content, standard())?;
                        STATS.record_enqueue_output_write();
                        ctx.output_writer_scope.spawn(move |_| {
                            STATS.record_dequeue_output_write();
                            if let Err(error) = atomic_write(output_path, content_for_write) {
                                warn!(?error, "failed to write derivation output");
                            }
                        });
                    }
                    Ok(content)
                })();
                let result = result.wrap_err_with(|| format!("failed to realise derivation: {self:?}"))?;
                STATS.record_derivation_realised();
                Ok(result)
            },
        )
    }
}

pub trait DerivationInner: Clone + Debug + Display + Send + Decode<()> + Encode + 'static {
    fn compute_id(&self) -> Id {
        let result =
            bincode::encode_to_vec(self, standard()).expect("guaranteed by derive Serialize");
        Id(self::hash::Hash(blake3::hash(&result)))
    }
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct Drv<Inner> {
    output: Id,
    inner: Inner,
}
impl<Inner: Display> Display for Drv<Inner> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Derivation({} -> {})", self.output.0, self.inner)
    }
}
struct UseDisplay<'d, D: Display>(&'d D);
struct CollectionDisplay<'d, I: Clone + Iterator<Item = &'d D>, D: Display + 'd>(I);
impl<'d, D: Display> Debug for UseDisplay<'d, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl<'d, I: Clone + Iterator<Item = &'d D>, D: Display + 'd> Debug for CollectionDisplay<'d, I, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.clone().map(|value| UseDisplay(value)))
            .finish()
    }
}
mod private {
    use bincode::config::standard;
    use jane_eyre::eyre;
    use tracing::warn;

    use crate::cache::{
        fs::atomic_writer, stats::STATS, ContextGuard, Derivation, DerivationInner, Drv,
    };

    impl<Inner: DerivationInner> Drv<Inner>
    where
        Self: Derivation,
    {
        pub fn instantiate(ctx: &ContextGuard, inner: Inner) -> eyre::Result<Self> {
            let output = inner.compute_id();
            let result = Self { output, inner }.store(ctx)?;
            STATS.record_derivation_instantiated();
            Ok(result)
        }

        fn store(self, ctx: &ContextGuard) -> eyre::Result<Self> {
            Self::derivation_cache(ctx.context).get_or_insert_as_write(
                self.id(),
                |id| Self::load(ctx, *id),
                |id| {
                    if !ctx.context.use_packs || ctx.context.did_load_packs {
                        let path = Self::derivation_path(id);
                        let self_for_write = self.clone();
                        STATS.record_enqueue_derivation_write();
                        ctx.derivation_writer_scope.spawn(move |_| {
                            STATS.record_dequeue_derivation_write();
                            let result = || -> eyre::Result<()> {
                                let mut file = atomic_writer(path)?;
                                bincode::encode_into_std_write(
                                    self_for_write,
                                    &mut file,
                                    standard(),
                                )?;
                                file.commit()?;
                                Ok(())
                            }();
                            if let Err(error) = result {
                                warn!(?error, "failed to write derivation");
                            }
                        });
                    }
                    Ok(self)
                },
            )
        }
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

pub async fn test(args: Test) -> eyre::Result<()> {
    Context::new(args.use_packs).run(|ctx| -> eyre::Result<()> {
        let top_level_post_paths = POSTS_PATH_ROOT.read_dir_flat()?;
        eprintln!("building tag index");
        let (threads_by_path, tag_index) = if args.use_cache {
            let threads = top_level_post_paths
                .par_iter()
                .map(|path| ThreadDrv::new(ctx, path.to_dynamic_path()))
                .collect::<eyre::Result<BTreeSet<_>>>()?;
            let tag_index = TagIndexDrv::new(ctx, threads)?.realise_recursive_info(ctx)?;
            let mut threads_by_path = BTreeMap::default();
            let mut new_tags: BTreeMap<String, BTreeSet<PostsPath>> = BTreeMap::default();
            for (tag, threads) in tag_index.tags {
                let threads = threads
                    .into_par_iter()
                    .map(|id| {
                        let thread = ThreadDrv::load(ctx, id)?.output(ctx)?;
                        Ok(thread.path.clone().map(|path| (path, thread)))
                    })
                    .filter_map(|result| result.transpose())
                    .collect::<eyre::Result<BTreeMap<_, _>>>()?;
                new_tags.insert(tag, threads.keys().cloned().collect());
                threads_by_path.extend(threads);
            }
            (threads_by_path, CachelessTagIndex { tags: new_tags })
        } else {
            let threads = top_level_post_paths
                .par_iter()
                .map(|path| {
                    let post = FilteredPost::load(path)?;
                    let thread = Thread::try_from(post)?;
                    Ok(thread.path.clone().map(|path| (path, thread)))
                })
                .filter_map(|result| result.transpose())
                .collect::<eyre::Result<BTreeMap<_, _>>>()?;
            (threads.clone(), CachelessTagIndex::new(threads))
        };
        if let Some(tag) = args.list_threads_in_tag {
            let thread_paths = tag_index.tags.get(&tag);
            let mut threads = thread_paths
                .par_iter()
                .flat_map(|paths| paths.par_iter())
                .map(|path| {
                    let thread = &threads_by_path[path];
                    Ok((thread.meta.front_matter.published.clone(), thread.clone()))
                })
                .collect::<eyre::Result<Vec<_>>>()?;
            threads.sort();
            println!("{} threads in tag {tag:?}:", threads.len());
            for (published, thread) in threads {
                if let Some(((published, path), description)) =
                    published.zip(thread.path).zip(thread.meta.og_description)
                {
                    let excerpt = description.chars().take(50).collect::<String>();
                    println!("- {published:?}, {}, {excerpt:?}", path.to_dynamic_path());
                }
            }
        }
        STATS.enable_pending_write_logging();
        Ok(())
    })??;

    Ok(())
}
