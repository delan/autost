use std::{
    cell::RefCell,
    ffi::OsString,
    fs::{create_dir_all, read_dir, DirEntry, File},
    io::{Read, Write},
    path::Path,
};

use askama::Template;
use html5ever::{local_name, namespace_url, ns, Attribute, LocalName, QualName};
use jane_eyre::eyre::{self, bail, eyre, Context};
use markup5ever_rcdom::{Node, NodeData, RcDom};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use reqwest::redirect::Policy;
use tracing::{debug, info, trace, warn};

use crate::{
    cohost::{
        attachment_id_to_url, attachment_url_to_id, Ask, AskingProject, Ast, Attachment, Block,
        Post,
    },
    dom::{
        attr_value, convert_idl_to_content_attribute, create_element, create_fragment,
        debug_attributes_seen, debug_not_known_good_attributes_seen, find_attr_mut,
        make_attribute_name, parse, serialize, tendril_to_str, Traverse,
    },
    migrations::run_migrations,
    path::{hard_link_if_not_exists, AttachmentsPath, PostsPath, SitePath},
    render_markdown, Author, PostMeta,
};

pub fn main(mut args: impl Iterator<Item = String>) -> eyre::Result<()> {
    run_migrations()?;

    let input_path = args.next().unwrap();
    let input_path = Path::new(&input_path);
    let specific_post_filenames = args.map(OsString::from).collect::<Vec<_>>();
    let dir_entries = read_dir(input_path)?.collect::<Vec<_>>();
    create_dir_all(&*PostsPath::ROOT)?;
    create_dir_all(&*SitePath::ATTACHMENTS)?;
    create_dir_all(&*SitePath::THUMBS)?;

    let results = dir_entries
        .into_par_iter()
        .map(|entry| -> eyre::Result<()> {
            let entry = entry?;
            if !specific_post_filenames.is_empty() {
                if !specific_post_filenames.contains(&entry.file_name()) {
                    return Ok(());
                }
            }
            convert_chost(&entry, &RealConvertChostContext)
                .wrap_err_with(|| eyre!("{:?}: failed to convert", entry.path()))?;
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

trait ConvertChostContext {
    fn cache_attachment_file(&self, id: &str) -> eyre::Result<SitePath>;
    fn cache_attachment_thumb(&self, id: &str) -> eyre::Result<SitePath>;
}
struct RealConvertChostContext;
impl ConvertChostContext for RealConvertChostContext {
    #[tracing::instrument(skip(self))]
    fn cache_attachment_file(&self, id: &str) -> eyre::Result<SitePath> {
        let url = attachment_id_to_url(id);
        let dir = &*AttachmentsPath::ROOT;
        let path = dir.join(id)?;
        create_dir_all(&path)?;
        cached_get_attachment(&url, &path, None)?;

        let attachments_path = cached_attachment_url(id, dir)?;
        let site_path = attachments_path.site_path()?;
        let Some(parent) = site_path.parent() else {
            bail!("path has no parent: {site_path:?}");
        };
        create_dir_all(parent)?;
        hard_link_if_not_exists(attachments_path, &site_path)?;

        Ok(site_path)
    }

    #[tracing::instrument(skip(self))]
    fn cache_attachment_thumb(&self, id: &str) -> eyre::Result<SitePath> {
        fn thumb(url: &str) -> String {
            format!("{url}?width=675")
        }

        let url = attachment_id_to_url(id);
        let dir = &*AttachmentsPath::THUMBS;
        let path = dir.join(id)?;
        create_dir_all(&path)?;
        cached_get_attachment(&url, &path, Some(thumb))?;

        let attachments_path = cached_attachment_url(id, dir)?;
        let site_path = attachments_path.site_path()?;
        let Some(parent) = site_path.parent() else {
            bail!("path has no parent: {site_path:?}");
        };
        create_dir_all(parent)?;
        hard_link_if_not_exists(attachments_path, &site_path)?;

        Ok(site_path)
    }
}

#[tracing::instrument(level = "error", skip(context))]
fn convert_chost(entry: &DirEntry, context: &dyn ConvertChostContext) -> eyre::Result<()> {
    let input_path = entry.path();

    trace!("parsing");
    let mut post: Post = serde_json::from_reader(File::open(&input_path)?)?;
    let post_id = post.postId;

    // each post has a “share tree”, a flat array of every post this post is in
    // reply to, from top to bottom.
    let shared_posts = post.shareTree;
    let shared_post_filenames = shared_posts
        .iter()
        .map(|post| PostsPath::references_post_path(post_id, post.postId))
        .collect::<Vec<_>>();
    post.shareTree = vec![];

    if !shared_posts.is_empty() {
        create_dir_all(PostsPath::references_dir(post_id))?;
    }

    for (shared_post, output_path) in shared_posts.into_iter().zip(shared_post_filenames.iter()) {
        convert_single_chost(shared_post, vec![], &output_path, context)?;
    }

    let output_path = PostsPath::generated_post_path(post_id);
    convert_single_chost(post, shared_post_filenames, &output_path, context)?;

    Ok(())
}

fn convert_single_chost(
    post: Post,
    shared_post_filenames: Vec<PostsPath>,
    output_path: &PostsPath,
    context: &dyn ConvertChostContext,
) -> eyre::Result<()> {
    info!("writing: {output_path:?}");
    let mut output = File::create(output_path)?;

    let meta = PostMeta {
        archived: Some(format!(
            "https://cohost.org/{}/post/{}",
            post.postingProject.handle, post.filename
        )),
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
            let html = process_chost_fragment(dom, context)?;
            output.write_all(html.as_bytes())?;
            continue;
        }

        let mut handle_attachment = |attachment| -> eyre::Result<()> {
            match attachment {
                Attachment::Image {
                    attachmentId,
                    altText,
                    width,
                    height,
                } => {
                    let template = CohostImgTemplate {
                        data_cohost_src: attachment_id_to_url(&attachmentId),
                        thumb_src: context.cache_attachment_thumb(&attachmentId)?,
                        src: context.cache_attachment_file(&attachmentId)?,
                        alt: altText,
                        width,
                        height,
                    };
                    output.write_all(template.render()?.as_bytes())?;
                }
                Attachment::Audio {
                    attachmentId,
                    artist,
                    title,
                } => {
                    let template = CohostAudioTemplate {
                        data_cohost_src: attachment_id_to_url(&attachmentId),
                        src: context.cache_attachment_file(&attachmentId)?,
                        artist,
                        title,
                    };
                    output.write_all(template.render()?.as_bytes())?;
                }
                Attachment::Unknown { fields } => {
                    warn!("unknown attachment kind: {fields:?}");
                }
            }
            Ok(())
        };

        match block {
            Block::Markdown { markdown } => {
                let html = render_markdown_block(&markdown.content, context)?;
                output.write_all(html.as_bytes())?;
                continue;
            }
            Block::Attachment { attachment } => handle_attachment(attachment)?,
            Block::Ask {
                ask:
                    Ask {
                        askingProject,
                        content,
                        ..
                    },
            } => {
                let html = render_markdown_block(&content, context)?;
                let template = AskTemplate {
                    author: askingProject,
                    content: html,
                };
                output.write_all(template.render()?.as_bytes())?;
                continue;
            }
            Block::AttachmentRow { attachments } => {
                for block in attachments {
                    match block {
                        Block::Attachment { attachment } => handle_attachment(attachment)?,
                        _ => warn!("AttachmentRow should only have Attachment blocks, but we got: {block:?}"),
                    }
                }
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

                // sort the properties by attribute name, to avoid spurious output diffs.
                let mut properties = properties.into_iter().collect::<Vec<_>>();
                properties.sort_by(|(n1, _v1), (n2, _v2)| n1.cmp(n2));

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
    thumb_src: SitePath,
    src: SitePath,
    alt: Option<String>,
    width: Option<usize>,
    height: Option<usize>,
}

#[derive(Template)]
#[template(path = "cohost-audio.html")]
struct CohostAudioTemplate {
    data_cohost_src: String,
    src: SitePath,
    artist: String,
    title: String,
}

#[derive(Template)]
#[template(path = "ask.html")]
struct AskTemplate {
    author: Option<AskingProject>,
    content: String,
}

fn render_markdown_block(
    markdown: &str,
    context: &dyn ConvertChostContext,
) -> eyre::Result<String> {
    let html = render_markdown(markdown);
    let dom = parse(html.as_bytes())?;

    process_chost_fragment(dom, context)
}

fn process_chost_fragment(
    mut dom: RcDom,
    context: &dyn ConvertChostContext,
) -> eyre::Result<String> {
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
                            attr.value = context
                                .cache_attachment_file(id)?
                                .base_relative_url()
                                .into();
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

    Ok(serialize(dom)?)
}

fn cached_attachment_url(id: &str, dir: &AttachmentsPath) -> eyre::Result<AttachmentsPath> {
    let path = dir.join(id)?;
    let mut entries = read_dir(&path)?;
    let Some(entry) = entries.next() else {
        bail!("directory is empty: {path:?}");
    };

    Ok(path.join_dir_entry(&entry?)?)
}

fn cached_get_attachment(
    url: &str,
    path: &AttachmentsPath,
    transform_redirect_target: Option<fn(&str) -> String>,
) -> eyre::Result<AttachmentsPath> {
    // if the attachment id directory exists...
    if let Ok(mut entries) = read_dir(path) {
        // and the directory contains a file...
        if let Some(entry) = entries.next() {
            // and we can open the file...
            // TODO: move this logic into path module
            let path = path.join_dir_entry(&entry?)?;
            if let Ok(mut file) = File::open(&path) {
                trace!("cache hit: {url}");
                // check if we can read the file.
                let mut result = Vec::default();
                file.read_to_end(&mut result)?;
                return Ok(path);
            }
        }
    }

    trace!("cache miss: {url}");
    debug!("downloading attachment");

    let client = reqwest::blocking::Client::builder()
        .redirect(Policy::none())
        .build()?;
    let redirect = client.head(url).send()?;

    let Some(url) = redirect.headers().get("location") else {
        bail!("expected redirect but got {}: {url}", redirect.status());
    };
    let url = url.to_str()?;

    let Some((_, original_filename)) = url.rsplit_once("/") else {
        bail!("redirect target has no slashes: {url}");
    };
    let original_filename = urlencoding::decode(original_filename)?;
    trace!("original filename: {original_filename}");

    // cohost attachment redirects don’t preserve query params, so if we want to add any,
    // we need to add them to the destination of the redirect.
    // FIXME: this will silently misbehave if the endpoint introduces a second redirect!
    let url = if let Some(transform) = transform_redirect_target {
        let transformed_url = transform(url);
        trace!("transformed redirect target: {transformed_url}");
        transformed_url
    } else {
        url.to_owned()
    };

    let path = path.join(original_filename.as_ref())?;
    let result = reqwest::blocking::get(url)?.bytes()?.to_vec();
    File::create(&path)?.write_all(&result)?;

    Ok(path)
}

#[test]
fn test_render_markdown_block() -> eyre::Result<()> {
    struct TestConvertChostContext {}
    impl ConvertChostContext for TestConvertChostContext {
        fn cache_attachment_file(&self, id: &str) -> eyre::Result<SitePath> {
            Ok(SitePath::ATTACHMENTS.join(&format!("{id}"))?)
        }
        fn cache_attachment_thumb(&self, id: &str) -> eyre::Result<SitePath> {
            Ok(SitePath::THUMBS.join(&format!("{id}"))?)
        }
    }

    let n = "\n";
    let context = TestConvertChostContext {};
    assert_eq!(
        render_markdown_block("text", &context)?,
        format!(r#"<p>text</p>{n}"#)
    );
    assert_eq!(render_markdown_block("![text](https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444)", &context)?,
        format!(r#"<p><img src="attachments/44444444-4444-4444-4444-444444444444" alt="text" data-cohost-src="https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444" loading="lazy"></p>{n}"#));
    assert_eq!(render_markdown_block("<img src=https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444>", &context)?,
        format!(r#"<img src="attachments/44444444-4444-4444-4444-444444444444" data-cohost-src="https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444" loading="lazy">{n}"#));
    assert_eq!(render_markdown_block("[text](https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444)", &context)?,
        format!(r#"<p><a href="attachments/44444444-4444-4444-4444-444444444444" data-cohost-href="https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444">text</a></p>{n}"#));
    assert_eq!(render_markdown_block("<a href=https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444>text</a>", &context)?,
        format!(r#"<p><a href="attachments/44444444-4444-4444-4444-444444444444" data-cohost-href="https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444">text</a></p>{n}"#));

    Ok(())
}
