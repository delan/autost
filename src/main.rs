use std::{borrow::Borrow, env::args, fs::File, io::Read, str};

use askama::Template;
use comrak::Options;
use html5ever::{
    local_name, namespace_url, ns,
    tendril::{StrTendril, TendrilSink},
    tree_builder::TreeBuilderOpts,
    Attribute, LocalName, Namespace, ParseOpts, QualName,
};
use jane_eyre::eyre;
use markup5ever_rcdom::{NodeData, RcDom, SerializableHandle};

#[derive(askama::Template)]
#[template(path = "post.html")]
struct PostTemplate<'input> {
    title: &'input str,
    content: &'input str,
}

fn main() -> eyre::Result<()> {
    jane_eyre::install()?;

    let path = args().nth(1).unwrap();
    let mut file = File::open(path)?;
    let mut markdown = String::default();
    file.read_to_string(&mut markdown)?;

    // author step: render markdown to html.
    let mut options = Options::default();
    options.render.unsafe_ = true;
    let unsafe_html = comrak::markdown_to_html(&markdown, &options);

    // reader step: extract metadata.
    let post = extract_metadata(unsafe_html)?;

    // reader step: filter html.
    let safe_html = ammonia::Builder::default()
        .add_generic_attributes(["style"])
        .add_tag_attributes("details", ["open"])
        .add_tags(["meta"])
        .add_tag_attributes("meta", ["name", "content"])
        .id_prefix(Some("user-content-")) // cohost compatibility
        .clean(&post.unsafe_html)
        .to_string();

    // reader step: generate post page.
    let template = PostTemplate {
        title: &post.title.unwrap_or("".to_owned()),
        content: &safe_html,
    };
    println!("{}", template.render()?);

    Ok(())
}

struct Post {
    unsafe_html: String,
    title: Option<String>,
}

fn extract_metadata(unsafe_html: String) -> eyre::Result<Post> {
    let options = ParseOpts {
        tree_builder: TreeBuilderOpts {
            drop_doctype: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let context = QualName::new(None, ns!(html), local_name!("section"));
    let dom = html5ever::parse_fragment(RcDom::default(), options, context, vec![])
        .from_utf8()
        .read_from(&mut unsafe_html.as_bytes())?;

    let mut title = None;
    let mut queue = vec![dom.document.clone()];
    while !queue.is_empty() {
        let node = queue.remove(0);
        let mut children = vec![];
        for kid in node.children.borrow().iter() {
            match &kid.data {
                NodeData::Element { name, attrs, .. } => {
                    if name == &QualName::new(None, ns!(html), local_name!("meta")) {
                        if attr_value(&attrs.borrow(), "name")? == Some("title") {
                            title = attr_value(&attrs.borrow(), "content")?.map(|t| t.to_owned());
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

    let document: SerializableHandle = dom.document.clone().into();
    let mut unsafe_html = Vec::default();
    html5ever::serialize(&mut unsafe_html, &document, Default::default())?;
    let unsafe_html = String::from_utf8(unsafe_html)?;

    Ok(Post { unsafe_html, title })
}

fn attr_value<'attrs>(attrs: &'attrs [Attribute], name: &str) -> eyre::Result<Option<&'attrs str>> {
    for attr in attrs.iter() {
        if dbg!(&attr.name)
            == dbg!(&QualName::new(
                None,
                Namespace::default(),
                LocalName::from(name)
            ))
        {
            dbg!(attr);
            return Ok(Some(tendril_to_owned(&attr.value)?));
        }
    }

    Ok(None)
}

fn tendril_to_owned(tendril: &StrTendril) -> eyre::Result<&str> {
    Ok(str::from_utf8(tendril.borrow())?)
}