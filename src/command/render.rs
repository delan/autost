use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{create_dir_all, read_dir, File},
    io::Write,
    path::Path,
};

use askama::Template;
use chrono::{SecondsFormat, Utc};
use jane_eyre::eyre::{self, bail};
use tracing::{debug, info, trace};

use crate::{
    migrations::run_migrations,
    path::{PostsPath, SitePath},
    AtomFeedTemplate, TemplatedPost, Thread, ThreadsContentTemplate, ThreadsTemplate, SETTINGS,
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
    let mut collections = Collections::new([
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
    ]);
    let mut threads_by_interesting_tag = BTreeMap::default();
    let mut tags = BTreeMap::default();
    let mut interesting_output_paths = BTreeSet::default();

    create_dir_all(&*SitePath::ROOT)?;
    create_dir_all(&*SitePath::TAGGED)?;

    fn copy_static(output_path: &SitePath, filename: &str) -> eyre::Result<()> {
        let path_to_autost = Path::new(&SETTINGS.path_to_autost);
        std::fs::copy(
            path_to_autost.join("static").join(filename),
            output_path.join(filename)?,
        )?;
        Ok(())
    }
    copy_static(&*SitePath::ROOT, "style.css")?;
    copy_static(&*SitePath::ROOT, "script.js")?;
    copy_static(
        &*SitePath::ROOT,
        "Atkinson-Hyperlegible-Font-License-2020-1104.pdf",
    )?;
    copy_static(&*SitePath::ROOT, "Atkinson-Hyperlegible-Regular-102.woff2")?;
    copy_static(&*SitePath::ROOT, "Atkinson-Hyperlegible-Italic-102.woff2")?;
    copy_static(&*SitePath::ROOT, "Atkinson-Hyperlegible-Bold-102.woff2")?;
    copy_static(
        &*SitePath::ROOT,
        "Atkinson-Hyperlegible-BoldItalic-102.woff2",
    )?;

    for path in post_paths {
        let post = TemplatedPost::load(&path)?;
        let Some(rendered_path) = post.rendered_path.clone() else {
            bail!("post has no rendered path");
        };
        let thread = Thread::try_from(post)?;

        for tag in thread.meta.tags.iter() {
            *tags.entry(tag.clone()).or_insert(0usize) += 1;
        }
        collections.push("all", thread.clone());
        let mut was_interesting = false;
        if thread.meta.archived.is_none() && SETTINGS.self_author == thread.meta.author {
            was_interesting = true;
        } else if SETTINGS.thread_is_on_excluded_archived_list(&thread) {
            collections.push("excluded", thread.clone());
        } else if SETTINGS.thread_is_on_interesting_archived_list(&thread) {
            collections.push("marked_interesting", thread.clone());
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
            interesting_output_paths.insert(rendered_path.clone());
            collections.push("index", thread.clone());
            for tag in thread.meta.tags.iter() {
                if SETTINGS.tag_is_interesting(tag) {
                    threads_by_interesting_tag
                        .entry(tag.clone())
                        .or_insert(vec![])
                        .push(thread.clone());
                }
            }
            if thread.meta.tags.is_empty() {
                collections.push("untagged_interesting", thread.clone());
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
                collections.push("skipped_own", thread.clone());
            } else {
                collections.push("skipped_other", thread.clone());
            }
        }

        // reader step: generate post page.
        let template = ThreadsContentTemplate {
            threads: vec![thread.clone()],
        };
        let content = template.render()?;
        let template = ThreadsTemplate {
            content,
            page_title: format!("{} — {}", thread.overall_title, SETTINGS.site_title),
            feed_href: None,
        };
        debug!("writing post page: {rendered_path:?}");
        writeln!(File::create(rendered_path)?, "{}", template.render()?)?;
    }

    for (_, threads) in threads_by_interesting_tag.iter_mut() {
        threads.sort_by(Thread::reverse_chronological);
    }
    trace!("threads by tag: {threads_by_interesting_tag:#?}");

    // author step: generate atom feeds.
    let atom_feed_path = collections.write_atom_feed("index", &SitePath::ROOT, &now)?;
    interesting_output_paths.insert(atom_feed_path);
    for (tag, threads) in threads_by_interesting_tag.clone().into_iter() {
        let template = AtomFeedTemplate {
            threads,
            feed_title: format!("{} — {tag}", SETTINGS.site_title),
            updated: now.clone(),
        };
        let atom_feed_path = SitePath::TAGGED.join(&format!("{tag}.feed.xml"))?;
        writeln!(File::create(&atom_feed_path)?, "{}", template.render()?)?;
        interesting_output_paths.insert(atom_feed_path);
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
            collections.threads(key).len()
        );
        // TODO: write internal collections to another dir?
        let threads_page_path = collections.write_threads_page(key, &SitePath::ROOT)?;
        if collections.is_interesting(key) {
            interesting_output_paths.insert(threads_page_path);
        }
    }
    for (tag, threads) in threads_by_interesting_tag.into_iter() {
        let template = ThreadsContentTemplate { threads };
        let content = template.render()?;
        let template = ThreadsTemplate {
            content,
            page_title: format!("#{tag} — {}", SETTINGS.site_title),
            // TODO: move this logic into path module and check for slashes
            feed_href: Some(SitePath::TAGGED.join(&format!("{tag}.feed.xml"))?),
        };
        // TODO: move this logic into path module
        let threads_page_path = SitePath::TAGGED.join(&format!("{tag}.html"))?;
        writeln!(File::create(&threads_page_path)?, "{}", template.render()?)?;
        interesting_output_paths.insert(threads_page_path);
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

struct Collections {
    inner: BTreeMap<&'static str, Collection>,
}

struct Collection {
    feed_href: Option<SitePath>,
    title: String,
    threads: Vec<Thread>,
}

impl Collections {
    fn new(collections: impl IntoIterator<Item = (&'static str, Collection)>) -> Self {
        Self {
            inner: collections.into_iter().collect(),
        }
    }

    fn keys(&self) -> impl Iterator<Item = &str> {
        self.inner.keys().map(|key| *key)
    }

    fn threads(&self, key: &str) -> &[Thread] {
        &self.inner[key].threads
    }

    fn push(&mut self, key: &str, thread: Thread) {
        self.inner
            .get_mut(key)
            .expect("BUG: unknown collection!")
            .threads
            .push(thread);
    }

    fn is_interesting(&self, key: &str) -> bool {
        self.inner[key].is_interesting()
    }

    fn write_threads_page(&self, key: &str, output_dir: &SitePath) -> eyre::Result<SitePath> {
        let path = output_dir.join(&format!("{key}.html"))?;
        self.inner[key].write_threads_page(&path)?;

        Ok(path)
    }

    fn write_atom_feed(
        &self,
        key: &str,
        output_dir: &SitePath,
        now: &str,
    ) -> eyre::Result<SitePath> {
        let path = output_dir.join(&format!("{key}.feed.xml"))?;
        self.inner[key].write_atom_feed(&path, now)?;

        Ok(path)
    }
}

impl Collection {
    fn new(feed_href: Option<SitePath>, title: &str) -> Self {
        Self {
            feed_href,
            title: title.to_owned(),
            threads: vec![],
        }
    }

    fn is_interesting(&self) -> bool {
        // this definition may change in the future.
        self.feed_href.is_some()
    }

    fn write_threads_page(&self, posts_page_path: &SitePath) -> eyre::Result<()> {
        let mut threads = self.threads.clone();
        threads.sort_by(Thread::reverse_chronological);
        let template = ThreadsContentTemplate { threads };
        let content = template.render()?;
        let template = ThreadsTemplate {
            content,
            page_title: format!("{} — {}", self.title, SETTINGS.site_title),
            feed_href: self.feed_href.clone(),
        };
        writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;

        Ok(())
    }

    fn write_atom_feed(&self, atom_feed_path: &SitePath, now: &str) -> eyre::Result<()> {
        let mut threads = self.threads.clone();
        threads.sort_by(Thread::reverse_chronological);
        let template = AtomFeedTemplate {
            threads,
            feed_title: SETTINGS.site_title.clone(),
            updated: now.to_owned(),
        };
        writeln!(File::create(atom_feed_path)?, "{}", template.render()?)?;

        Ok(())
    }
}
