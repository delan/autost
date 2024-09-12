use askama::Template;

pub mod cohost;
pub mod dom;

#[derive(Template)]
#[template(path = "post-meta.html")]
pub struct PostMeta {
    pub title: String,
    pub published: String,
}

/// render markdown in a cohost-compatible way.
///
/// known discrepancies:
/// - `~~strikethrough~~` not handled
/// - @mentions not handled
/// - :emotes: not handled
/// - single newline always yields `<br>`
///   (this was not the case for older chosts, as reflected in their `.astMap`)
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
