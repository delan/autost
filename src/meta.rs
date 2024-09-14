use html5ever::{local_name, namespace_url, ns, QualName};
use jane_eyre::eyre;
use markup5ever_rcdom::NodeData;

use crate::{
    dom::{attr_value, parse, serialize},
    ExtractedPost, PostMeta,
};

pub fn extract_metadata(unsafe_html: &str) -> eyre::Result<ExtractedPost> {
    let dom = parse(&mut unsafe_html.as_bytes())?;

    let mut meta = PostMeta::default();
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
                            Some("tags") => {
                                if let Some(tag) = content {
                                    meta.tags.push(tag);
                                }
                            }
                            _ => {}
                        }
                        continue;
                    } else if name == &QualName::new(None, ns!(html), local_name!("link")) {
                        let name = attr_value(&attrs.borrow(), "name")?.map(|t| t.to_owned());
                        let href = attr_value(&attrs.borrow(), "href")?.map(|t| t.to_owned());
                        match attr_value(&attrs.borrow(), "rel")? {
                            Some("references") => {
                                if let Some(href) = href {
                                    meta.references.push(href);
                                }
                            }
                            Some("author") => {
                                meta.author = href.zip(name);
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

    Ok(ExtractedPost {
        unsafe_html: serialize(dom)?,
        meta,
    })
}

#[test]
fn test_extract_metadata() -> eyre::Result<()> {
    fn post(
        unsafe_html: &str,
        references: &[&str],
        title: Option<&str>,
        published: Option<&str>,
        author: Option<(&str, &str)>,
        tags: &[&str],
    ) -> ExtractedPost {
        ExtractedPost {
            unsafe_html: unsafe_html.to_owned(),
            meta: PostMeta {
                references: references.iter().map(|&url| url.to_owned()).collect(),
                title: title.map(|t| t.to_owned()),
                published: published.map(|t| t.to_owned()),
                author: author.map(|(name, href)| (name.to_owned(), href.to_owned())),
                tags: tags.iter().map(|&tag| tag.to_owned()).collect(),
            },
        }
    }
    assert_eq!(
        extract_metadata(r#"<meta name="title" content="foo">bar"#)?,
        post("bar", &[], Some("foo"), None, None, &[]),
    );

    Ok(())
}
