//! output templates. these templates are wrapped in a safe interface that
//! guarantees that path-relative urls are made path-absolute.

use askama::Template;
use jane_eyre::eyre;
use markup5ever_rcdom::{NodeData, RcDom};
use tracing::trace;

use crate::{
    css::{parse_inline_style, serialise_inline_style, InlineStyleToken},
    dom::{
        html_attributes_with_urls, parse_html_document, parse_html_fragment,
        serialize_html_document, serialize_html_fragment, AttrsMutExt, TendrilExt, Transform,
    },
    path::{parse_path_relative_scheme_less_url_string, SitePath},
    Author, PostMeta, Thread, SETTINGS,
};

#[derive(Clone, Debug, Template)]
#[template(path = "threads.html")]
pub struct ThreadsPageTemplate<'template> {
    thread_page_meta: Option<&'template str>,
    /// not `threads: Vec<Thread>`, to encourage us to cache ThreadsContentTemplate output between
    /// individual thread pages and combined collection pages.
    threads_content: &'template str,
    page_title: &'template str,
    feed_href: &'template Option<SitePath>,
}

#[derive(Clone, Debug, Template)]
#[template(path = "threads-content.html")]
pub struct ThreadsContentTemplate<'template> {
    thread: &'template Thread,
    simple_mode: bool,
}

#[derive(Clone, Debug, Template)]
#[template(path = "thread-or-post-header.html")]
pub struct ThreadOrPostHeaderTemplate<'template> {
    thread: &'template Thread,
    post_meta: &'template PostMeta,
    is_thread_header: bool,
}

#[derive(Clone, Debug, Template)]
#[template(path = "thread-or-post-author.html")]
pub struct ThreadOrPostAuthorTemplate<'template> {
    author: &'template Author,
}

#[derive(Clone, Debug, Template)]
#[template(path = "thread-or-post-meta.html")]
pub struct ThreadOrPostMetaTemplate<'template> {
    thread: &'template Thread,
}

#[derive(Clone, Debug, Template)]
#[template(path = "feed.xml")]
pub struct AtomFeedTemplate<'template> {
    thread_refs: Vec<&'template Thread>,
    feed_title: &'template str,
    updated: &'template str,
}

impl ThreadsPageTemplate<'_> {
    pub fn render(
        threads_content: &str,
        page_title: &str,
        feed_href: &Option<SitePath>,
    ) -> eyre::Result<String> {
        // render the template with a placeholder for `threads_content`, to avoid having to fix relative urls in the
        // html for the same threads over and over (we do that once per thread, when rendering the `CachedThread`).
        let template = ThreadsPageTemplate {
            thread_page_meta: None,
            threads_content: "\u{FDD0}",
            page_title,
            feed_href,
        }
        .render()?;
        let result = fix_relative_urls_in_html_document(&template)?;

        // now insert `threads_content` at the placeholder, making sure the placeholder wasn’t used elsewhere.
        // based on the assumption that the “fixing relative urls” operation distributes over the two parts.
        assert_eq!(template.matches("\u{FDD0}").count(), 1);
        let result = result.replace("\u{FDD0}", threads_content);

        Ok(result)
    }

    pub fn render_single_thread(
        thread: &Thread,
        threads_content: &str,
        page_title: &str,
        feed_href: &Option<SitePath>,
    ) -> eyre::Result<String> {
        let thread_page_meta = ThreadOrPostMetaTemplate::render(thread)?;

        fix_relative_urls_in_html_document(
            &ThreadsPageTemplate {
                thread_page_meta: Some(&thread_page_meta),
                threads_content,
                page_title,
                feed_href,
            }
            .render()?,
        )
    }
}

impl<'template> ThreadsContentTemplate<'template> {
    pub fn render_normal(thread: &'template Thread) -> eyre::Result<String> {
        fix_relative_urls_in_html_fragment(
            &Self {
                thread,
                simple_mode: false,
            }
            .render()?,
        )
    }

    fn render_simple(thread: &'template Thread) -> eyre::Result<String> {
        fix_relative_urls_in_html_fragment(
            &Self {
                thread,
                simple_mode: true,
            }
            .render()?,
        )
    }
}

impl<'template> ThreadOrPostHeaderTemplate<'template> {
    pub fn render(
        thread: &'template Thread,
        post_meta: &'template PostMeta,
        is_thread_header: bool,
    ) -> eyre::Result<String> {
        fix_relative_urls_in_html_fragment(
            &Self {
                thread,
                post_meta,
                is_thread_header,
            }
            .render()?,
        )
    }
}

impl<'template> ThreadOrPostAuthorTemplate<'template> {
    pub fn render(author: &'template Author) -> eyre::Result<String> {
        fix_relative_urls_in_html_fragment(&Self { author }.render()?)
    }
}

impl<'template> ThreadOrPostMetaTemplate<'template> {
    pub fn render(thread: &'template Thread) -> eyre::Result<String> {
        fix_relative_urls_in_html_fragment(&Self { thread }.render()?)
    }
}

impl<'template> AtomFeedTemplate<'template> {
    pub fn render(
        thread_refs: Vec<&'template Thread>,
        feed_title: &'template str,
        updated: &'template str,
    ) -> eyre::Result<String> {
        Ok(Self {
            thread_refs,
            feed_title,
            updated,
        }
        .render()?)
    }
}

fn fix_relative_urls_in_html_document(html: &str) -> eyre::Result<String> {
    let dom = parse_html_document(html.as_bytes())?;
    let dom = fix_relative_urls(dom)?;

    serialize_html_document(dom)
}

fn fix_relative_urls_in_html_fragment(html: &str) -> eyre::Result<String> {
    let dom = parse_html_fragment(html.as_bytes())?;
    let dom = fix_relative_urls(dom)?;

    serialize_html_fragment(dom)
}

fn fix_relative_urls(dom: RcDom) -> eyre::Result<RcDom> {
    let mut transform = Transform::new(dom.document.clone());
    while transform.next(|kids, new_kids| {
        for kid in kids {
            if let NodeData::Element { name, attrs, .. } = &kid.data {
                if let Some(attr_names) = html_attributes_with_urls().get(name) {
                    for attr in attrs.borrow_mut().iter_mut() {
                        if attr_names.contains(&attr.name) {
                            if let Some(url) =
                                parse_path_relative_scheme_less_url_string(attr.value.to_str())
                            {
                                attr.value = SETTINGS.base_url_relativise(&url).into();
                            }
                        }
                    }
                }
                if let Some(style) = attrs.borrow_mut().attr_mut("style") {
                    let old_style = style.value.to_str();
                    let mut has_any_relative_urls = false;
                    let mut tokens = vec![];
                    for token in parse_inline_style(style.value.to_str()) {
                        tokens.push(match token {
                            InlineStyleToken::Url(url) => {
                                if let Some(url) = parse_path_relative_scheme_less_url_string(&url)
                                {
                                    trace!(url, "found relative url in inline style");
                                    has_any_relative_urls = true;
                                    InlineStyleToken::Url(SETTINGS.base_url_relativise(&url))
                                } else {
                                    InlineStyleToken::Url(url)
                                }
                            }
                            other => other,
                        });
                    }
                    let new_style = serialise_inline_style(&tokens);
                    if has_any_relative_urls {
                        trace!("old style: {old_style}");
                        trace!("new style: {new_style}");
                        style.value = new_style.into();
                    }
                }
            }
            new_kids.push(kid.clone());
        }
        Ok(())
    })? {}

    Ok(dom)
}
