use askama::Template;
use jane_eyre::eyre;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod cohost;
pub mod dom;

#[derive(Clone, Debug, PartialEq, Template)]
#[template(path = "post-meta.html")]
pub struct PostMeta {
    pub title: Option<String>,
    pub published: Option<String>,
    pub author: Option<(String, String)>,
}

#[derive(Debug, PartialEq)]
pub struct ExtractedPost {
    pub unsafe_html: String,
    pub meta: PostMeta,
}

#[derive(Clone, Debug, Template)]
#[template(path = "posts.html")]
pub struct PostsPageTemplate {
    pub posts: Vec<TemplatedPost>,
}

#[derive(Clone, Debug)]
pub struct TemplatedPost {
    pub post_page_href: String,
    pub meta: PostMeta,
    pub content: String,
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
