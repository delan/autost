use std::{collections::BTreeSet, fs::create_dir_all};

use html5ever::QualName;
use jane_eyre::eyre::{self, bail, OptionExt};
use markup5ever_rcdom::NodeData;
use tracing::trace;

use crate::{
    css::{parse_inline_style, InlineStyleToken},
    dom::{
        html_attributes_with_urls, parse_html_fragment, text_content_for_summaries, AttrsRefExt,
        QualNameExt, TendrilExt, Transform,
    },
    path::{hard_link_if_not_exists, PostsPath, SitePath},
    Author, ExtractedPost, FrontMatter, PostMeta,
};

pub fn extract_metadata(unsafe_html: &str, path: Option<PostsPath>) -> eyre::Result<ExtractedPost> {
    let dom = parse_html_fragment(unsafe_html.as_bytes())?;

    let mut meta = FrontMatter::default();
    let mut needs_attachments = BTreeSet::default();
    let mut og_image = None;
    let og_description = Some(text_content_for_summaries(dom.document.clone())?);
    let mut author_href = None;
    let mut author_name = None;
    let mut author_display_name = None;
    let mut author_display_handle = None;
    let mut transform = Transform::new(dom.document.clone());
    while transform.next(|kids, new_kids| {
        for kid in kids {
            if let NodeData::Element { name, attrs, .. } = &kid.data {
                let attrs = attrs.borrow();
                if name == &QualName::html("meta") {
                    let content = attrs.attr_str("content")?.map(|t| t.to_owned());
                    match attrs.attr_str("name")? {
                        Some("title") => {
                            meta.title = content;
                        }
                        Some("published") => {
                            meta.published = content;
                        }
                        Some("author_display_name") => {
                            author_display_name = content;
                        }
                        Some("author_display_handle") => {
                            author_display_handle = content;
                        }
                        Some("tags") => {
                            if let Some(tag) = content {
                                meta.tags.push(tag);
                            }
                        }
                        Some("is_transparent_share") => {
                            meta.is_transparent_share = true;
                        }
                        _ => {}
                    }
                    continue;
                } else if name == &QualName::html("link") {
                    let href = attrs.attr_str("href")?.map(|t| t.to_owned());
                    let name = attrs.attr_str("name")?.map(|t| t.to_owned());
                    match attrs.attr_str("rel")? {
                        Some("archived") => {
                            meta.archived = href;
                        }
                        Some("references") => {
                            if let Some(href) = href {
                                meta.references.push(PostsPath::from_references_url(&href)?);
                            }
                        }
                        Some("author") => {
                            author_href = href;
                            author_name = name;
                        }
                        _ => {}
                    }
                    continue;
                } else {
                    if let Some(attr_names) = html_attributes_with_urls().get(name) {
                        for attr in attrs.iter() {
                            if attr_names.contains(&attr.name) {
                                if let Ok(url) =
                                    SitePath::from_rendered_attachment_url(attr.value.to_str())
                                {
                                    trace!("found attachment url in rendered post: {url:?}");
                                    needs_attachments.insert(url);
                                }
                            }
                        }
                    }
                    if let Some(style) = attrs.attr_str("style")? {
                        for token in parse_inline_style(style) {
                            if let InlineStyleToken::Url(url) = token {
                                if let Ok(url) = SitePath::from_rendered_attachment_url(&url) {
                                    trace!("found attachment url in rendered post (inline styles): {url:?}");
                                    needs_attachments.insert(url);
                                }
                            }
                        }
                    }
                    // use the first <img src>, if any, as the <meta> og:image.
                    if og_image.is_none() && name == &QualName::html("img") {
                        if let Some(src) = attrs.attr_str("src")?.map(|t| t.to_owned()) {
                            og_image = Some(src);
                        }
                    }
                }
            }
            new_kids.push(kid.clone());
        }
        Ok(())
    })? {}

    if author_href.is_some()
        || author_name.is_some()
        || author_display_name.is_some()
        || author_display_handle.is_some()
    {
        meta.author = Some(Author {
            href: author_href.unwrap_or("".to_owned()),
            name: author_name.unwrap_or("".to_owned()),
            display_name: author_display_name.unwrap_or("".to_owned()),
            display_handle: author_display_handle.unwrap_or("".to_owned()),
        });
    }

    Ok(ExtractedPost {
        path,
        dom,
        meta: PostMeta {
            front_matter: meta,
            needs_attachments,
            og_image,
            og_description,
        },
    })
}

#[tracing::instrument(skip(site_paths))]
pub fn hard_link_attachments_into_site<'paths>(
    site_paths: impl IntoIterator<Item = &'paths SitePath>,
) -> eyre::Result<()> {
    for site_path in site_paths {
        trace!(?site_path);
        let attachments_path = site_path
            .attachments_path()?
            .ok_or_eyre("path is not an attachment path")?;
        let Some(parent) = site_path.parent() else {
            bail!("path has no parent: {site_path:?}");
        };
        create_dir_all(parent)?;
        hard_link_if_not_exists(attachments_path, site_path)?;
    }

    Ok(())
}

#[test]
fn test_extract_metadata() -> eyre::Result<()> {
    use crate::dom::serialize_html_fragment;
    let post = extract_metadata(r#"<meta name="title" content="foo">bar"#, None)?;
    assert_eq!(serialize_html_fragment(post.dom)?, "bar");
    assert_eq!(post.meta.front_matter.title.as_deref(), Some("foo"));

    Ok(())
}
