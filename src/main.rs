use std::{
    env::args,
    fs::File,
    io::{Read, Write},
    path::Path,
};

use askama::Template;
use autost::{
    cli_init,
    dom::{attr_value, parse, serialize},
    render_markdown,
};
use html5ever::{local_name, namespace_url, ns, QualName};
use jane_eyre::eyre::{self, OptionExt};
use markup5ever_rcdom::NodeData;
use tracing::info;

#[derive(Clone, Debug, Template)]
#[template(path = "posts.html")]
struct PostsTemplate {
    posts: Vec<TemplatedPost>,
}

#[derive(Clone, Debug)]
struct TemplatedPost {
    post_page_href: String,
    title: String,
    published: String,
    content: String,
}

fn main() -> eyre::Result<()> {
    cli_init()?;

    let mut posts = vec![];

    let output_path = args().nth(1).unwrap();
    let output_path = Path::new(&output_path);

    for path in args().skip(2) {
        let path = Path::new(&path);
        let mut file = File::open(&path)?;
        let mut unsafe_source = String::default();
        file.read_to_string(&mut unsafe_source)?;

        let unsafe_html = if path.ends_with(".md") {
            // author step: render markdown to html.
            render_markdown(&unsafe_source)
        } else {
            unsafe_source
        };

        // reader step: extract metadata.
        let post = extract_metadata(&unsafe_html)?;

        // reader step: filter html.
        let safe_html = ammonia::Builder::default()
            .add_generic_attributes(["style", "id"])
            .add_generic_attributes(["data-cohost-href", "data-cohost-src"]) // cohost2autost
            .add_tag_attributes("details", ["open"])
            .add_tag_attributes("img", ["loading"])
            .add_tags(["meta"])
            .add_tag_attributes("meta", ["name", "content"])
            .id_prefix(Some("user-content-")) // cohost compatibility
            .clean(&post.unsafe_html)
            .to_string();

        let original_name = path.file_name().ok_or_eyre("post has no file name")?;
        let original_name = original_name.to_str().ok_or_eyre("unsupported file name")?;
        let (post_page_name, _) = original_name
            .rsplit_once(".")
            .unwrap_or((original_name, ""));
        let post_page_name = format!("{post_page_name}.html");
        let post = TemplatedPost {
            post_page_href: post_page_name.clone(),
            title: post.title.unwrap_or("".to_owned()),
            published: post.published.unwrap_or("".to_owned()),
            content: safe_html,
        };

        // generate post page.
        let template = PostsTemplate {
            posts: vec![post.clone()],
        };
        let post_page_path = output_path.join(post_page_name);
        info!("writing post page: {post_page_path:?}");
        writeln!(File::create(post_page_path)?, "{}", template.render()?)?;

        posts.push(post);
    }

    // reader step: generate posts page.
    posts.sort_by(|p, q| p.published.cmp(&q.published).reverse());
    let template = PostsTemplate { posts };
    let posts_page_path = output_path.join("index.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;

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
