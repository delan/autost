use std::{cmp::Ordering, fs::File, io::Read, path::Path, sync::LazyLock};

use askama::Template;
use jane_eyre::eyre::{self, Context, OptionExt};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{meta::extract_metadata, settings::Settings};

pub mod cohost;
pub mod dom;
pub mod meta;
pub mod settings;

pub static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    Settings::load_default()
        .context("failed to load settings")
        .unwrap()
});

#[derive(Clone, Debug, Default, PartialEq, Template)]
#[template(path = "post-meta.html")]
pub struct PostMeta {
    pub archived: Option<String>,
    pub references: Vec<String>,
    pub title: Option<String>,
    pub published: Option<String>,
    pub author: Option<Author>,
    pub tags: Vec<String>,
    pub is_transparent_share: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Author {
    pub href: String,
    pub name: String,
    pub display_name: String,
    pub display_handle: String,
}

#[derive(Debug, PartialEq)]
pub struct ExtractedPost {
    pub unsafe_html: String,
    pub meta: PostMeta,
}

#[derive(Clone, Debug, Template)]
#[template(path = "threads.html")]
pub struct ThreadsTemplate {
    pub threads: Vec<Thread>,
    pub page_title: String,
    pub feed_href: Option<String>,
}

#[derive(Clone, Debug, Template)]
#[template(path = "feed.xml")]
pub struct AtomFeedTemplate {
    pub threads: Vec<Thread>,
    pub feed_title: String,
    pub updated: String,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub href: String,
    pub posts: Vec<TemplatedPost>,
    pub meta: PostMeta,
    pub overall_title: String,
}

#[derive(Clone, Debug)]
pub struct TemplatedPost {
    pub filename: String,
    pub meta: PostMeta,
    pub original_html: String,
    pub safe_html: String,
}

impl Thread {
    pub fn reverse_chronological(p: &Thread, q: &Thread) -> Ordering {
        p.meta.published.cmp(&q.meta.published).reverse()
    }
}

impl TemplatedPost {
    pub fn load(path: &Path) -> eyre::Result<Self> {
        let mut file = File::open(&path)?;
        let mut unsafe_source = String::default();
        file.read_to_string(&mut unsafe_source)?;

        let unsafe_html = if path.ends_with(".md") {
            // author step: render markdown to html.
            render_markdown(&unsafe_source)
        } else {
            unsafe_source
        };

        let original_name = path.file_name().ok_or_eyre("post has no filename")?;
        let original_name = original_name.to_str().ok_or_eyre("unsupported filename")?;
        let (filename, _) = original_name
            .rsplit_once(".")
            .unwrap_or((original_name, ""));
        let filename = format!("{filename}.html");

        Self::filter(&unsafe_html, &filename)
    }

    pub fn filter(unsafe_html: &str, filename: &str) -> eyre::Result<Self> {
        // reader step: extract metadata.
        let post = extract_metadata(unsafe_html)?;

        // reader step: filter html.
        let safe_html = ammonia::Builder::default()
            .add_generic_attributes(["style", "id"])
            .add_generic_attributes(["data-cohost-href", "data-cohost-src"]) // cohost2autost
            .add_tag_attributes("a", ["target"])
            .add_tag_attributes("details", ["open"])
            .add_tag_attributes("img", ["loading"])
            .add_tags(["meta"])
            .add_tag_attributes("meta", ["name", "content"])
            .id_prefix(Some("user-content-")) // cohost compatibility
            .clean(&post.unsafe_html)
            .to_string();

        Ok(TemplatedPost {
            filename: filename.to_owned(),
            meta: post.meta,
            original_html: unsafe_html.to_owned(),
            safe_html,
        })
    }
}

pub fn cli_init() -> eyre::Result<()> {
    jane_eyre::install()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
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
