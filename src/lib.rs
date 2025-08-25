use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::Display,
    fs::File,
    io::{ErrorKind, Read, Write},
    path::Path,
    sync::LazyLock,
};

use askama::Template;
use bincode::{Decode, Encode};
use chrono::{SecondsFormat, Utc};
use command::{
    attach::Attach,
    cache::Cache,
    cohost2autost::Cohost2autost,
    cohost2json::Cohost2json,
    cohost_archive::CohostArchive,
    db::Db,
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
    cache::Id,
    dom::serialize_html_fragment,
    path::{PostsPath, SitePath},
    settings::Settings,
};

pub mod command {
    pub mod attach;
    pub mod cache;
    pub mod cohost2autost;
    pub mod cohost2json;
    pub mod cohost_archive;
    pub mod db;
    pub mod import;
    pub mod new;
    pub mod render;
    pub mod server;
}

pub mod akkoma;
pub mod attachments;
pub mod cache;
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
    #[command(subcommand)]
    Db(Db),
    #[command(subcommand)]
    Cache(Cache),
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

/// post metadata in the front matter only.
#[derive(Clone, Debug, Default, Template, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
#[template(path = "front-matter.html")]
pub struct FrontMatter {
    pub archived: Option<String>,
    pub references: Vec<PostsPath>,
    pub title: Option<String>,
    pub published: Option<String>,
    pub author: Option<Author>,
    pub tags: Vec<String>,
    pub is_transparent_share: bool,
}

/// all post metadata, including computed metadata.
#[derive(Clone, Debug, Default, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct PostMeta {
    pub front_matter: FrontMatter,
    pub needs_attachments: BTreeSet<SitePath>,
    pub og_image: Option<String>,
    pub og_description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct Author {
    pub href: String,
    pub name: String,
    pub display_name: String,
    pub display_handle: String,
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnsafePost {
    pub path: Option<PostsPath>,
    pub unsafe_html: String,
}

pub struct UnsafeExtractedPost {
    pub post: UnsafePost,
    pub dom: RcDom,
    pub meta: PostMeta,
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct FilteredPost {
    pub post: UnsafePost,
    pub meta: PostMeta,
    pub safe_html: String,
}

#[derive(Clone, Debug, Decode, Encode, PartialEq, Eq, PartialOrd, Ord)]
pub struct Thread {
    pub path: Option<PostsPath>,
    pub posts: Vec<FilteredPost>,
    pub meta: PostMeta,
}

#[derive(Clone, Debug, Decode, Encode)]
pub struct TagIndex {
    tags: BTreeMap<String, BTreeSet<Id>>,
}
impl TagIndex {
    pub fn new(threads: BTreeMap<Id, Thread>) -> Self {
        let mut tags: BTreeMap<String, BTreeSet<Id>> = BTreeMap::default();
        for (id, thread) in threads.into_iter() {
            for tag in thread.meta.front_matter.tags.iter() {
                tags.entry(tag.clone()).or_default().insert(id);
            }
        }
        Self { tags }
    }
}
impl Display for TagIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TagIndex {{")?;
        for (tag, threads) in self.tags.iter() {
            // ds.field(tag, &threads.len());
            write!(f, "\n- {tag:?} ({} threads)", threads.len())?;
        }
        write!(f, "\n}}")
    }
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

impl FrontMatter {
    pub fn is_main_self_author(&self, settings: &Settings) -> bool {
        self.author
            .as_ref()
            .map_or(settings.self_author.is_none(), |a| {
                settings.is_main_self_author(a)
            })
    }

    pub fn is_any_self_author(&self, settings: &Settings) -> bool {
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
    let mut meta_no_author = FrontMatter::default();
    settings_no_self_author.self_author = None;
    meta_no_author.author = None;

    // same href as [self_author], but different name, display_name, and handle
    let meta_same_href = FrontMatter {
        author: Some(Author {
            href: "https://example.com".to_owned(),
            name: "".to_owned(),
            display_name: "".to_owned(),
            display_handle: "".to_owned(),
        }),
        ..Default::default()
    };

    // different href from [self_author]
    let meta_different_href = FrontMatter {
        author: Some(Author {
            href: "https://example.net".to_owned(),
            name: "".to_owned(),
            display_name: "".to_owned(),
            display_handle: "".to_owned(),
        }),
        ..Default::default()
    };

    assert!(meta_same_href.is_main_self_author(&settings));
    assert!(!meta_different_href.is_main_self_author(&settings));
    assert!(!meta_no_author.is_main_self_author(&settings));
    assert!(!meta_same_href.is_main_self_author(&settings_no_self_author));
    assert!(!meta_different_href.is_main_self_author(&settings_no_self_author));
    assert!(meta_no_author.is_main_self_author(&settings_no_self_author));

    Ok(())
}

impl Thread {
    pub fn new(mut post: FilteredPost, references: Vec<FilteredPost>) -> Self {
        let path = post.post.path.clone();
        let extra_tags = SETTINGS
            .extra_archived_thread_tags(&post)
            .iter()
            .filter(|tag| !post.meta.front_matter.tags.contains(tag))
            .map(|tag| tag.to_owned())
            .collect::<Vec<_>>();
        let combined_tags = extra_tags
            .into_iter()
            .chain(post.meta.front_matter.tags)
            .collect();
        let resolved_tags = SETTINGS.resolve_tags(combined_tags);
        post.meta.front_matter.tags = resolved_tags;
        let mut meta = post.meta.clone();

        let mut posts = references;
        posts.push(post);

        // TODO: skip threads with other authors?
        // TODO: skip threads with private or logged-in-only authors?
        // TODO: gate sensitive posts behind an interaction?

        // for thread metadata, take the last post that is not a transparent share (which MAY have
        // tags, but SHOULD NOT have a title and MUST NOT have a body), and use its metadata if any.
        let last_non_transparent_share_post = posts
            .iter()
            .rev()
            .find(|post| !post.meta.front_matter.is_transparent_share);
        meta.front_matter.title = last_non_transparent_share_post.map(|post| {
            if let Some(title) = post
                .meta
                .front_matter
                .title
                .clone()
                .filter(|t| !t.is_empty())
            {
                title
            } else if let Some(author) = post.meta.front_matter.author.as_ref() {
                format!("untitled post by {}", author.display_handle)
            } else {
                "untitled post".to_owned()
            }
        });

        let og_image = last_non_transparent_share_post
            .and_then(|post| post.meta.og_image.as_deref())
            .map(|og_image| SETTINGS.base_url_relativise(og_image));
        let og_description =
            last_non_transparent_share_post.and_then(|post| post.meta.og_description.to_owned());
        let needs_attachments = posts
            .iter()
            .flat_map(|post| post.meta.needs_attachments.iter())
            .map(|attachment_path| attachment_path.to_owned())
            .collect();
        let meta = PostMeta {
            front_matter: meta.front_matter,
            needs_attachments,
            og_image,
            og_description,
        };

        Self { path, posts, meta }
    }

    pub fn reverse_chronological(p: &Thread, q: &Thread) -> Ordering {
        p.meta
            .front_matter
            .published
            .cmp(&q.meta.front_matter.published)
            .reverse()
    }

    pub fn url_for_original_path(&self) -> eyre::Result<Option<String>> {
        let result = self.path.as_ref().map(|path| path.references_url());

        Ok(result)
    }

    pub fn url_for_fragment(&self) -> eyre::Result<Option<String>> {
        let result = self.path.as_ref().map(|path| path.references_url());

        Ok(result)
    }

    pub fn url_for_html_permalink(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(|path| path.rendered_path())
            .transpose()?
            .flatten()
            .map(|path| path.internal_url());

        Ok(result)
    }

    pub fn url_for_atom_permalink(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(|path| path.rendered_path())
            .transpose()?
            .flatten()
            .map(|path| path.external_url());

        Ok(result)
    }

    pub fn atom_feed_entry_id(&self) -> eyre::Result<Option<String>> {
        let result = self
            .path
            .as_ref()
            .map(|path| path.rendered_path())
            .transpose()?
            .flatten()
            .map(|path| path.atom_feed_entry_id());

        Ok(result)
    }

    pub fn needs_attachments(&self) -> impl Iterator<Item = &SitePath> {
        self.meta.needs_attachments.iter()
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

    pub fn main_post(&self) -> eyre::Result<&FilteredPost> {
        self.posts.last().ok_or_eyre("thread has no posts")
    }
}

pub struct PostInThread {
    inner: FilteredPost,
    is_main_post: bool,
}

impl TryFrom<FilteredPost> for Thread {
    type Error = eyre::Report;

    fn try_from(post: FilteredPost) -> eyre::Result<Self> {
        let references = post
            .meta
            .front_matter
            .references
            .iter()
            .map(FilteredPost::load)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self::new(post, references))
    }
}

impl UnsafePost {
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

        Ok(Self {
            path: Some(path.to_owned()),
            unsafe_html,
        })
    }

    pub fn with_markdown(unsafe_source: &str) -> Self {
        let unsafe_html = render_markdown(unsafe_source);

        Self {
            path: None,
            unsafe_html,
        }
    }

    pub fn with_html(unsafe_html: &str) -> Self {
        // the source is already html; there is no markdown.
        let unsafe_source = unsafe_html;

        Self {
            path: None,
            unsafe_html: unsafe_source.to_owned(),
        }
    }
}

impl FilteredPost {
    pub fn load(path: &PostsPath) -> eyre::Result<Self> {
        let post = UnsafePost::load(path)?;

        Self::filter(post)
    }

    pub fn filter(post: UnsafePost) -> eyre::Result<Self> {
        // reader step: extract metadata.
        let post = UnsafeExtractedPost::new(post)?;

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
            .add_tag_attributes("audio", ["controls", "src", "loop"])
            .add_tag_attributes("details", ["open", "name"]) // <details name> for cohost compatibility
            .add_tag_attributes("img", ["loading"])
            .add_tag_attributes("video", ["controls", "src", "loop"])
            .add_tags(["audio", "meta", "video"])
            .add_tag_attributes("meta", ["name", "content"])
            .id_prefix(Some("user-content-")) // cohost compatibility
            .clean(&extracted_html)
            .to_string();

        Ok(FilteredPost {
            post: post.post,
            meta: post.meta,
            safe_html,
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
pub fn render_markdown(markdown: &str) -> String {
    let mut options = comrak::Options::default();
    options.render.unsafe_ = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.render.hardbreaks = true;
    #[allow(clippy::let_and_return)]
    let unsafe_html = comrak::markdown_to_html(markdown, &options);

    unsafe_html
}

#[test]
fn test_render_markdown() {
    assert_eq!(
        render_markdown("first\nsecond"),
        "<p>first<br />\nsecond</p>\n"
    );
}
