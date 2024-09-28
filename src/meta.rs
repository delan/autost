use html5ever::{local_name, namespace_url, ns, QualName};
use jane_eyre::eyre;
use markup5ever_rcdom::NodeData;

use crate::{
    dom::{attr_value, parse, serialize},
    path::PostsPath,
    Author, ExtractedPost, PostMeta,
};

pub fn extract_metadata(unsafe_html: &str) -> eyre::Result<ExtractedPost> {
    let dom = parse(&mut unsafe_html.as_bytes())?;

    let mut meta = PostMeta::default();
    let mut author_href = None;
    let mut author_name = None;
    let mut author_display_name = None;
    let mut author_display_handle = None;
    let mut queue = vec![dom.document.clone()];
    while !queue.is_empty() {
        let node = queue.remove(0);
        let mut children = vec![];
        for kid in node.children.borrow().iter() {
            match &kid.data {
                NodeData::Element { name, attrs, .. } => {
                    if name == &QualName::new(None, ns!(html), local_name!("meta")) {
                        let content = attr_value(&attrs.borrow(), "content")?.map(|t| t.to_owned());
                        match attr_value(&attrs.borrow(), "name")? {
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
                    } else if name == &QualName::new(None, ns!(html), local_name!("link")) {
                        let href = attr_value(&attrs.borrow(), "href")?.map(|t| t.to_owned());
                        let name = attr_value(&attrs.borrow(), "name")?.map(|t| t.to_owned());
                        match attr_value(&attrs.borrow(), "rel")? {
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
                    }
                }
                _ => {}
            }
            children.push(kid.clone());
            queue.push(kid.clone());
        }
        node.children.replace(children);
    }

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
    })
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
        }
    }
    assert_eq!(
        extract_metadata(r#"<meta name="title" content="foo">bar"#)?,
        post("bar", None, &[], Some("foo"), None, None, &[], false),
    );

    Ok(())
}
