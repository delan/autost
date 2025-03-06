use std::{
    cmp::Ordering,
    collections::BTreeSet,
    env,
    fs::File,
    io::{ErrorKind, Read, Write},
    path::Path,
    sync::LazyLock,
};

use askama::Template;
use chrono::{SecondsFormat, Utc};
use command::{
    attach::Attach,
    cohost2autost::Cohost2autost,
    cohost2json::Cohost2json,
    cohost_archive::CohostArchive,
    import::{Import, Reimport},
    new::New,
    render::Render,
    server::Server,
};
use dom::{QualNameExt, Transform};
use html5ever::{Attribute, QualName};
use indexmap::{indexmap, IndexMap};
use jane_eyre::eyre::{self, bail, Context, OptionExt};
use markup5ever_rcdom::{NodeData, RcDom};
use renamore::rename_exclusive_fallback;
use serde::{Deserialize, Serialize};
use toml::{toml, Value};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{
    dom::serialize_html_fragment,
    meta::extract_metadata,
    path::{PostsPath, SitePath},
    settings::Settings,
};

pub mod command {
    pub mod attach;
    pub mod cohost2autost;
    pub mod cohost2json;
    pub mod cohost_archive;
    pub mod import;
    pub mod new;
    pub mod render;
    pub mod server;
}

pub mod akkoma;
pub mod attachments;
pub mod cohost;
pub mod css;
pub mod dom;
pub mod http;
pub mod meta;
pub mod migrations;
pub mod output;
pub mod path;
pub mod rocket_eyre;
pub mod settings;

pub static SETTINGS: LazyLock<Settings> = LazyLock::new(|| {
    #[cfg(test)]
    let result = Settings::load_example();

    #[cfg(not(test))]
    let result = Settings::load_default();

    result.context("failed to load settings").unwrap()
});

#[derive(clap::Parser, Debug)]
pub enum Command {
    Attach(Attach),
    Cohost2autost(Cohost2autost),
    Cohost2json(Cohost2json),
    CohostArchive(CohostArchive),
    Import(Import),
    New(New),
    Reimport(Reimport),
    Render(Render),
    Server(Server),
}

/// details about the run, to help with migrations and bug fixes.
#[derive(Debug, Deserialize, Serialize)]
pub struct RunDetails {
    pub version: String,
    pub args: Vec<String>,
    pub start_time: String,
    #[serde(flatten)]
    pub rest: IndexMap<String, Value>,
    pub ok: Option<bool>,
}
pub struct RunDetailsWriter {
    file: File,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Template)]
#[template(path = "post-meta.html")]
pub struct PostMeta {
    pub archived: Option<String>,
    pub references: Vec<PostsPath>,
    pub title: Option<String>,
    pub published: Option<String>,
    pub author: Option<Author>,
    pub tags: Vec<String>,
    pub is_transparent_share: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Author {
    pub href: String,
    pub name: String,
    pub display_name: String,
    pub display_handle: String,
}

pub struct ExtractedPost {
    pub dom: RcDom,
    pub meta: PostMeta,
    pub needs_attachments: BTreeSet<SitePath>,
    pub og_image: Option<String>,
    pub og_description: String,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub path: Option<PostsPath>,
    pub posts: Vec<TemplatedPost>,
    pub meta: PostMeta,
    pub needs_attachments: BTreeSet<SitePath>,
    pub og_image: Option<String>,
    pub og_description: Option<String>,
}

#[derive(Clone, Debug)]
pub struct TemplatedPost {
    pub path: Option<PostsPath>,
    pub meta: PostMeta,
    pub original_html: String,
    pub safe_html: String,
    pub needs_attachments: BTreeSet<SitePath>,
    pub og_image: Option<String>,
    pub og_description: String,
}

impl Default for RunDetails {
    fn default() -> Self {
        let version = if let Some(git_describe) = option_env!("VERGEN_GIT_DESCRIBE") {
            git_describe.to_owned()
        } else if option_env!("AUTOST_IS_NIX_BUILD").is_some_and(|e| e == "1") {
            // FIXME: nix package does not have access to git
            // <https://github.com/NixOS/nix/issues/7201>
            format!("{}-nix", env!("CARGO_PKG_VERSION"))
        } else {
            // other cases, including crates.io (hypothetically)
            format!("{}-unknown", env!("CARGO_PKG_VERSION"))
        };

        Self {
            version,
            args: env::args().skip(1).collect(),
            start_time: Utc::now().to_rfc3339_opts(SecondsFormat::Nanos, true),
            rest: indexmap! {},
            ok: None,
        }
    }
}

impl RunDetailsWriter {
    pub fn create_in(dir: impl AsRef<Path>) -> eyre::Result<Self> {
        let dir = dir.as_ref();
        for i in 0.. {
            if let Err(error) = rename_exclusive_fallback(
                dir.join("run_details.toml"),
                dir.join(format!("run_details.{i}.toml")),
            ) {
                match error.kind() {
                    ErrorKind::NotFound => break,
                    ErrorKind::AlreadyExists => continue,
                    other => bail!(
                        "failed to hard link old run_details.toml at run_details.{i}.toml: {other:?}"
                    ),
                }
            } else {
                break;
            }
        }

        // at this point, run_details.toml should not exist, unless there are concurrent shenanigans
        let mut file = File::create_new(dir.join("run_details.toml"))?;
        write!(file, "{}", toml::to_string(&RunDetails::default())?)?;

        Ok(Self { file })
    }

    pub fn write(&mut self, key: &str, value: impl Into<Value>) -> eyre::Result<()> {
        let result = toml! { x = (value.into()) }.to_string();
        let result = result
            .strip_prefix("x = ")
            .expect("guaranteed by definition");

        Ok(write!(self.file, r#"{key} = {result}"#)?)
    }

    pub fn ok(mut self) -> eyre::Result<()> {
        Ok(writeln!(self.file, "ok = true")?)
    }
}

impl PostMeta {
    #[must_use] pub fn is_main_self_author(&self, settings: &Settings) -> bool {
        self.author
            .as_ref()
            .map_or(settings.self_author.is_none(), |a| {
                settings.is_main_self_author(a)
            })
    }

    #[must_use] pub fn is_any_self_author(&self, settings: &Settings) -> bool {
        let no_self_authors =
            settings.self_author.is_none() && settings.other_self_authors.is_empty();

        self.author
            .as_ref()
            .map_or(no_self_authors, |a| settings.is_any_self_author(a))
    }
}

#[test]
fn test_is_main_self_author() -> eyre::Result<()> {
    let settings = Settings::load_example()?;

    let mut settings_no_self_author = Settings::load_example()?;
    let mut meta_no_author = PostMeta::default();
    settings_no_self_author.self_author = None;
    meta_no_author.author = None;

    // same href as [self_author], but different name, display_name, and handle
    let mut meta_same_href = PostMeta::default();
    meta_same_href.author = Some(Author {
        href: "https://example.com".to_owned(),
        name: String::new(),
        display_name: String::new(),
        display_handle: String::new(),
    });

    // different href from [self_author]
    let mut meta_different_href = PostMeta::default();
    meta_different_href.author = Some(Author {
        href: "https://example.net".to_owned(),
        name: String::new(),
        display_name: String::new(),
        display_handle: String::new(),
    });

    assert!(meta_same_href.is_main_self_author(&settings));
    assert!(!meta_different_href.is_main_self_author(&settings));
    assert!(!meta_no_author.is_main_self_author(&settings));
    assert!(!meta_same_href.is_main_self_author(&settings_no_self_author));
    assert!(!meta_different_href.is_main_self_author(&settings_no_self_author));
    assert!(meta_no_author.is_main_self_author(&settings_no_self_author));

    Ok(())
}

impl Thread {
    #[must_use] pub fn reverse_chronological(p: &Self, q: &Self) -> Ordering {
        p.meta.published.cmp(&q.meta.published).reverse()
    }

    pub fn url_for_original_path(&self) -> eyre::Result<Option<String>> {
        let result = self.path.as_ref().map(path::RelativePath::references_url);

        Ok(result)
    }

    pub fn url_for_html_permalink(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(path::RelativePath::rendered_path)
            .transpose()?
            .flatten()
            .map(|path| path.internal_url());

        Ok(result)
    }

    pub fn url_for_atom_permalink(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(path::RelativePath::rendered_path)
            .transpose()?
            .flatten()
            .map(|path| path.external_url());

        Ok(result)
    }

    pub fn atom_feed_entry_id(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(path::RelativePath::rendered_path)
            .transpose()?
            .flatten()
            .map(|path| path.atom_feed_entry_id());

        Ok(result)
    }

    pub fn needs_attachments(&self) -> impl Iterator<Item = &SitePath> {
        self.needs_attachments.iter()
    }

    pub fn posts_in_thread(&self) -> impl Iterator<Item = PostInThread> + '_ {
        let len = self.posts.len();

        self.posts
            .iter()
            .cloned()
            .enumerate()
            .map(move |(i, post)| {
                if i == len - 1 {
                    PostInThread {
                        inner: post,
                        is_main_post: true,
                    }
                } else {
                    PostInThread {
                        inner: post,
                        is_main_post: false,
                    }
                }
            })
    }

    pub fn main_post(&self) -> eyre::Result<&TemplatedPost> {
        self.posts.last().ok_or_eyre("thread has no posts")
    }
}

pub struct PostInThread {
    inner: TemplatedPost,
    is_main_post: bool,
}

impl TryFrom<TemplatedPost> for Thread {
    type Error = eyre::Report;

    fn try_from(mut post: TemplatedPost) -> eyre::Result<Self> {
        let path = post.path.clone();
        let extra_tags = SETTINGS
            .extra_archived_thread_tags(&post)
            .iter()
            .filter(|tag| !post.meta.tags.contains(tag))
            .map(std::borrow::ToOwned::to_owned)
            .collect::<Vec<_>>();
        let combined_tags = extra_tags
            .into_iter()
            .chain(post.meta.tags)
            .collect();
        let resolved_tags = SETTINGS.resolve_tags(combined_tags);
        post.meta.tags = resolved_tags;
        let mut meta = post.meta.clone();

        let mut posts = post
            .meta
            .references
            .iter()
            .map(TemplatedPost::load)
            .collect::<Result<Vec<_>, _>>()?;
        posts.push(post);

        // TODO: skip threads with other authors?
        // TODO: skip threads with private or logged-in-only authors?
        // TODO: gate sensitive posts behind an interaction?

        // for thread metadata, take the last post that is not a transparent share (which MAY have
        // tags, but SHOULD NOT have a title and MUST NOT have a body), and use its metadata if any.
        let last_non_transparent_share_post = posts
            .iter()
            .rev()
            .find(|post| !post.meta.is_transparent_share);
        meta.title = last_non_transparent_share_post.map(|post| {
            if let Some(title) = post.meta.title.clone().filter(|t| !t.is_empty()) {
                title
            } else if let Some(author) = post.meta.author.as_ref() {
                format!("untitled post by {}", author.display_handle)
            } else {
                "untitled post".to_owned()
            }
        });
        let og_image = last_non_transparent_share_post
            .and_then(|post| post.og_image.as_deref())
            .map(|og_image| SETTINGS.base_url_relativise(og_image));
        let og_description =
            last_non_transparent_share_post.map(|post| post.og_description.clone());

        let needs_attachments = posts
            .iter()
            .flat_map(|post| post.needs_attachments.iter())
            .map(std::borrow::ToOwned::to_owned)
            .collect();

        Ok(Self {
            path,
            posts,
            meta,
            needs_attachments,
            og_image,
            og_description,
        })
    }
}

impl TemplatedPost {
    pub fn load(path: &PostsPath) -> eyre::Result<Self> {
        let mut file = File::open(path)?;
        let mut unsafe_source = String::default();
        file.read_to_string(&mut unsafe_source)?;

        let unsafe_html = if path.is_markdown_post() {
            // author step: render markdown to html.
            render_markdown(&unsafe_source)
        } else {
            unsafe_source
        };

        Self::filter(&unsafe_html, Some(path.to_owned()))
    }

    pub fn filter(unsafe_html: &str, path: Option<PostsPath>) -> eyre::Result<Self> {
        // reader step: extract metadata.
        let post = extract_metadata(unsafe_html)?;

        let mut transform = Transform::new(post.dom.document.clone());
        while transform.next(|kids, new_kids| {
            for kid in kids {
                if let NodeData::Element { name, attrs, .. } = &kid.data {
                    // reader step: make all `<img>` elements lazy loaded.
                    if name == &QualName::html("img") {
                        attrs.borrow_mut().push(Attribute {
                            name: QualName::attribute("loading"),
                            value: "lazy".into(),
                        });
                    }
                }
                new_kids.push(kid.clone());
            }
            Ok(())
        })? {}

        // reader step: filter html.
        let extracted_html = serialize_html_fragment(post.dom)?;
        let safe_html = ammonia::Builder::default()
            .add_generic_attributes(["style", "id", "aria-label"])
            .add_generic_attributes(["data-cohost-href", "data-cohost-src"]) // cohost2autost
            .add_generic_attributes(["data-import-src"]) // autost import
            .add_tag_attributes("a", ["target"])
            .add_tag_attributes("audio", ["controls", "src"])
            .add_tag_attributes("details", ["open", "name"]) // <details name> for cohost compatibility
            .add_tag_attributes("img", ["loading"])
            .add_tag_attributes("video", ["controls", "src"])
            .add_tags(["audio", "meta", "video"])
            .add_tag_attributes("meta", ["name", "content"])
            .id_prefix(Some("user-content-")) // cohost compatibility
            .clean(&extracted_html)
            .to_string();

        Ok(Self {
            path,
            meta: post.meta,
            original_html: unsafe_html.to_owned(),
            safe_html,
            needs_attachments: post.needs_attachments,
            og_image: post.og_image,
            og_description: post.og_description,
        })
    }
}

pub fn cli_init() -> eyre::Result<()> {
    jane_eyre::install()?;
    tracing_subscriber::registry()
        // FIXME: rocket launch logging would print nicer if
        // it didn't have the module path etc
        .with(tracing_subscriber::fmt::layer())
        .with(if std::env::var("RUST_LOG").is_ok() {
            EnvFilter::builder().from_env_lossy()
        } else {
            "autost=info,rocket=info".parse()?
        })
        .init();

    Ok(())
}

/// render markdown in a cohost-compatible way.
///
/// known discrepancies:
/// - `~~strikethrough~~` not handled
/// - @mentions not handled
/// - :emotes: not handled
/// - single newline always yields `<br>`
///   (this was not the case for older chosts, as reflected in their `.astMap`)
/// - blank lines in `<details>` close the element in some situations?
/// - spaced numbered lists yield separate `<ol start>` instead of `<li><p>`
#[must_use] pub fn render_markdown(markdown: &str) -> String {
    let mut options = comrak::Options::default();
    options.render.unsafe_ = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.render.hardbreaks = true;
    

    comrak::markdown_to_html(markdown, &options)
}

#[test]
fn test_render_markdown() {
    assert_eq!(
        render_markdown("first\nsecond"),
        "<p>first<br />\nsecond</p>\n"
    );
}
