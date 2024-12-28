use std::{cmp::Ordering, collections::BTreeSet, fs::File, io::Read, sync::LazyLock};

use askama::Template;
use jane_eyre::eyre::{self, Context, OptionExt};
use markup5ever_rcdom::RcDom;
use serde::Deserialize;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{
    dom::serialize_html_fragment,
    meta::extract_metadata,
    path::{PostsPath, SitePath},
    settings::Settings,
};

pub mod command {
    pub mod attach;
    pub mod cohost2autost;
    pub mod cohost2json;
    pub mod cohost_archive;
    pub mod import;
    pub mod new;
    pub mod render;
    pub mod server;
}

pub mod attachments;
pub mod cohost;
pub mod dom;
pub mod meta;
pub mod migrations;
pub mod output;
pub mod path;
pub mod settings;

pub static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    #[cfg(test)]
    let result = Settings::load_example();

    #[cfg(not(test))]
    let result = Settings::load_default();

    result.context("failed to load settings").unwrap()
});

#[derive(Clone, Debug, Default, PartialEq, Template)]
#[template(path = "post-meta.html")]
pub struct PostMeta {
    pub archived: Option<String>,
    pub references: Vec<PostsPath>,
    pub title: Option<String>,
    pub published: Option<String>,
    pub author: Option<Author>,
    pub tags: Vec<String>,
    pub is_transparent_share: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Author {
    pub href: String,
    pub name: String,
    pub display_name: String,
    pub display_handle: String,
}

pub struct ExtractedPost {
    pub dom: RcDom,
    pub meta: PostMeta,
    pub needs_attachments: BTreeSet<SitePath>,
    pub og_image: Option<String>,
    pub og_description: String,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub path: Option<PostsPath>,
    pub posts: Vec<TemplatedPost>,
    pub meta: PostMeta,
    pub needs_attachments: BTreeSet<SitePath>,
    pub og_image: Option<String>,
    pub og_description: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TemplatedPost {
    pub path: Option<PostsPath>,
    pub meta: PostMeta,
    pub original_html: String,
    pub safe_html: String,
    pub needs_attachments: BTreeSet<SitePath>,
    pub og_image: Option<String>,
    pub og_description: String,
}

impl PostMeta {
    pub fn is_main_self_author(&self, settings: &Settings) -> bool {
        self.author
            .as_ref()
            .map_or(settings.self_author.is_none(), |a| {
                settings.is_main_self_author(a)
            })
    }
}

#[test]
fn test_is_main_self_author() -> eyre::Result<()> {
    let settings = Settings::load_example()?;

    let mut settings_no_self_author = Settings::load_example()?;
    let mut meta_no_author = PostMeta::default();
    settings_no_self_author.self_author = None;
    meta_no_author.author = None;

    // same href as [self_author], but different name, display_name, and handle
    let mut meta_same_href = PostMeta::default();
    meta_same_href.author = Some(Author {
        href: "https://example.com".to_owned(),
        name: "".to_owned(),
        display_name: "".to_owned(),
        display_handle: "".to_owned(),
    });

    // different href from [self_author]
    let mut meta_different_href = PostMeta::default();
    meta_different_href.author = Some(Author {
        href: "https://example.net".to_owned(),
        name: "".to_owned(),
        display_name: "".to_owned(),
        display_handle: "".to_owned(),
    });

    assert!(meta_same_href.is_main_self_author(&settings));
    assert!(!meta_different_href.is_main_self_author(&settings));
    assert!(!meta_no_author.is_main_self_author(&settings));
    assert!(!meta_same_href.is_main_self_author(&settings_no_self_author));
    assert!(!meta_different_href.is_main_self_author(&settings_no_self_author));
    assert!(meta_no_author.is_main_self_author(&settings_no_self_author));

    Ok(())
}

impl Thread {
    pub fn reverse_chronological(p: &Thread, q: &Thread) -> Ordering {
        p.meta.published.cmp(&q.meta.published).reverse()
    }

    pub fn url_for_original_path(&self) -> eyre::Result<Option<String>> {
        let result = self.path.as_ref().map(|path| path.references_url());

        Ok(result)
    }

    pub fn url_for_html_permalink(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(|path| path.rendered_path())
            .transpose()?
            .flatten()
            .map(|path| path.internal_url());

        Ok(result)
    }

    pub fn url_for_atom_permalink(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(|path| path.rendered_path())
            .transpose()?
            .flatten()
            .map(|path| path.external_url());

        Ok(result)
    }

    pub fn atom_feed_entry_id(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(|path| path.rendered_path())
            .transpose()?
            .flatten()
            .map(|path| path.atom_feed_entry_id());

        Ok(result)
    }

    pub fn needs_attachments(&self) -> impl Iterator<Item = &SitePath> {
        self.needs_attachments.iter()
    }

    pub fn posts_in_thread(&self) -> impl Iterator<Item = PostInThread> + '_ {
        let len = self.posts.len();

        self.posts
            .iter()
            .cloned()
            .enumerate()
            .map(move |(i, post)| {
                if i == len - 1 {
                    PostInThread {
                        inner: post,
                        is_main_post: true,
                    }
                } else {
                    PostInThread {
                        inner: post,
                        is_main_post: false,
                    }
                }
            })
    }

    pub fn main_post(&self) -> eyre::Result<&TemplatedPost> {
        self.posts.last().ok_or_eyre("thread has no posts")
    }
}

pub struct PostInThread {
    inner: TemplatedPost,
    is_main_post: bool,
}

impl TryFrom<TemplatedPost> for Thread {
    type Error = eyre::Report;

    fn try_from(mut post: TemplatedPost) -> eyre::Result<Self> {
        let path = post.path.clone();
        let extra_tags = SETTINGS
            .extra_archived_thread_tags(&post)
            .into_iter()
            .filter(|tag| !post.meta.tags.contains(tag))
            .map(|tag| tag.to_owned())
            .collect::<Vec<_>>();
        let combined_tags = extra_tags
            .into_iter()
            .chain(post.meta.tags.into_iter())
            .collect();
        let resolved_tags = SETTINGS.resolve_tags(combined_tags);
        post.meta.tags = resolved_tags;
        let mut meta = post.meta.clone();

        let mut posts = post
            .meta
            .references
            .iter()
            .map(|path| TemplatedPost::load(path))
            .collect::<Result<Vec<_>, _>>()?;
        posts.push(post);

        // TODO: skip threads with other authors?
        // TODO: skip threads with private or logged-in-only authors?
        // TODO: gate sensitive posts behind an interaction?

        // for thread metadata, take the last post that is not a transparent share (which MAY have
        // tags, but SHOULD NOT have a title and MUST NOT have a body), and use its metadata if any.
        let last_non_transparent_share_post = posts
            .iter()
            .rev()
            .find(|post| !post.meta.is_transparent_share);
        meta.title = last_non_transparent_share_post.map(|post| {
            if let Some(title) = post.meta.title.clone().filter(|t| !t.is_empty()) {
                title
            } else if let Some(author) = post.meta.author.as_ref() {
                format!("untitled post by {}", author.display_handle)
            } else {
                "untitled post".to_owned()
            }
        });
        let og_image = last_non_transparent_share_post
            .and_then(|post| post.og_image.as_deref())
            .map(|og_image| SETTINGS.base_url_relativise(og_image));
        let og_description =
            last_non_transparent_share_post.map(|post| post.og_description.to_owned());

        let needs_attachments = posts
            .iter()
            .flat_map(|post| post.needs_attachments.iter())
            .map(|attachment_path| attachment_path.to_owned())
            .collect();

        Ok(Thread {
            path,
            posts,
            meta,
            needs_attachments,
            og_image,
            og_description,
        })
    }
}

impl TemplatedPost {
    pub fn load(path: &PostsPath) -> eyre::Result<Self> {
        let mut file = File::open(path)?;
        let mut unsafe_source = String::default();
        file.read_to_string(&mut unsafe_source)?;

        let unsafe_html = if path.is_markdown_post() {
            // author step: render markdown to html.
            render_markdown(&unsafe_source)
        } else {
            unsafe_source
        };

        Self::filter(&unsafe_html, Some(path.to_owned()))
    }

    pub fn filter(unsafe_html: &str, path: Option<PostsPath>) -> eyre::Result<Self> {
        // reader step: extract metadata.
        let post = extract_metadata(unsafe_html)?;

        // reader step: filter html.
        let extracted_html = serialize_html_fragment(post.dom)?;
        let safe_html = ammonia::Builder::default()
            .add_generic_attributes(["style", "id"])
            .add_generic_attributes(["data-cohost-href", "data-cohost-src"]) // cohost2autost
            .add_generic_attributes(["data-import-src"]) // autost import
            .add_tag_attributes("a", ["target"])
            .add_tag_attributes("audio", ["controls", "src"])
            .add_tag_attributes("details", ["open", "name"]) // <details name> for cohost compatibility
            .add_tag_attributes("img", ["loading"])
            .add_tag_attributes("video", ["controls", "src"])
            .add_tags(["audio", "meta", "video"])
            .add_tag_attributes("meta", ["name", "content"])
            .id_prefix(Some("user-content-")) // cohost compatibility
            .clean(&extracted_html)
            .to_string();

        Ok(TemplatedPost {
            path,
            meta: post.meta,
            original_html: unsafe_html.to_owned(),
            safe_html,
            needs_attachments: post.needs_attachments,
            og_image: post.og_image,
            og_description: post.og_description,
        })
    }
}

pub fn cli_init() -> eyre::Result<()> {
    jane_eyre::install()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive("autost=info".parse()?)
                .from_env_lossy(),
        )
        .init();

    Ok(())
}

/// render markdown in a cohost-compatible way.
///
/// known discrepancies:
/// - `~~strikethrough~~` not handled
/// - @mentions not handled
/// - :emotes: not handled
/// - single newline always yields `<br>`
///   (this was not the case for older chosts, as reflected in their `.astMap`)
/// - blank lines in `<details>` close the element in some situations?
/// - spaced numbered lists yield separate `<ol start>` instead of `<li><p>`
pub fn render_markdown(markdown: &str) -> String {
    let mut options = comrak::Options::default();
    options.render.unsafe_ = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.render.hardbreaks = true;
    let unsafe_html = comrak::markdown_to_html(&markdown, &options);

    unsafe_html
}

#[test]
fn test_render_markdown() {
    assert_eq!(
        render_markdown("first\nsecond"),
        "<p>first<br />\nsecond</p>\n"
    );
}
