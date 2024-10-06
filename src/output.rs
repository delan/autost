//! output templates. these templates are wrapped in a safe interface that
//! guarantees that path-relative urls are made path-absolute.

use askama::Template;
use jane_eyre::eyre;
use markup5ever_rcdom::{NodeData, RcDom};

use crate::{
    dom::{
        html_attributes_with_urls, parse_html_document, parse_html_fragment,
        serialize_html_document, serialize_html_fragment, TendrilExt, Transform,
    },
    path::{parse_path_relative_scheme_less_url_string, SitePath},
    PostMeta, Thread, SETTINGS,
};

#[derive(Clone, Debug, Template)]
#[template(path = "threads.html")]
pub struct ThreadsPageTemplate {
    /// not `threads: Vec<Thread>`, to encourage us to cache ThreadsContentTemplate output between
    /// individual thread pages and combined collection pages.
    threads_content: String,
    page_title: String,
    feed_href: Option<SitePath>,
}

#[derive(Clone, Debug, Template)]
#[template(path = "threads-content.html")]
pub struct ThreadsContentTemplate {
    threads: Vec<Thread>,
    simple_mode: bool,
}

#[derive(Clone, Debug, Template)]
#[template(path = "thread-or-post-header.html")]
pub struct ThreadOrPostHeaderTemplate {
    thread: Thread,
    post_meta: PostMeta,
    is_thread_header: bool,
}

#[derive(Clone, Debug, Template)]
#[template(path = "feed.xml")]
pub struct AtomFeedTemplate<'threads> {
    thread_refs: Vec<&'threads Thread>,
    feed_title: String,
    updated: String,
}

impl ThreadsPageTemplate {
    pub fn render(
        threads_content: String,
        page_title: String,
        feed_href: Option<SitePath>,
    ) -> eyre::Result<String> {
        fix_relative_urls_in_html_document(
            &Self {
                threads_content,
                page_title,
                feed_href,
            }
            .render()?,
        )
    }
}

impl ThreadsContentTemplate {
    pub fn render_normal(threads: Vec<Thread>) -> eyre::Result<String> {
        fix_relative_urls_in_html_fragment(&Self::render_normal_without_fixing_relative_urls(
            threads,
        )?)
    }

    pub fn render_normal_without_fixing_relative_urls(
        threads: Vec<Thread>,
    ) -> eyre::Result<String> {
        Ok(Self {
            threads,
            simple_mode: false,
        }
        .render()?)
    }

    fn render_simple(thread: &Thread) -> eyre::Result<String> {
        fix_relative_urls_in_html_fragment(
            &Self {
                threads: vec![thread.to_owned()],
                simple_mode: true,
            }
            .render()?,
        )
    }
}

impl ThreadOrPostHeaderTemplate {
    pub fn render(
        thread: Thread,
        post_meta: PostMeta,
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

impl<'threads> AtomFeedTemplate<'threads> {
    pub fn render(
        thread_refs: Vec<&'threads Thread>,
        feed_title: String,
        updated: String,
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
            }
            new_kids.push(kid.clone());
        }
        Ok(())
    })? {}

    Ok(dom)
}
