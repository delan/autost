use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{create_dir_all, read_dir, File},
    io::Write,
};

use chrono::{SecondsFormat, Utc};
use jane_eyre::eyre::{self, bail, OptionExt};
use tracing::{debug, info};

use crate::{
    meta::hard_link_attachments_into_site,
    migrations::run_migrations,
    output::{AtomFeedTemplate, ThreadsContentTemplate, ThreadsPageTemplate},
    path::{PostsPath, SitePath},
    TemplatedPost, Thread, SETTINGS,
};

pub fn main(args: impl Iterator<Item = String>) -> eyre::Result<()> {
    let mut args = args.peekable();

    if args.peek().is_some() {
        let args = args
            .map(|path| PostsPath::from_site_root_relative_path(&path))
            .collect::<eyre::Result<Vec<_>>>()?;
        render(args)
    } else {
        render_all()
    }
}

pub fn render_all() -> eyre::Result<()> {
    let mut post_paths = vec![];

    create_dir_all(&*PostsPath::ROOT)?;
    for entry in read_dir(&*PostsPath::ROOT)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        // cohost2autost creates directories for chost thread ancestors.
        if metadata.is_dir() {
            continue;
        }

        let path = PostsPath::ROOT.join_dir_entry(&entry)?;
        post_paths.push(path);
    }

    render(post_paths)
}

pub fn render<'posts>(post_paths: Vec<PostsPath>) -> eyre::Result<()> {
    run_migrations()?;

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    create_dir_all(&*SitePath::ROOT)?;
    create_dir_all(&*SitePath::TAGGED)?;

    fn copy_static(output_path: &SitePath, file: &StaticFile) -> eyre::Result<()> {
        let StaticFile(filename, content) = file;
        if let Some(static_path) = SETTINGS.path_to_static() {
            std::fs::copy(static_path.join(filename), output_path.join(filename)?)?;
        } else {
            File::create(output_path.join(filename)?)?.write_all(content)?;
        }
        Ok(())
    }
    struct StaticFile(&'static str, &'static [u8]);
    let static_files = [
        StaticFile("deploy.sh", include_bytes!("../../static/deploy.sh")),
        StaticFile("style.css", include_bytes!("../../static/style.css")),
        StaticFile("script.js", include_bytes!("../../static/script.js")),
        StaticFile(
            "Atkinson-Hyperlegible-Font-License-2020-1104.pdf",
            include_bytes!("../../static/Atkinson-Hyperlegible-Font-License-2020-1104.pdf"),
        ),
        StaticFile(
            "Atkinson-Hyperlegible-Regular-102.woff2",
            include_bytes!("../../static/Atkinson-Hyperlegible-Regular-102.woff2"),
        ),
        StaticFile(
            "Atkinson-Hyperlegible-Italic-102.woff2",
            include_bytes!("../../static/Atkinson-Hyperlegible-Italic-102.woff2"),
        ),
        StaticFile(
            "Atkinson-Hyperlegible-Bold-102.woff2",
            include_bytes!("../../static/Atkinson-Hyperlegible-Bold-102.woff2"),
        ),
        StaticFile(
            "Atkinson-Hyperlegible-BoldItalic-102.woff2",
            include_bytes!("../../static/Atkinson-Hyperlegible-BoldItalic-102.woff2"),
        ),
    ];
    for file in static_files.iter() {
        copy_static(&*SitePath::ROOT, file)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let deploy_path = SitePath::ROOT.join("deploy.sh")?;
        let mut permissions = std::fs::metadata(&deploy_path)?.permissions();
        let mode = permissions.mode();
        permissions.set_mode(mode | 0o111);
        std::fs::set_permissions(deploy_path, permissions)?;
    }

    let results = post_paths
        // TODO: .into_par_iter()
        .into_iter()
        .map(render_single_post)
        .collect::<Vec<_>>();

    let RenderResult {
        mut tags,
        mut collections,
        mut interesting_output_paths,
        mut threads_by_interesting_tag,
    } = RenderResult::default()?;
    let mut threads_cache = BTreeMap::default();
    for result in results {
        let CacheableRenderResult {
            render_result: result,
            cached_thread,
        } = result?;
        for (tag, count) in result.tags {
            *tags.entry(tag).or_insert(0) += count;
        }
        collections.merge(result.collections);
        interesting_output_paths.extend(result.interesting_output_paths);
        for (tag, threads) in result.threads_by_interesting_tag {
            threads_by_interesting_tag
                .entry(tag)
                .or_default()
                .extend(threads);
        }
        let path = cached_thread
            .thread
            .path
            .clone()
            .ok_or_eyre("thread has no path")?;
        debug_assert!(!threads_cache.contains_key(&path));
        threads_cache.insert(path, cached_thread);
    }

    // author step: generate atom feeds.
    let atom_feed_path =
        collections.write_atom_feed("index", &SitePath::ROOT, &now, &threads_cache)?;
    interesting_output_paths.insert(atom_feed_path);

    // generate /tagged/<tag>.feed.xml and /tagged/<tag>.html.
    for (tag, threads) in threads_by_interesting_tag {
        let atom_feed_path = SitePath::TAGGED.join(&format!("{tag}.feed.xml"))?;
        let thread_refs = threads
            .iter()
            .map(|thread| &threads_cache[&thread.path].thread)
            .collect::<Vec<_>>();
        let atom_feed = AtomFeedTemplate::render(
            thread_refs,
            &format!("{} — {tag}", SETTINGS.site_title),
            &now,
        )?;
        writeln!(File::create(&atom_feed_path)?, "{}", atom_feed,)?;
        interesting_output_paths.insert(atom_feed_path);
        let threads_content = render_cached_threads_content(&threads_cache, &threads);
        let threads_page = ThreadsPageTemplate::render(
            &threads_content,
            &format!("#{tag} — {}", SETTINGS.site_title),
            &Some(SitePath::TAGGED.join(&format!("{tag}.feed.xml"))?),
        )?;
        // TODO: move this logic into path module and check for slashes
        let threads_page_path = SitePath::TAGGED.join(&format!("{tag}.html"))?;
        writeln!(File::create(&threads_page_path)?, "{}", threads_page)?;
        interesting_output_paths.insert(threads_page_path);
    }

    let mut tags = tags.into_iter().collect::<Vec<_>>();
    tags.sort_by(|p, q| p.1.cmp(&q.1).reverse().then(p.0.cmp(&q.0)));
    info!("all tags: {tags:?}");
    info!(
        "interesting tags: {:?}",
        tags.iter()
            .filter(|(tag, _)| SETTINGS.tag_is_interesting(tag))
            .collect::<Vec<_>>()
    );

    // reader step: generate posts pages.
    for key in collections.keys() {
        info!(
            "writing threads page for collection {key:?} ({} threads)",
            collections.len(key),
        );
        // TODO: write internal collections to another dir?
        let threads_page_path =
            collections.write_threads_page(key, &SitePath::ROOT, &threads_cache)?;
        if collections.is_interesting(key) {
            interesting_output_paths.insert(threads_page_path);
        }
    }

    let interesting_output_paths = interesting_output_paths
        .into_iter()
        .map(|path| format!("{}\n", path.rsync_deploy_line()))
        .collect::<Vec<_>>()
        .join("");
    if let Some(path) = &SETTINGS.interesting_output_filenames_list_path {
        File::create(path)?.write_all(interesting_output_paths.as_bytes())?;
    }

    Ok(())
}

fn render_single_post(path: PostsPath) -> eyre::Result<CacheableRenderResult> {
    let mut result = RenderResult::default()?;

    let post = TemplatedPost::load(&path)?;
    let Some(rendered_path) = path.rendered_path()? else {
        bail!("post has no rendered path");
    };
    let thread = Thread::try_from(post)?;
    hard_link_attachments_into_site(thread.needs_attachments())?;
    for tag in thread.meta.tags.iter() {
        *result.tags.entry(tag.clone()).or_insert(0usize) += 1;
    }
    result.collections.push("all", &path, &thread);
    let mut was_interesting = false;
    if thread.meta.archived.is_none() && SETTINGS.self_author == thread.meta.author {
        was_interesting = true;
    } else if SETTINGS.thread_is_on_excluded_archived_list(&thread) {
        result.collections.push("excluded", &path, &thread);
    } else if SETTINGS.thread_is_on_interesting_archived_list(&thread) {
        result
            .collections
            .push("marked_interesting", &path, &thread);
        was_interesting = true;
    } else {
        for tag in thread.meta.tags.iter() {
            if SETTINGS.tag_is_interesting(tag) {
                was_interesting = true;
                break;
            }
        }
    }
    if was_interesting {
        result
            .interesting_output_paths
            .insert(rendered_path.clone());
        result.collections.push("index", &path, &thread);
        for tag in thread.meta.tags.iter() {
            if SETTINGS.tag_is_interesting(tag) {
                result
                    .threads_by_interesting_tag
                    .entry(tag.clone())
                    .or_default()
                    .insert(ThreadInCollection {
                        published: thread.meta.published.clone(),
                        path: path.clone(),
                    });
            }
        }
        if thread.meta.tags.is_empty() {
            result
                .collections
                .push("untagged_interesting", &path, &thread);
        }
    } else {
        // if the thread had some input from us at publish time, that is, if the last post was
        // authored by us with content and/or tags...
        if thread.posts.last().is_some_and(|post| {
            (!post.meta.is_transparent_share || !post.meta.tags.is_empty())
                && post
                    .meta
                    .author
                    .as_ref()
                    .is_some_and(|author| SETTINGS.other_self_authors.contains(&author.href))
        }) {
            result.collections.push("skipped_own", &path, &thread);
        } else {
            result.collections.push("skipped_other", &path, &thread);
        }
    }

    let threads_content =
        ThreadsContentTemplate::render_normal_without_fixing_relative_urls(vec![thread.clone()])?;

    debug!("writing post page: {rendered_path:?}");
    let threads_page = ThreadsPageTemplate::render(
        &threads_content,
        &format!("{} — {}", thread.overall_title, SETTINGS.site_title),
        &None,
    )?;
    writeln!(File::create(rendered_path)?, "{}", threads_page)?;

    let result = CacheableRenderResult {
        render_result: result,
        cached_thread: CachedThread {
            thread,
            threads_content,
        },
    };

    Ok(result)
}

struct CacheableRenderResult {
    render_result: RenderResult,
    cached_thread: CachedThread,
}

struct RenderResult {
    tags: BTreeMap<String, usize>,
    collections: Collections,
    interesting_output_paths: BTreeSet<SitePath>,
    threads_by_interesting_tag: BTreeMap<String, BTreeSet<ThreadInCollection>>,
}

struct CachedThread {
    thread: Thread,
    threads_content: String,
}

struct Collections {
    inner: BTreeMap<&'static str, Collection>,
}

struct Collection {
    feed_href: Option<SitePath>,
    title: String,
    threads: BTreeSet<ThreadInCollection>,
}

#[derive(Eq, PartialEq)]
struct ThreadInCollection {
    published: Option<String>,
    path: PostsPath,
}

impl RenderResult {
    fn default() -> eyre::Result<Self> {
        Ok(Self {
            tags: Default::default(),
            collections: Collections::default()?,
            interesting_output_paths: Default::default(),
            threads_by_interesting_tag: Default::default(),
        })
    }
}

impl Collections {
    fn default() -> eyre::Result<Self> {
        Ok(Self {
            inner: [
                (
                    "index",
                    Collection::new(Some(SitePath::ROOT.join("index.feed.xml")?), "posts"),
                ),
                ("all", Collection::new(None, "all posts")),
                (
                    "untagged_interesting",
                    Collection::new(None, "untagged interesting posts"),
                ),
                (
                    "excluded",
                    Collection::new(None, "archived posts that were marked excluded"),
                ),
                (
                    "marked_interesting",
                    Collection::new(None, "archived posts that were marked interesting"),
                ),
                (
                    "skipped_own",
                    Collection::new(None, "own skipped archived posts"),
                ),
                (
                    "skipped_other",
                    Collection::new(None, "others’ skipped archived posts"),
                ),
            ]
            .into(),
        })
    }

    fn merge(&mut self, other: Self) {
        assert!(self.inner.keys().eq(other.inner.keys()));
        for (key, collection) in other.inner {
            assert_eq!(self.inner[key].feed_href, collection.feed_href);
            assert_eq!(self.inner[key].title, collection.title);
            let threads = &mut self
                .inner
                .get_mut(key)
                .expect("guaranteed by assert")
                .threads;
            for thread in collection.threads {
                threads.insert(thread);
            }
        }
    }

    fn keys(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(|key| *key)
    }

    fn len(&self, key: &str) -> usize {
        self.inner[key].threads.len()
    }

    fn push(&mut self, key: &str, path: &PostsPath, thread: &Thread) {
        self.inner
            .get_mut(key)
            .expect("BUG: unknown collection!")
            .threads
            .insert(ThreadInCollection {
                published: thread.meta.published.clone(),
                path: path.clone(),
            });
    }

    fn is_interesting(&self, key: &str) -> bool {
        self.inner[key].is_interesting()
    }

    fn write_threads_page(
        &self,
        key: &str,
        output_dir: &SitePath,
        threads_cache: &BTreeMap<PostsPath, CachedThread>,
    ) -> eyre::Result<SitePath> {
        let path = output_dir.join(&format!("{key}.html"))?;
        self.inner[key].write_threads_page(&path, threads_cache)?;

        Ok(path)
    }

    fn write_atom_feed(
        &self,
        key: &str,
        output_dir: &SitePath,
        now: &str,
        threads_cache: &BTreeMap<PostsPath, CachedThread>,
    ) -> eyre::Result<SitePath> {
        let path = output_dir.join(&format!("{key}.feed.xml"))?;
        self.inner[key].write_atom_feed(&path, now, threads_cache)?;

        Ok(path)
    }
}

impl Collection {
    fn new(feed_href: Option<SitePath>, title: &str) -> Self {
        Self {
            feed_href,
            title: title.to_owned(),
            threads: BTreeSet::default(),
        }
    }

    fn is_interesting(&self) -> bool {
        // this definition may change in the future.
        self.feed_href.is_some()
    }

    fn write_threads_page(
        &self,
        posts_page_path: &SitePath,
        threads_cache: &BTreeMap<PostsPath, CachedThread>,
    ) -> eyre::Result<()> {
        let threads_content = render_cached_threads_content(threads_cache, &self.threads);
        writeln!(
            File::create(posts_page_path)?,
            "{}",
            ThreadsPageTemplate::render(
                &threads_content,
                &format!("{} — {}", self.title, SETTINGS.site_title),
                &self.feed_href,
            )?
        )?;

        Ok(())
    }

    fn write_atom_feed(
        &self,
        atom_feed_path: &SitePath,
        now: &str,
        threads_cache: &BTreeMap<PostsPath, CachedThread>,
    ) -> eyre::Result<()> {
        let thread_refs = self
            .threads
            .iter()
            .map(|thread| &threads_cache[&thread.path].thread)
            .collect::<Vec<_>>();
        writeln!(
            File::create(atom_feed_path)?,
            "{}",
            AtomFeedTemplate::render(thread_refs, &SETTINGS.site_title, now)?
        )?;

        Ok(())
    }
}

impl Ord for ThreadInCollection {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // reverse chronological
        self.published
            .cmp(&other.published)
            .reverse()
            .then_with(|| self.path.cmp(&other.path))
    }
}
impl PartialOrd for ThreadInCollection {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn render_cached_threads_content(
    cache: &BTreeMap<PostsPath, CachedThread>,
    threads: &BTreeSet<ThreadInCollection>,
) -> String {
    let threads_contents = threads
        .iter()
        .map(|thread| &*cache[&thread.path].threads_content)
        .collect::<Vec<_>>();

    threads_contents.join("")
}
