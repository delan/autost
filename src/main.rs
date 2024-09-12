use std::{env::args, fs::File, io::Read};

use askama::Template;
use autost::{
    dom::{attr_value, parse, serialize},
    render_markdown,
};
use html5ever::{local_name, namespace_url, ns, QualName};
use jane_eyre::eyre;
use markup5ever_rcdom::NodeData;

#[derive(Template)]
#[template(path = "posts.html")]
struct PostsTemplate {
    posts: Vec<PostTemplate>,
}

struct PostTemplate {
    title: String,
    published: String,
    content: String,
}

fn main() -> eyre::Result<()> {
    jane_eyre::install()?;

    let mut posts = vec![];

    for path in args().skip(1) {
        let mut file = File::open(path)?;
        let mut markdown = String::default();
        file.read_to_string(&mut markdown)?;

        // author step: render markdown to html.
        let unsafe_html = render_markdown(&markdown);

        // reader step: extract metadata.
        let post = extract_metadata(&unsafe_html)?;

        // reader step: filter html.
        let safe_html = ammonia::Builder::default()
            .add_generic_attributes(["style", "id"])
            .add_tag_attributes("details", ["open"])
            .add_tag_attributes("img", ["loading"])
            .add_tags(["meta"])
            .add_tag_attributes("meta", ["name", "content"])
            .id_prefix(Some("user-content-")) // cohost compatibility
            .clean(&post.unsafe_html)
            .to_string();

        posts.push(PostTemplate {
            title: post.title.unwrap_or("".to_owned()),
            published: post.published.unwrap_or("".to_owned()),
            content: safe_html,
        });
    }

    // reader step: generate posts page.
    posts.sort_by(|p, q| p.published.cmp(&q.published).reverse());
    let template = PostsTemplate { posts };
    println!("{}", template.render()?);

    Ok(())
}

#[derive(Debug, PartialEq)]
struct Post {
    unsafe_html: String,
    title: Option<String>,
    published: Option<String>,
}

fn extract_metadata(unsafe_html: &str) -> eyre::Result<Post> {
    let dom = parse(&mut unsafe_html.as_bytes())?;

    let mut title = None;
    let mut published = None;
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
                                title = content;
                            }
                            Some("published") => {
                                published = content;
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

    Ok(Post {
        unsafe_html: serialize(dom)?,
        title,
        published,
    })
}

#[test]
fn test_extract_metadata() -> eyre::Result<()> {
    fn post(unsafe_html: &str, title: Option<&str>, published: Option<&str>) -> Post {
        Post {
            unsafe_html: unsafe_html.to_owned(),
            title: title.map(|t| t.to_owned()),
            published: published.map(|t| t.to_owned()),
        }
    }
    assert_eq!(
        extract_metadata(r#"<meta name="title" content="foo">bar"#)?,
        post("bar", Some("foo"), None)
    );

    Ok(())
}
