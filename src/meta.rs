use std::{collections::BTreeSet, fs::create_dir_all};

use html5ever::QualName;
use jane_eyre::eyre::{self, bail, OptionExt};
use markup5ever_rcdom::NodeData;
use tracing::trace;

use crate::{
    dom::{parse, serialize, AttrsRefExt, QualNameExt, TendrilExt, Transform},
    path::{hard_link_if_not_exists, PostsPath, SitePath},
    Author, ExtractedPost, PostMeta,
};

pub fn extract_metadata(unsafe_html: &str) -> eyre::Result<ExtractedPost> {
    let dom = parse(&mut unsafe_html.as_bytes())?;

    let mut meta = PostMeta::default();
    let mut needs_attachments = BTreeSet::default();
    let mut author_href = None;
    let mut author_name = None;
    let mut author_display_name = None;
    let mut author_display_handle = None;
    let mut transform = Transform::new(dom.document.clone());
    while transform.next(|kids, new_kids| {
        for kid in kids {
            match &kid.data {
                NodeData::Element { name, attrs, .. } => {
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
                        for attr in attrs.iter() {
                            if let Ok(url) =
                                SitePath::from_rendered_attachment_url(attr.value.to_str())
                            {
                                trace!("found attachment url in rendered post: {url:?}");
                                needs_attachments.insert(url);
                            }
                        }
                    }
                }
                _ => {}
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
        unsafe_html: serialize(dom)?,
        meta,
        needs_attachments,
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
        hard_link_if_not_exists(attachments_path, &site_path)?;
    }

    Ok(())
}

#[test]
fn test_extract_metadata() -> eyre::Result<()> {
    fn post(
        unsafe_html: &str,
        archived: Option<&str>,
        references: &[PostsPath],
        title: Option<&str>,
        published: Option<&str>,
        author: Option<Author>,
        tags: &[&str],
        is_transparent_share: bool,
        needs_attachments: &[SitePath],
    ) -> ExtractedPost {
        ExtractedPost {
            unsafe_html: unsafe_html.to_owned(),
            meta: PostMeta {
                archived: archived.map(|a| a.to_owned()),
                references: references.iter().map(|url| url.to_owned()).collect(),
                title: title.map(|t| t.to_owned()),
                published: published.map(|t| t.to_owned()),
                author,
                tags: tags.iter().map(|&tag| tag.to_owned()).collect(),
                is_transparent_share,
            },
            needs_attachments: needs_attachments.iter().map(|url| url.to_owned()).collect(),
        }
    }
    assert_eq!(
        extract_metadata(r#"<meta name="title" content="foo">bar"#)?,
        post("bar", None, &[], Some("foo"), None, None, &[], false, &[]),
    );

    Ok(())
}
