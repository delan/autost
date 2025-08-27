use std::{collections::BTreeSet, fmt::Display, fs::read};

use bincode::{Decode, Encode};
use jane_eyre::eyre::{self, bail};
use rayon::iter::{once, IntoParallelRefIterator as _, ParallelIterator as _};
use tokio::runtime::Runtime;
use tracing::Span;

use crate::{
    cache::{
        mem::MemoryCache, CollectionDisplay, Context, ContextGuard, Derivation, DerivationInner,
        Drv, Id, UseDisplay,
    },
    command::render::RenderedThread,
    output::{ThreadsContentTemplate, ThreadsPageTemplate},
    path::DynamicPath,
    render_markdown, FilteredPost, TagIndex, Thread, UnsafePost, SETTINGS,
};

pub type ReadFileDrv = Drv<DoReadFile>;
pub type RenderMarkdownDrv = Drv<DoRenderMarkdown>;
pub type FilteredPostDrv = Drv<DoFilteredPost>;
pub type ThreadDrv = Drv<DoThread>;
pub type TagIndexDrv = Drv<DoTagIndex>;
pub type RenderedThreadDrv = Drv<DoRenderedThread>;

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct DoReadFile {
    path: DynamicPath,
    hash: super::hash::Hash,
}
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct DoRenderMarkdown {
    file: ReadFileDrv,
}
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub enum DoFilteredPost {
    Html(ReadFileDrv),
    Markdown(RenderMarkdownDrv),
}
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct DoThread {
    post: FilteredPostDrv,
    references: Vec<FilteredPostDrv>,
}
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct DoTagIndex {
    files: BTreeSet<ReadFileDrv>,
}
#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct DoRenderedThread {
    thread: ThreadDrv,
}

impl DerivationInner for DoReadFile {}
impl DerivationInner for DoRenderMarkdown {}
impl DerivationInner for DoFilteredPost {}
impl DerivationInner for DoThread {}
impl DerivationInner for DoTagIndex {}
impl DerivationInner for DoRenderedThread {}

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
            .field("references", &CollectionDisplay(self.references.iter()))
            .finish()
    }
}
impl Display for DoTagIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TagIndex")
            .field("files", &CollectionDisplay(self.files.iter()))
            .finish()
    }
}
impl Display for DoRenderedThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderedThread")
            .field("thread", &UseDisplay(&self.thread))
            .finish()
    }
}

impl ReadFileDrv {
    pub fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        let hash = super::hash::Hash(blake3::hash(&read(&path)?));
        Self::instantiate(ctx, DoReadFile { path, hash })
    }
}
impl RenderMarkdownDrv {
    pub fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
        Self::instantiate(
            ctx,
            DoRenderMarkdown {
                file: ReadFileDrv::new(ctx, path)?,
            },
        )
    }
}
impl FilteredPostDrv {
    pub fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
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
    pub fn new(ctx: &ContextGuard, path: DynamicPath) -> eyre::Result<Self> {
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
impl TagIndexDrv {
    pub fn new(ctx: &ContextGuard, files: BTreeSet<ReadFileDrv>) -> eyre::Result<Self> {
        Self::instantiate(ctx, DoTagIndex { files })
    }
}
impl RenderedThreadDrv {
    pub fn new(ctx: &ContextGuard, thread: ThreadDrv) -> eyre::Result<Self> {
        Self::instantiate(ctx, DoRenderedThread { thread })
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
        let actual_hash = super::hash::Hash(blake3::hash(&output));
        if actual_hash != expected_hash {
            bail!("hash mismatch! expected {expected_hash}, actual {actual_hash}");
        }
        Ok(output)
    }
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
        let (path, unsafe_html) = match &self.inner {
            DoFilteredPost::Html(file) => (
                &file.inner.path,
                str::from_utf8(&ReadFileDrv::load(ctx, file.id())?.output(ctx)?)?.to_owned(),
            ),
            DoFilteredPost::Markdown(file) => (
                &file.inner.file.inner.path,
                RenderMarkdownDrv::load(ctx, file.id())?.output(ctx)?,
            ),
        };
        let post = if let DynamicPath::Posts(path) = path {
            UnsafePost::with_html(&unsafe_html, path.clone())
        } else {
            UnsafePost::with_html(&unsafe_html, None)
        };
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
        let span = Span::current();
        self.inner
            .references
            .par_iter()
            .chain(once(&self.inner.post))
            .map(|post| {
                let _entered = span.clone().entered();
                post.realise_recursive_debug(ctx)
            })
            .collect::<eyre::Result<Vec<_>>>()?;
        self.realise_self_only(ctx)
    }
}
impl Derivation for TagIndexDrv {
    type Output = TagIndex;
    fn function_name() -> &'static str {
        "TagIndex"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache(ctx: &Context) -> &MemoryCache<Id, Self> {
        &ctx.tag_index_derivation_cache
    }
    fn output_cache(ctx: &Context) -> &MemoryCache<Id, Self::Output> {
        &ctx.tag_index_output_cache
    }
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        let span = Span::current();
        let threads = self
            .inner
            .files
            .iter()
            .map(|file| {
                let _entered = span.clone().entered();
                let drv = ThreadDrv::new(ctx, file.inner.path.clone())?;
                let thread = drv.realise_recursive_debug(ctx)?;
                Ok((drv.id(), thread))
            })
            .collect::<eyre::Result<_>>()?;
        let thread = Runtime::new()?.block_on(TagIndex::new(threads))?;
        Ok(thread)
    }
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        // XXX: should we continue to realise the ReadFileDrv deps here at least?
        self.realise_self_only(ctx)
    }
}
impl Derivation for RenderedThreadDrv {
    type Output = RenderedThread;
    fn function_name() -> &'static str {
        "RenderedThread"
    }
    fn id(&self) -> Id {
        self.output
    }
    fn derivation_cache(ctx: &Context) -> &MemoryCache<Id, Self> {
        &ctx.rendered_thread_derivation_cache
    }
    fn output_cache(ctx: &Context) -> &MemoryCache<Id, Self::Output> {
        &ctx.rendered_thread_output_cache
    }
    fn compute_output(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        let thread = self.inner.thread.output(ctx)?;
        let threads_content_normal = ThreadsContentTemplate::render_normal(&thread)?;
        let threads_content_simple = ThreadsContentTemplate::render_simple(&thread)?;
        let single_threads_page = ThreadsPageTemplate::render_single_thread(
            &thread,
            &threads_content_normal,
            // FIXME: impure
            &SETTINGS.page_title(thread.meta.front_matter.title.as_deref()),
            &None,
        )?;
        Ok(RenderedThread {
            threads_content_normal,
            threads_content_simple,
            single_threads_page,
        })
    }
    fn realise_recursive(&self, ctx: &ContextGuard) -> eyre::Result<Self::Output> {
        self.inner.thread.realise_recursive_debug(ctx)?;
        self.realise_self_only(ctx)
    }
}
