use std::{
    cell::RefCell,
    env::args,
    ffi::OsString,
    fs::{create_dir_all, read_dir, DirEntry, File},
    io::{Read, Write},
    path::Path,
};

use askama::Template;
use autost::{
    cli_init,
    cohost::{
        attachment_id_to_url, attachment_url_to_id, Ask, AskingProject, Ast, Attachment, Block,
        Post,
    },
    dom::{
        attr_value, convert_idl_to_content_attribute, create_element, create_fragment,
        debug_attributes_seen, debug_not_known_good_attributes_seen, find_attr_mut,
        make_attribute_name, parse, serialize, tendril_to_str, Traverse,
    },
    render_markdown, Author, PostMeta,
};
use html5ever::{local_name, namespace_url, ns, Attribute, LocalName, QualName};
use jane_eyre::eyre::{self, bail, eyre, Context};
use markup5ever_rcdom::{Node, NodeData, RcDom};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use reqwest::redirect::Policy;
use tracing::{debug, info, trace, warn};

fn main() -> eyre::Result<()> {
    cli_init()?;

    let input_path = args().nth(1).unwrap();
    let input_path = Path::new(&input_path);
    let output_path = args().nth(2).unwrap();
    let output_path = Path::new(&output_path);
    let attachments_path = args().nth(3).unwrap();
    let attachments_path = Path::new(&attachments_path);
    let specific_post_filenames = args().skip(4).map(OsString::from).collect::<Vec<_>>();
    let dir_entries = read_dir(input_path)?.collect::<Vec<_>>();

    let results = dir_entries
        .into_par_iter()
        .map(|entry| -> eyre::Result<Vec<String>> {
            let entry = entry?;
            if !specific_post_filenames.is_empty() {
                if !specific_post_filenames.contains(&entry.file_name()) {
                    return Ok(vec![]);
                }
            }
            let result = convert_chost(&entry, output_path)
                .wrap_err_with(|| eyre!("{:?}: failed to convert", entry.path()));
            Ok(result?)
        })
        .collect::<Vec<_>>();

    let mut all_attachment_ids = vec![];
    for result in results {
        all_attachment_ids.extend(result?);
    }

    let results = all_attachment_ids
        .into_par_iter()
        .map(|attachment_id| -> eyre::Result<()> {
            cache_attachment_image(&attachment_id, attachments_path)?;
            cache_attachment_thumb(&attachment_id, &attachments_path.join("thumbs"))?;
            Ok(())
        })
        .collect::<Vec<_>>();
    for result in results {
        result?;
    }

    trace!("saw html attributes: {:?}", debug_attributes_seen());
    let not_known_good_attributes_seen = debug_not_known_good_attributes_seen();
    if !not_known_good_attributes_seen.is_empty() {
        let attributes = not_known_good_attributes_seen
            .iter()
            .map(|(tag, attr)| format!("<{tag} {attr}>"))
            .collect::<Vec<_>>();
        let attributes = attributes.join(" ");
        warn!("saw attributes not on known-good-attributes list! check if output is correct for: {attributes}");
    }

    Ok(())
}

#[tracing::instrument(level = "error", skip(output_path))]
fn convert_chost(entry: &DirEntry, output_path: &Path) -> eyre::Result<Vec<String>> {
    let input_path = entry.path();

    trace!("parsing");
    let mut post: Post = serde_json::from_reader(File::open(&input_path)?)?;
    let post_id = post.postId;

    // each post has a “share tree”, a flat array of every post this post is in
    // reply to, from top to bottom.
    let shared_posts = post.shareTree;
    let shared_post_filenames = shared_posts
        .iter()
        .map(|post| format!("{}/{}.html", post_id, post.postId))
        .collect::<Vec<_>>();
    let shared_post_paths = shared_post_filenames
        .iter()
        .map(|shared_post_filename| output_path.join(shared_post_filename))
        .collect::<Vec<_>>();
    post.shareTree = vec![];

    if !shared_posts.is_empty() {
        create_dir_all(output_path.join(post_id.to_string()))?;
    }

    let mut attachment_ids = vec![];
    for (shared_post, output_path) in shared_posts.into_iter().zip(shared_post_paths) {
        convert_single_chost(
            shared_post,
            vec![],
            output_path.as_path(),
            &mut attachment_ids,
        )?;
    }

    let output_path = output_path.join(format!("{post_id}.html"));
    convert_single_chost(
        post,
        shared_post_filenames,
        output_path.as_path(),
        &mut attachment_ids,
    )?;

    Ok(attachment_ids)
}

fn convert_single_chost(
    post: Post,
    shared_post_filenames: Vec<String>,
    output_path: &Path,
    all_attachment_ids: &mut Vec<String>,
) -> eyre::Result<()> {
    info!("writing: {output_path:?}");
    let mut output = File::create(output_path)?;

    let meta = PostMeta {
        references: shared_post_filenames,
        title: Some(post.headline),
        published: Some(post.publishedAt),
        author: Some(Author {
            href: format!("https://cohost.org/{}", post.postingProject.handle),
            name: format!(
                "{} (@{})",
                post.postingProject.displayName, post.postingProject.handle
            ),
            display_name: post.postingProject.displayName,
            display_handle: format!("@{}", post.postingProject.handle),
        }),
        tags: post.tags,
        is_transparent_share: post.transparentShareOfPostId.is_some(),
    };
    output.write_all(meta.render()?.as_bytes())?;
    output.write_all(b"\n\n")?;

    let mut spans = post
        .astMap
        .spans
        .iter()
        .map(|span| -> eyre::Result<(Ast, usize, usize)> {
            Ok((
                serde_json::from_str(&span.ast)?,
                span.startIndex,
                span.endIndex,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    spans.sort_by_key(|(_ast, start, end)| (*start, *end));

    for (i, block) in post.blocks.into_iter().enumerate() {
        // posts in the cohost api provide an `astMap` that contains the perfect rendering of
        // markdown blocks. since our own markdown rendering is far from perfect, we use their
        // rendering instead of our own when available.
        while spans.first().map_or(false, |(_ast, _start, end)| i >= *end) {
            spans.remove(0);
        }
        if let Some((ast, start, end)) = match spans.first() {
            Some((_, _, end)) if i == *end - 1 => Some(spans.remove(0)),
            Some((_, start, end)) if (*start..*end).contains(&i) => continue,
            _ => None,
        } {
            trace!("replacing blocks {start}..{end} with ast");
            let dom = process_ast(ast);
            let ProcessChostFragmentResult {
                html,
                attachment_ids,
            } = process_chost_fragment(dom)?;
            output.write_all(html.as_bytes())?;
            all_attachment_ids.extend(attachment_ids);
            continue;
        }

        match block {
            Block::Markdown { markdown } => {
                let ProcessChostFragmentResult {
                    html,
                    attachment_ids,
                } = render_markdown_block(&markdown.content)?;
                output.write_all(html.as_bytes())?;
                all_attachment_ids.extend(attachment_ids);
                continue;
            }
            Block::Attachment { attachment } => match attachment {
                Attachment::Image {
                    attachmentId,
                    altText,
                    width,
                    height,
                } => {
                    all_attachment_ids.push(attachmentId.to_owned());
                    let template = CohostImgTemplate {
                        data_cohost_src: attachment_id_to_url(&attachmentId),
                        thumb_src: cached_attachment_thumb_url(&attachmentId),
                        src: cached_attachment_image_url(&attachmentId),
                        alt: altText,
                        width,
                        height,
                    };
                    output.write_all(template.render()?.as_bytes())?;
                }
                Attachment::Unknown { fields } => {
                    warn!("unknown attachment kind: {fields:?}");
                }
            },
            Block::Ask {
                ask:
                    Ask {
                        askingProject,
                        content,
                        ..
                    },
            } => {
                let ProcessChostFragmentResult {
                    html,
                    attachment_ids,
                } = render_markdown_block(&content)?;
                let template = AskTemplate {
                    author: askingProject,
                    content: html,
                };
                output.write_all(template.render()?.as_bytes())?;
                all_attachment_ids.extend(attachment_ids);
                continue;
            }
            Block::Unknown { fields } => {
                warn!("unknown block type: {fields:?}");
            }
        }
        output.write_all(b"\n\n")?;
    }

    Ok(())
}

fn process_ast(root: Ast) -> RcDom {
    let (dom, html_root) = create_fragment();
    let mut ast_queue = vec![(root, html_root.clone())];

    while !ast_queue.is_empty() {
        let (node, parent) = ast_queue.remove(0);

        match node {
            Ast::Root { children } => {
                ast_queue.extend(
                    children
                        .into_iter()
                        .map(|node| (node, parent.clone()))
                        .collect::<Vec<_>>(),
                );
            }
            Ast::Element {
                tagName,
                properties,
                children,
            } => {
                let name = QualName::new(None, ns!(html), LocalName::from(tagName.clone()));
                let attrs = properties
                    .into_iter()
                    .filter_map(|(name, value)| {
                        // the `astMap` contains idl attributes like `<details>.open=true` and `<ol>.start=2`, not
                        // content attributes like `<details open>` and `<ol start="2">`.
                        convert_idl_to_content_attribute(&tagName, &name, value)
                    })
                    .collect::<Vec<_>>()
                    .into();
                let element = Node::new(NodeData::Element {
                    name,
                    attrs,
                    template_contents: RefCell::new(None),
                    mathml_annotation_xml_integration_point: false,
                });

                parent.children.borrow_mut().push(element.clone());
                ast_queue.extend(
                    children
                        .into_iter()
                        .map(|node| (node, element.clone()))
                        .collect::<Vec<_>>(),
                );
            }
            Ast::Text { value } => {
                let text = Node::new(NodeData::Text {
                    contents: RefCell::new(value.into()),
                });
                parent.children.borrow_mut().push(text);
            }
        }
    }

    dom
}

#[derive(Template)]
#[template(path = "cohost-img.html")]
struct CohostImgTemplate {
    data_cohost_src: String,
    thumb_src: String,
    src: String,
    alt: String,
    width: usize,
    height: usize,
}

#[derive(Template)]
#[template(path = "ask.html")]
struct AskTemplate {
    author: Option<AskingProject>,
    content: String,
}

#[derive(Debug, PartialEq)]
struct ProcessChostFragmentResult {
    html: String,
    attachment_ids: Vec<String>,
}

fn render_markdown_block(markdown: &str) -> eyre::Result<ProcessChostFragmentResult> {
    let html = render_markdown(markdown);
    let dom = parse(html.as_bytes())?;

    process_chost_fragment(dom)
}

fn process_chost_fragment(mut dom: RcDom) -> eyre::Result<ProcessChostFragmentResult> {
    let mut attachment_ids = vec![];

    // rewrite cohost attachment urls to relative cached paths.
    for node in Traverse::new(dom.document.clone()) {
        match &node.data {
            NodeData::Element { name, attrs, .. } => {
                let img = QualName::new(None, ns!(html), local_name!("img"));
                let a = QualName::new(None, ns!(html), local_name!("a"));
                let element_attr_names = match name {
                    name if name == &img => Some(("img", "src")),
                    name if name == &a => Some(("a", "href")),
                    _ => None,
                };
                if let Some((element_name, attr_name)) = element_attr_names {
                    let mut attrs = attrs.borrow_mut();
                    if let Some(attr) = find_attr_mut(&mut attrs, attr_name) {
                        let old_url = tendril_to_str(&attr.value)?.to_owned();
                        if let Some(id) = attachment_url_to_id(&old_url) {
                            trace!("found cohost attachment url in <{element_name} {attr_name}>: {old_url}");
                            attachment_ids.push(id.to_owned());
                            attr.value = cached_attachment_image_url(id).into();
                            attrs.push(Attribute {
                                name: QualName::new(
                                    None,
                                    ns!(),
                                    LocalName::from(format!("data-cohost-{attr_name}")),
                                ),
                                value: old_url.into(),
                            });
                        }
                    }
                    if element_name == "img" {
                        attrs.push(Attribute {
                            name: make_attribute_name("loading"),
                            value: "lazy".into(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // rewrite `<Mention handle>` elements into ordinary links.
    let mut queue = vec![dom.document.clone()];
    while !queue.is_empty() {
        let node = queue.remove(0);
        let mut children = vec![];
        for kid in node.children.borrow().iter() {
            match &kid.data {
                NodeData::Element { name, attrs, .. } => {
                    let attrs = attrs.borrow();
                    let handle =
                        if name == &QualName::new(None, ns!(html), LocalName::from("Mention")) {
                            attr_value(&attrs, "handle")?
                        } else {
                            None
                        };
                    if let Some(handle) = handle {
                        let new_kid = create_element(&mut dom, "a");
                        new_kid.children.replace(kid.children.take());
                        let NodeData::Element { attrs, .. } = &new_kid.data else {
                            bail!("irrefutable! guaranteed by create_element");
                        };
                        attrs.borrow_mut().push(Attribute {
                            name: QualName::new(None, ns!(), local_name!("href")),
                            value: format!("https://cohost.org/{handle}").into(),
                        });
                        children.push(new_kid);
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

    Ok(ProcessChostFragmentResult {
        html: serialize(dom)?,
        attachment_ids,
    })
}

fn cached_attachment_image_url(id: &str) -> String {
    format!("attachments/{id}")
}

fn cached_attachment_thumb_url(id: &str) -> String {
    format!("attachments/thumbs/{id}")
}

fn cache_attachment_image(id: &str, attachments_path: &Path) -> eyre::Result<()> {
    debug!("caching attachment image: {id}");
    let url = attachment_id_to_url(id);
    let path = attachments_path.join(id);
    cached_get(&url, &path, None)?;

    Ok(())
}

fn cache_attachment_thumb(id: &str, attachments_path: &Path) -> eyre::Result<()> {
    fn thumb(url: &str) -> String {
        format!("{url}?width=675")
    }

    debug!("caching attachment thumb: {id}");
    let url = attachment_id_to_url(id);
    let path = attachments_path.join(id);
    cached_get(&url, &path, Some(thumb))?;

    Ok(())
}

fn cached_get(
    url: &str,
    path: &Path,
    transform_redirect_target: Option<fn(&str) -> String>,
) -> eyre::Result<Vec<u8>> {
    if let Ok(mut file) = File::open(path) {
        trace!("cache hit: {url}");
        let mut result = Vec::default();
        file.read_to_end(&mut result)?;
        return Ok(result);
    }

    trace!("cache miss: {url}");

    // cohost attachment redirects don’t preserve query params, so if we want to add any,
    // we need to add them to the destination of the redirect.
    // FIXME: this will silently misbehave if the endpoint introduces a second redirect!
    let transformed_url = if let Some(transform) = transform_redirect_target {
        let client = reqwest::blocking::Client::builder()
            .redirect(Policy::none())
            .build()?;
        let redirect = client.head(url).send()?;
        if let Some(location) = redirect.headers().get("location") {
            Some(transform(location.to_str()?))
        } else {
            Some(transform(url))
        }
    } else {
        None
    };

    let url = if let Some(transformed_url) = transformed_url {
        trace!("transformed redirect target: {transformed_url}");
        transformed_url
    } else {
        url.to_owned()
    };
    let result = reqwest::blocking::get(url)?.bytes()?.to_vec();
    File::create(path)?.write_all(&result)?;

    Ok(result)
}

#[test]
fn test_render_markdown_block() -> eyre::Result<()> {
    fn result(html: &str, attachment_ids: &[&str]) -> ProcessChostFragmentResult {
        ProcessChostFragmentResult {
            html: html.to_owned(),
            attachment_ids: attachment_ids.iter().map(|&x| x.to_owned()).collect(),
        }
    }
    let n = "\n";

    assert_eq!(
        render_markdown_block("text")?,
        result(&format!(r#"<p>text</p>{n}"#), &[])
    );
    assert_eq!(render_markdown_block("![text](https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444)")?,
        result(&format!(r#"<p><img src="attachments/44444444-4444-4444-4444-444444444444" alt="text" data-cohost-src="https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444" loading="lazy"></p>{n}"#), &["44444444-4444-4444-4444-444444444444"]));
    assert_eq!(render_markdown_block("<img src=https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444>")?,
        result(&format!(r#"<img src="attachments/44444444-4444-4444-4444-444444444444" data-cohost-src="https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444" loading="lazy">{n}"#), &["44444444-4444-4444-4444-444444444444"]));
    assert_eq!(render_markdown_block("[text](https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444)")?,
        result(&format!(r#"<p><a href="attachments/44444444-4444-4444-4444-444444444444" data-cohost-href="https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444">text</a></p>{n}"#), &["44444444-4444-4444-4444-444444444444"]));
    assert_eq!(render_markdown_block("<a href=https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444>text</a>")?,
        result(&format!(r#"<p><a href="attachments/44444444-4444-4444-4444-444444444444" data-cohost-href="https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444">text</a></p>{n}"#), &["44444444-4444-4444-4444-444444444444"]));

    Ok(())
}
