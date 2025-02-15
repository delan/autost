use std::{
    fs::{create_dir_all, File},
    io::{self, Write},
    rc::Rc,
};

use askama::Template;
use base64::{prelude::BASE64_STANDARD, Engine};
use html5ever::Attribute;
use jane_eyre::eyre::{self, bail, OptionExt};
use markup5ever_rcdom::{Handle, NodeData};
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, info, trace, warn};
use url::Url;

use crate::{
    akkoma::{AkkomaImgTemplate, ApiInstance, ApiStatus},
    attachments::{AttachmentsContext, RealAttachmentsContext},
    dom::{
        html_attributes_with_embedding_urls, html_attributes_with_non_embedding_urls,
        parse_html_document, parse_html_fragment, serialize_html_fragment, serialize_node_contents,
        text_content, AttrsRefExt, BreadthTraverse, QualName, QualNameExt, TendrilExt,
    },
    migrations::run_migrations,
    path::PostsPath,
    Author, PostMeta, TemplatedPost,
};

#[derive(clap::Args, Debug)]
pub struct Import {
    url: String,
}

#[derive(clap::Args, Debug)]
pub struct Reimport {
    posts_path: String,
}

pub async fn main(args: Import) -> eyre::Result<()> {
    run_migrations()?;

    let url = args.url;
    create_dir_all(&*PostsPath::IMPORTED)?;

    let FetchPostResult {
        base_href,
        content: e_content,
        url: u_url,
        meta,
    } = fetch_post(&url).await?;

    let mut result = None;
    for post_id in 1.. {
        let path = PostsPath::imported_post_path(post_id);
        match File::create_new(&path) {
            Ok(file) => {
                info!("creating new post: {path:?}");
                result = Some((path, file));
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let post = TemplatedPost::load(&path)?;
                if post.meta.archived == Some(u_url.to_string()) {
                    info!("updating existing post: {path:?}");
                    let file = File::create(&path)?;
                    result = Some((path, file));
                    break;
                }
            }
            Err(other) => Err(other)?,
        }
    }

    let (path, file) = result.ok_or_eyre("too many posts :(")?;
    write_post(file, meta, e_content, base_href, path)?;

    Ok(())
}

pub async fn reimport(args: Reimport) -> eyre::Result<()> {
    run_migrations()?;

    let path = args.posts_path;
    let path = PostsPath::from_site_root_relative_path(&path)?;
    let post = TemplatedPost::load(&path)?;
    let url = post.meta.archived.ok_or_eyre("post is not archived")?;
    let FetchPostResult {
        base_href,
        content: e_content,
        url: u_url,
        meta,
    } = fetch_post(&url).await?;
    assert_eq!(url, u_url.to_string());

    info!("updating existing post: {path:?}");
    let file = File::create(&path)?;
    write_post(file, meta, e_content, base_href, path)?;

    Ok(())
}

async fn fetch_post(url: &str) -> eyre::Result<FetchPostResult> {
    info!("GET {url}");
    let client = reqwest::Client::new();
    let response = client.get(url).send().await?;
    let dom = parse_html_document(&response.bytes().await?)?;

    if let Some(result) = fetch_h_entry_post(dom.document.clone(), url)? {
        return Ok(result);
    }
    if let Some(result) = fetch_akkoma_post(dom.document.clone(), url, &client).await? {
        return Ok(result);
    }

    bail!("failed to find a supported post")
}

fn fetch_h_entry_post(document: Handle, url: &str) -> eyre::Result<Option<FetchPostResult>> {
    let Some(h_entry) = mf2_find(document.clone(), "h-entry") else {
        return Ok(None);
    };
    info!("found h-entry post");

    let mut base_href = Url::parse(&url)?;
    for node in BreadthTraverse::elements(document) {
        let NodeData::Element { name, attrs, .. } = &node.data else {
            unreachable!()
        };
        if name == &QualName::html("base") {
            if let Some(href) = attrs.borrow().attr_str("href")? {
                base_href = base_href.join(href)?;
                break;
            }
        }
    }

    let e_content =
        mf2_e(h_entry.clone(), "e-content")?.ok_or_eyre(".h-entry has no .e-content")?;
    trace!(?e_content);

    let u_url = mf2_u(h_entry.clone(), "u-url", &base_href)?;
    let dt_published = mf2_dt(h_entry.clone(), "dt-published")?;
    let p_name = mf2_p(h_entry.clone(), "p-name")?;
    let p_author = mf2_find(h_entry.clone(), "p-author").ok_or_eyre(".h-entry has no .p-author")?;
    let p_category = mf2_find_all(h_entry.clone(), "p-category");
    trace!(?u_url, ?dt_published, ?p_name, ?p_author, ?p_category);

    // the canonical url is what the h-entry says it is.
    let canonical_url = u_url.ok_or_eyre(".h-entry has no .u-url")?;
    let author = if has_class(p_author.clone(), "h-card")? {
        let card_url = mf2_u(p_author.clone(), "u-url", &base_href)?;
        let card_name = mf2_p(p_author.clone(), "p-name")?.ok_or_eyre(".h-card has no .p-name")?;
        let url = card_url.unwrap_or(canonical_url.clone());
        Author {
            href: url.to_string(),
            name: card_name.clone(),
            display_name: card_name.clone(),
            display_handle: url.authority().to_owned(),
        }
    } else {
        let p_author = mf2_p(p_author.clone(), "p-author")?
            .ok_or_eyre("failed to parse .p-author as p-property")?;
        Author {
            href: canonical_url.to_string(),
            name: p_author.clone(),
            display_name: p_author.clone(),
            display_handle: canonical_url.authority().to_owned(),
        }
    };
    trace!(?author);

    let mut tags = vec![];
    'category: for p_category in p_category {
        // skip any .p-category that may be in a nested .h-entry (nex-3.com extension).
        // <https://nex-3.com/blog/reblogging-posts-with-h-entry/>
        let mut node = p_category.clone();
        // access the parent, per <markup5ever_rcdom-0.3.0/lib.rs:170>.
        while let Some(weak) = node.parent.take() {
            let parent = weak.upgrade().expect("dangling weak pointer");
            node.parent.set(Some(weak));
            if has_class(parent.clone(), "h-entry")? {
                if !Rc::ptr_eq(&parent, &h_entry) {
                    continue 'category;
                }
            }
            node = parent;
        }

        let p_category = mf2_p(p_category.clone(), "p-category")?
            .ok_or_eyre("failed to parse .p-category as p-property")?;
        tags.push(p_category);
    }

    let meta = PostMeta {
        archived: Some(canonical_url.to_string()),
        references: vec![], // TODO: define a cohost-like h-entry extension for this?
        title: p_name,
        published: dt_published,
        author: Some(author),
        tags,
        is_transparent_share: false,
    };
    debug!(?meta);

    Ok(Some(FetchPostResult {
        base_href,
        content: e_content,
        url: canonical_url,
        meta,
    }))
}

async fn fetch_akkoma_post(
    document: Handle,
    url: &str,
    client: &Client,
) -> eyre::Result<Option<FetchPostResult>> {
    // check if the page is actually an akkoma page.
    #[derive(Deserialize)]
    struct InitialResults {
        #[serde(rename = "/api/v1/instance")]
        api_v1_instance: String,
    }
    let Some(initial_results) = (|| -> eyre::Result<Option<InitialResults>> {
        for node in BreadthTraverse::elements(document) {
            let NodeData::Element { name, attrs, .. } = &node.data else {
                unreachable!()
            };
            if name == &QualName::html("script") {
                if attrs.borrow().attr_str("id")? == Some("initial-results") {
                    return Ok(Some(serde_json::from_str(&text_content(node)?)?));
                }
            }
        }
        Ok(None)
    })()?
    else {
        return Ok(None);
    };
    let instance = BASE64_STANDARD.decode(initial_results.api_v1_instance)?;
    let instance = serde_json::from_slice::<ApiInstance>(&instance)?;
    info!(?instance.uri, ?instance.version, "found akkoma instance");

    // try to fetch the post via the mastodon api.
    let instance_url = Url::parse(&instance.uri)?;
    trace!(?instance_url);
    let fetched_page_url = Url::parse(url)?;
    trace!(?fetched_page_url);
    let status_id = fetched_page_url
        .path_segments()
        .ok_or_eyre("bad page url")?
        .last()
        .ok_or_eyre("page url has no last path segment")?;
    trace!(?status_id);
    let api_url = instance_url.join(&format!("api/v1/statuses/{status_id}"))?;
    info!("GET {api_url}");
    let response = client.get(api_url).send().await?;
    let status = response.json::<ApiStatus>().await?;

    // the canonical url is what the api says it is.
    let canonical_url = status.url;
    let author = Author::from(&status.account);

    let mut contents = vec![];
    for attachment in status.media_attachments {
        if attachment.r#type != "image" {
            warn!(?attachment.r#type, "skipping unknown attachment type");
            continue;
        }
        let template = AkkomaImgTemplate {
            data_akkoma_src: attachment.preview_url.clone(),
            href: attachment.url,
            src: attachment.preview_url,
            alt: attachment.description,
        };
        contents.push(template.render()?);
    }
    contents.push(status.content);
    let content = contents.join("");

    let url = Url::parse(&canonical_url)?;
    let meta = PostMeta {
        archived: Some(canonical_url),
        references: vec![], // TODO: handle akkoma reply chain?
        title: None,
        published: Some(status.created_at),
        author: Some(author),
        tags: status.tags.into_iter().map(|tag| tag.name).collect(),
        is_transparent_share: false,
    };

    Ok(Some(FetchPostResult {
        base_href: url.clone(),
        content: content,
        url,
        meta,
    }))
}

fn write_post(
    mut file: File,
    meta: PostMeta,
    e_content: String,
    base_href: Url,
    path: PostsPath,
) -> eyre::Result<()> {
    info!("writing {path:?}");
    file.write_all(meta.render()?.as_bytes())?;
    file.write_all(b"\n\n")?;
    let basename = path.basename().ok_or_eyre("path has no basename")?;
    let unsafe_html = process_content(&e_content, basename, &base_href, &RealAttachmentsContext)?;
    let post = TemplatedPost::filter(&unsafe_html, Some(path.clone()))?;
    file.write_all(post.safe_html.as_bytes())?;
    info!("click here to reply: {}", path.compose_reply_url());
    info!(
        "or transparent share: {}",
        path.compose_transparent_share_url()
    );

    Ok(())
}

struct FetchPostResult {
    base_href: Url,
    content: String,
    url: Url,
    meta: PostMeta,
}

fn process_content(
    content: &str,
    post_basename: &str,
    base_href: &Url,
    context: &dyn AttachmentsContext,
) -> eyre::Result<String> {
    let dom = parse_html_fragment(content.as_bytes())?;

    for node in BreadthTraverse::nodes(dom.document.clone()) {
        match &node.data {
            NodeData::Element { name, attrs, .. } => {
                let mut attrs = attrs.borrow_mut();
                let mut extra_attrs = vec![];
                if let Some(attr_names) = html_attributes_with_embedding_urls().get(name) {
                    for attr in attrs.iter_mut() {
                        if attr_names.contains(&attr.name) {
                            // rewrite attachment urls to relative cached paths.
                            let old_url = attr.value.to_str().to_owned();
                            let fetch_url = base_href.join(&old_url)?;
                            trace!(
                                "found attachment url in <{} {}>: {old_url}",
                                name.local,
                                attr.name.local
                            );
                            attr.value = context
                                .cache_imported(&fetch_url.to_string(), post_basename)?
                                .site_path()?
                                .base_relative_url()
                                .into();
                            extra_attrs.push(Attribute {
                                name: QualName::attribute(&format!(
                                    "data-import-{}",
                                    attr.name.local
                                )),
                                value: old_url.into(),
                            });
                        }
                    }
                }
                if let Some(attr_names) = html_attributes_with_non_embedding_urls().get(name) {
                    for attr in attrs.iter_mut() {
                        if attr_names.contains(&attr.name) {
                            // rewrite urls in links to bake in the `base_href`.
                            let old_url = attr.value.to_str().to_owned();
                            let new_url = if old_url.starts_with("#") {
                                format!("#user-content-{}", &old_url[1..])
                            } else {
                                base_href.join(&old_url)?.to_string()
                            };
                            trace!(
                                "rewriting <{} {}>: {old_url:?} -> {new_url:?}",
                                name.local,
                                attr.name.local,
                            );
                            attr.value = new_url.to_string().into();
                            extra_attrs.push(Attribute {
                                name: QualName::attribute(&format!(
                                    "data-import-{}",
                                    attr.name.local
                                )),
                                value: old_url.into(),
                            });
                        }
                    }
                }
                if name == &QualName::html("img") {
                    extra_attrs.push(Attribute {
                        name: QualName::attribute("loading"),
                        value: "lazy".into(),
                    });
                }
                attrs.extend(extra_attrs);
            }
            _ => {}
        }
    }

    Ok(serialize_html_fragment(dom)?)
}

fn mf2_e(node: Handle, class: &str) -> eyre::Result<Option<String>> {
    // TODO: handle full return value in <https://microformats.org/wiki/microformats2-parsing#parsing_an_e-_property>
    let Some(node) = mf2_find(node, class) else {
        return Ok(None);
    };
    let html = serialize_node_contents(node)?;

    Ok(Some(html))
}

/// <https://microformats.org/wiki/index.php?title=microformats2-parsing&oldid=70607#parsing_a_p-_property>
fn mf2_p(node: Handle, class: &str) -> eyre::Result<Option<String>> {
    // TODO: handle other cases in <https://microformats.org/wiki/microformats2-parsing#parsing_a_p-_property>
    let Some(node) = mf2_find(node, class) else {
        return Ok(None);
    };
    if let NodeData::Element { name, attrs, .. } = &node.data {
        let attrs = attrs.borrow();
        // “If `abbr.p-x[title]` or `link.p-x[title]`, then return the `title` attribute.”
        if name == &QualName::html("abbr") || name == &QualName::html("link") {
            if let Some(title) = attrs.attr_str("title")? {
                return Ok(Some(title.to_owned()));
            }
        }
        // “else if `data.p-x[value]` or `input.p-x[value]`, then return the `value` attribute”
        if name == &QualName::html("data") || name == &QualName::html("input") {
            if let Some(value) = attrs.attr_str("value")? {
                return Ok(Some(value.to_owned()));
            }
        }
        // “else if `img.p-x[alt]` or `area.p-x[alt]`, then return the `alt` attribute”
        if name == &QualName::html("img") || name == &QualName::html("area") {
            if let Some(alt) = attrs.attr_str("alt")? {
                return Ok(Some(alt.to_owned()));
            }
        }
    }
    // “else return the textContent of the element after:”
    // - TODO: “dropping any nested <script> & <style> elements;”
    // - TODO: “replacing any nested <img> elements with their alt attribute, if present; otherwise their src attribute, if present, adding a space at the beginning and end, resolving the URL if it’s relative;”
    // - “removing all leading/trailing spaces”
    let result = text_content(node)?.trim_ascii().to_owned();

    Ok(Some(result))
}

fn mf2_u(node: Handle, class: &str, base_href: &Url) -> eyre::Result<Option<Url>> {
    // TODO: handle other cases in <https://microformats.org/wiki/microformats2-parsing#parsing_a_u-_property>
    let Some(element) = mf2_find(node.clone(), class) else {
        return Ok(None);
    };
    let attrs = if let NodeData::Element { attrs, .. } = &element.data {
        attrs.borrow()
    } else {
        unreachable!("guaranteed by mf2_find")
    };

    if let Some(result) = attrs.attr_str("href")? {
        Ok(Some(base_href.join(result)?))
    } else if let Some(result) = attrs.attr_str("value")? {
        Ok(Some(base_href.join(result)?))
    } else {
        bail!(".u-class has no value");
    }
}

fn mf2_dt(node: Handle, class: &str) -> eyre::Result<Option<String>> {
    // TODO: handle other cases in <https://microformats.org/wiki/microformats2-parsing#parsing_a_dt-_property>
    let Some(element) = mf2_find(node.clone(), class) else {
        return Ok(None);
    };
    let NodeData::Element { attrs, .. } = &element.data else {
        unreachable!("guaranteed by mf2_find")
    };
    let result = attrs
        .borrow()
        .attr_str("datetime")?
        .map(|datetime| datetime.to_owned())
        .ok_or_eyre(".dt-class has no [datetime]")?;

    Ok(Some(result))
}

fn mf2_find(node: Handle, class: &str) -> Option<Handle> {
    // TODO: handle errors from has_class()
    BreadthTraverse::elements(node.clone())
        .find(|node| has_class(node.clone(), class).unwrap_or(false))
}

fn mf2_find_all(node: Handle, class: &str) -> Vec<Handle> {
    // TODO: handle errors from has_class()
    BreadthTraverse::elements(node.clone())
        .filter(|node| has_class(node.clone(), class).unwrap_or(false))
        .collect()
}

fn has_class(node: Handle, class: &str) -> eyre::Result<bool> {
    if let NodeData::Element { attrs, .. } = &node.data {
        if let Some(node_class) = attrs.borrow().attr_str("class")? {
            if node_class.split(" ").find(|&c| c == class).is_some() {
                return Ok(true);
            }
        }
    }

    Ok(false)
}
