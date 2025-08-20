use std::{
    fmt::Display,
    fs::{hard_link, read_dir, DirEntry},
    io::ErrorKind,
    marker::PhantomData,
    path::{Component, Path, PathBuf},
    sync::LazyLock,
};

use jane_eyre::eyre::{self, bail, Context, OptionExt};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use serde::{de::Visitor, Deserialize, Serialize};
use url::Url;

use crate::SETTINGS;

pub type PostsPath = RelativePath<PostsKind>;
pub type SitePath = RelativePath<SiteKind>;
pub type AttachmentsPath = RelativePath<AttachmentsKind>;
pub type CachePath = RelativePath<CacheKind>;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[allow(private_bounds)]
pub struct RelativePath<Kind> {
    inner: PathBuf,
    kind: Kind,
}

trait PathKind: Sized + Clone {
    const ROOT: &'static str;
    fn new(path: &Path) -> eyre::Result<Self>;
    fn dynamic_path_variant() -> fn(RelativePath<Self>) -> DynamicPath;
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PostsKind {
    Post {
        is_markdown: bool,
        in_top_level: bool,
        in_imported_dir: bool,
    },
    Other,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SiteKind {
    Attachments,
    Other,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AttachmentsKind {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CacheKind {}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum DynamicPath {
    Posts(PostsPath),
    Site(SitePath),
    Attachments(AttachmentsPath),
    Cache(CachePath),
}

impl PathKind for PostsKind {
    const ROOT: &'static str = "posts";

    fn new(path: &Path) -> eyre::Result<Self> {
        let components = path
            .components()
            .skip(1)
            .map(|c| {
                c.as_os_str()
                    .to_str()
                    .ok_or_eyre("unsupported path component")
            })
            .collect::<eyre::Result<Vec<_>>>()?;

        Ok(match components[..] {
            [c] if c.ends_with(".html") => Self::Post {
                is_markdown: false,
                in_top_level: true,
                in_imported_dir: false,
            },
            [c] if c.ends_with(".md") => Self::Post {
                is_markdown: true,
                in_top_level: true,
                in_imported_dir: false,
            },
            ["imported", c] if c.ends_with(".html") => Self::Post {
                is_markdown: false,
                in_top_level: false,
                in_imported_dir: true,
            },
            _ => Self::Other,
        })
    }

    fn dynamic_path_variant() -> fn(RelativePath<Self>) -> DynamicPath {
        DynamicPath::Posts
    }
}

#[test]
fn test_posts_kind() -> eyre::Result<()> {
    assert_eq!(PostsKind::new(Path::new("posts"))?, PostsKind::Other);
    assert_eq!(PostsKind::new(Path::new("posts/foo"))?, PostsKind::Other);
    assert_eq!(
        PostsKind::new(Path::new("posts/1.html"))?,
        PostsKind::Post {
            is_markdown: false,
            in_top_level: true,
            in_imported_dir: false
        }
    );
    assert_eq!(
        PostsKind::new(Path::new("posts/1.md"))?,
        PostsKind::Post {
            is_markdown: true,
            in_top_level: true,
            in_imported_dir: false
        }
    );
    assert_eq!(
        PostsKind::new(Path::new("posts/imported"))?,
        PostsKind::Other
    );
    assert_eq!(
        PostsKind::new(Path::new("posts/imported/foo"))?,
        PostsKind::Other
    );
    assert_eq!(
        PostsKind::new(Path::new("posts/imported/1.html"))?,
        PostsKind::Post {
            is_markdown: false,
            in_top_level: false,
            in_imported_dir: true
        }
    );

    Ok(())
}

impl PathKind for SiteKind {
    const ROOT: &'static str = "site";

    fn new(path: &Path) -> eyre::Result<Self> {
        let mut components = path.components().skip(1);
        if components.next().and_then(|c| c.as_os_str().to_str()) == Some("attachments") {
            return Ok(Self::Attachments);
        }

        Ok(Self::Other)
    }

    fn dynamic_path_variant() -> fn(RelativePath<Self>) -> DynamicPath {
        DynamicPath::Site
    }
}

impl PathKind for AttachmentsKind {
    const ROOT: &'static str = "attachments";

    fn new(_path: &Path) -> eyre::Result<Self> {
        Ok(Self {})
    }

    fn dynamic_path_variant() -> fn(RelativePath<Self>) -> DynamicPath {
        DynamicPath::Attachments
    }
}

impl PathKind for CacheKind {
    const ROOT: &'static str = "cache";

    fn new(_path: &Path) -> eyre::Result<Self> {
        Ok(Self {})
    }

    fn dynamic_path_variant() -> fn(RelativePath<Self>) -> DynamicPath {
        DynamicPath::Cache
    }
}

impl<Kind: PathKind> AsRef<Path> for RelativePath<Kind> {
    fn as_ref(&self) -> &Path {
        self.inner.as_ref()
    }
}

pub static POSTS_PATH_ROOT: LazyLock<PostsPath> =
    LazyLock::new(|| PostsPath::new(PostsKind::ROOT.into()).expect("guaranteed by argument"));
pub static POSTS_PATH_IMPORTED: LazyLock<PostsPath> = LazyLock::new(|| {
    POSTS_PATH_ROOT
        .join("imported")
        .expect("guaranteed by argument")
});
impl PostsPath {
    /// creates a path from `<link rel=references href>`, which is relative to
    /// the posts directory, but percent-encoded as a url.
    pub fn from_references_url(references: &str) -> eyre::Result<Self> {
        let references = urlencoding::decode(references)?;
        let path = Path::new(PostsKind::ROOT).join(&*references);

        Self::new(path)
    }

    pub fn markdown_post_path(post_id: usize) -> Self {
        POSTS_PATH_ROOT
            .join(&format!("{post_id}.md"))
            .expect("guaranteed by argument")
    }

    pub fn generated_post_path(post_id: usize) -> Self {
        POSTS_PATH_ROOT
            .join(&format!("{post_id}.html"))
            .expect("guaranteed by argument")
    }

    pub fn references_dir(post_id: usize) -> Self {
        POSTS_PATH_ROOT
            .join(&format!("{post_id}"))
            .expect("guaranteed by argument")
    }

    pub fn references_post_path(post_id: usize, references_post_id: usize) -> Self {
        POSTS_PATH_ROOT
            .join(&format!("{post_id}/{references_post_id}.html"))
            .expect("guaranteed by argument")
    }

    pub fn imported_post_path(post_id: usize) -> Self {
        POSTS_PATH_IMPORTED
            .join(&format!("{post_id}.html"))
            .expect("guaranteed by argument")
    }

    pub fn db_post_table_path(&self) -> String {
        self.relative_path()
    }

    pub fn references_url(&self) -> String {
        self.relative_url()
    }

    pub fn compose_reply_url(&self) -> String {
        // references_url is already urlencoded
        format!(
            "http://[::1]:{}{}compose?reply_to={}",
            SETTINGS.server_port(),
            SETTINGS.base_url,
            self.references_url()
        )
    }

    pub fn compose_transparent_share_url(&self) -> String {
        // references_url is already urlencoded
        format!(
            "http://[::1]:{}{}compose?reply_to={}&is_transparent_share",
            SETTINGS.server_port(),
            SETTINGS.base_url,
            self.references_url()
        )
    }

    pub fn rendered_path(&self) -> eyre::Result<Option<SitePath>> {
        match self.kind {
            PostsKind::Post { .. } => {
                let (basename, _) = self
                    .filename()
                    .rsplit_once(".")
                    .expect("guaranteed by PostsKind::new");
                let filename = format!("{basename}.html");
                Ok(Some(SITE_PATH_ROOT.join(&filename)?))
            }
            PostsKind::Other => Ok(None),
        }
    }

    pub fn is_markdown_post(&self) -> bool {
        matches!(
            self.kind,
            PostsKind::Post {
                is_markdown: true,
                ..
            }
        )
    }

    pub fn is_top_level_post(&self) -> bool {
        matches!(
            self.kind,
            PostsKind::Post {
                in_top_level: true,
                ..
            }
        )
    }

    pub fn top_level_numeric_post_id(&self) -> Option<usize> {
        if !self.is_top_level_post() {
            return None;
        }
        let (basename, _) = self.filename().rsplit_once(".")?;

        basename.parse().ok()
    }

    pub fn import_id(&self) -> Option<usize> {
        if let PostsKind::Post {
            in_imported_dir: true,
            ..
        } = self.kind
        {
            let (basename, _) = self.filename().rsplit_once(".")?;
            return basename.parse().ok();
        }

        None
    }
}

pub static SITE_PATH_ROOT: LazyLock<SitePath> =
    LazyLock::new(|| SitePath::new(SiteKind::ROOT.into()).expect("guaranteed by argument"));
pub static SITE_PATH_TAGGED: LazyLock<SitePath> = LazyLock::new(|| {
    SITE_PATH_ROOT
        .join("tagged")
        .expect("guaranteed by argument")
});
pub static SITE_PATH_ATTACHMENTS: LazyLock<SitePath> = LazyLock::new(|| {
    SITE_PATH_ROOT
        .join("attachments")
        .expect("guaranteed by argument")
});
pub static SITE_PATH_THUMBS: LazyLock<SitePath> = LazyLock::new(|| {
    SITE_PATH_ATTACHMENTS
        .join("thumbs")
        .expect("guaranteed by argument")
});
impl SitePath {
    /// creates a path from an attachment url in a rendered post, which is relative to
    /// the posts directory, but percent-encoded as a url.
    pub fn from_rendered_attachment_url(url: &str) -> eyre::Result<Self> {
        let url = urlencoding::decode(url)?;
        let path = Path::new(SiteKind::ROOT).join(&*url);
        if !path.starts_with(&*SITE_PATH_ATTACHMENTS) {
            bail!("url is not an attachment path: {url}");
        }

        Self::new(path)
    }

    pub fn db_post_table_path(&self) -> String {
        self.relative_path()
    }

    /// use this only in post authoring contexts, like the output of importers.
    pub fn base_relative_url(&self) -> String {
        self.relative_url()
    }

    pub fn internal_url(&self) -> String {
        format!("{}{}", SETTINGS.base_url, self.relative_url())
    }

    pub fn external_url(&self) -> String {
        format!("{}{}", SETTINGS.external_base_url, self.relative_url())
    }

    pub fn atom_feed_entry_id(&self) -> String {
        // TODO: this violates the atom spec (#6)
        self.relative_url()
    }

    pub fn rsync_deploy_line(&self) -> String {
        self.relative_path()
    }

    pub fn attachments_path(&self) -> eyre::Result<Option<AttachmentsPath>> {
        match self.kind {
            SiteKind::Attachments => {
                let components = self.components().collect::<Vec<_>>();
                let path = components.join(std::path::MAIN_SEPARATOR_STR);
                Ok(Some(AttachmentsPath::new(path.into())?))
            }
            SiteKind::Other => Ok(None),
        }
    }
}

pub static ATTACHMENTS_PATH_ROOT: LazyLock<AttachmentsPath> = LazyLock::new(|| {
    AttachmentsPath::new(AttachmentsKind::ROOT.into()).expect("guaranteed by argument")
});
pub static ATTACHMENTS_PATH_THUMBS: LazyLock<AttachmentsPath> = LazyLock::new(|| {
    ATTACHMENTS_PATH_ROOT
        .join("thumbs")
        .expect("guaranteed by argument")
});
#[deprecated(since = "1.2.0", note = "cohost emoji are now stored in COHOST_STATIC")]
pub static ATTACHMENTS_PATH_EMOJI: LazyLock<AttachmentsPath> = LazyLock::new(|| {
    ATTACHMENTS_PATH_ROOT
        .join("emoji")
        .expect("guaranteed by argument")
});
pub static ATTACHMENTS_PATH_COHOST_STATIC: LazyLock<AttachmentsPath> = LazyLock::new(|| {
    ATTACHMENTS_PATH_ROOT
        .join("cohost-static")
        .expect("guaranteed by argument")
});
pub static ATTACHMENTS_PATH_COHOST_AVATAR: LazyLock<AttachmentsPath> = LazyLock::new(|| {
    ATTACHMENTS_PATH_ROOT
        .join("cohost-avatar")
        .expect("guaranteed by argument")
});
pub static ATTACHMENTS_PATH_COHOST_HEADER: LazyLock<AttachmentsPath> = LazyLock::new(|| {
    ATTACHMENTS_PATH_ROOT
        .join("cohost-header")
        .expect("guaranteed by argument")
});
impl AttachmentsPath {
    pub fn site_path(&self) -> eyre::Result<SitePath> {
        let mut result = SITE_PATH_ATTACHMENTS.to_owned();
        for component in self.components() {
            result = result.join(component)?;
        }

        Ok(result)
    }
}

pub static CACHE_PATH_ROOT: LazyLock<CachePath> =
    LazyLock::new(|| CachePath::new(CacheKind::ROOT.into()).expect("guaranteed by argument"));
impl CachePath {}

#[allow(private_bounds)]
impl<Kind: PathKind> RelativePath<Kind> {
    #[tracing::instrument]
    fn new(inner: PathBuf) -> eyre::Result<Self> {
        if inner.is_absolute() {
            bail!("path must not be absolute: {inner:?}");
        }
        if !inner.starts_with(Kind::ROOT) {
            bail!("path does not start with base: {inner:?}");
        }
        for component in inner.components() {
            match component {
                Component::Normal(component) => {
                    if component.to_str().is_none() {
                        bail!("component not unicode: {component:?}");
                    }
                }
                // disallow other components, including `..` components.
                other => bail!("component not allowed: {other:?}"),
            }
        }
        let kind = Kind::new(&inner)?;

        Ok(Self { inner, kind })
    }

    pub fn from_site_root_relative_path(path: &str) -> eyre::Result<Self> {
        Self::new(path.into())
    }

    pub fn into_dynamic_path(self) -> DynamicPath {
        Kind::dynamic_path_variant()(self)
    }

    pub fn to_dynamic_path(&self) -> DynamicPath {
        self.clone().into_dynamic_path()
    }

    pub fn join(&self, component: &str) -> eyre::Result<Self> {
        Self::new(self.inner.join(component))
    }

    pub fn join_dir_entry(&self, entry: &DirEntry) -> eyre::Result<Self> {
        let filename = entry.file_name();
        let Some(filename) = filename.to_str() else {
            bail!("filename is not unicode: {filename:?}");
        };

        self.join(filename)
    }

    fn read_dir_into_vecs(&self) -> eyre::Result<(Vec<Self>, Vec<Self>)> {
        let mut dirs = vec![];
        let mut files = vec![];
        for entry in read_dir(self)? {
            let entry = entry?;
            let path = self.join_dir_entry(&entry)?;
            if entry.file_type()?.is_dir() {
                dirs.push(path);
            } else {
                files.push(path);
            }
        }

        Ok((dirs, files))
    }

    pub fn read_dir_flat(&self) -> eyre::Result<Vec<Self>> {
        let mut files = vec![];
        for entry in read_dir(self)? {
            let entry = entry?;
            let path = self.join_dir_entry(&entry)?;
            if !entry.file_type()?.is_dir() {
                files.push(path);
            }
        }

        Ok(files)
    }

    pub fn read_dir_recursive(&self) -> eyre::Result<Vec<Self>>
    where
        Self: Send,
    {
        let mut combined_result = vec![];
        let (dirs, files) = self.read_dir_into_vecs()?;
        combined_result.extend(files);
        let results = dirs
            .into_par_iter()
            .map(|path| -> eyre::Result<Vec<Self>> { path.read_dir_recursive() })
            .collect::<eyre::Result<Vec<_>>>()?;
        for result in results {
            combined_result.extend(result);
        }

        Ok(combined_result)
    }

    pub fn parent(&self) -> Option<Self> {
        if let Some(parent) = self.inner.parent() {
            let parent = parent.to_owned();
            let parent = Self::new(parent).expect("guaranteed by RelativePath::new");
            return Some(parent);
        }

        None
    }

    pub fn filename(&self) -> &str {
        self.inner
            .file_name()
            .expect("guaranteed by RelativePath::new")
            .to_str()
            .expect("guaranteed by RelativePath::new")
    }

    fn components(&self) -> impl Iterator<Item = &str> {
        self.inner.components().skip(1).map(|c| {
            c.as_os_str()
                .to_str()
                .expect("guaranteed by RelativePath::new")
        })
    }

    fn site_root_relative_components(&self) -> impl Iterator<Item = &str> {
        self.inner.components().map(|c| {
            c.as_os_str()
                .to_str()
                .expect("guaranteed by RelativePath::new")
        })
    }

    /// converts path to a url relative to the root directory of the kind.
    ///
    /// this is tricky to use correctly, because not all pages and feeds are in that directory.
    fn relative_url(&self) -> String {
        let components = self
            .components()
            .map(|c| urlencoding::encode(c))
            .collect::<Vec<_>>();

        components.join("/")
    }

    /// converts path to a path string relative to the root directory of the kind.
    ///
    /// this is tricky to use correctly, because not all pages and feeds are in that directory.
    fn relative_path(&self) -> String {
        let components = self.components().collect::<Vec<_>>();

        components.join(std::path::MAIN_SEPARATOR_STR)
    }

    fn site_root_relative_path_for_db(&self) -> String {
        let components = self.site_root_relative_components().collect::<Vec<_>>();

        components.join("/")
    }

    fn site_root_relative_path_for_display(&self) -> &str {
        self.inner
            .to_str()
            .expect("guaranteed by RelativePath::new")
    }
}

impl<Kind: PathKind> Serialize for RelativePath<Kind> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_dynamic_path().db_dep_table_path())
    }
}
impl<'de, Kind: PathKind> Deserialize<'de> for RelativePath<Kind> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(RelativePathVisitor(PhantomData))
    }
}
struct RelativePathVisitor<Kind>(PhantomData<Kind>);
impl<'de, Kind: PathKind> Visitor<'de> for RelativePathVisitor<Kind> {
    type Value = RelativePath<Kind>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string that is a site-root-relative path")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        RelativePath::from_site_root_relative_path(v)
            .map_err(|e| E::custom(format!("failed to parse path: {e:?}")))
    }
}

impl DynamicPath {
    #[tracing::instrument]
    pub fn from_site_root_relative_path(inner: &str) -> eyre::Result<Self> {
        if let Ok(result) = PostsPath::from_site_root_relative_path(inner) {
            return Ok(Self::Posts(result));
        }
        if let Ok(result) = SitePath::from_site_root_relative_path(inner) {
            return Ok(Self::Site(result));
        }
        if let Ok(result) = AttachmentsPath::from_site_root_relative_path(inner) {
            return Ok(Self::Attachments(result));
        }

        bail!("path is not of a known type: {inner:?}")
    }

    pub fn db_dep_table_path(&self) -> String {
        match self {
            DynamicPath::Posts(path) => path.site_root_relative_path_for_db(),
            DynamicPath::Site(path) => path.site_root_relative_path_for_db(),
            DynamicPath::Attachments(path) => path.site_root_relative_path_for_db(),
            DynamicPath::Cache(path) => path.site_root_relative_path_for_db(),
        }
    }
}

impl Display for DynamicPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DynamicPath::Posts(path) => {
                write!(f, "{:?}", path.site_root_relative_path_for_display())
            }
            DynamicPath::Site(path) => {
                write!(f, "{:?}", path.site_root_relative_path_for_display())
            }
            DynamicPath::Attachments(path) => {
                write!(f, "{:?}", path.site_root_relative_path_for_display())
            }
            DynamicPath::Cache(path) => {
                write!(f, "{:?}", path.site_root_relative_path_for_display())
            }
        }
    }
}

impl AsRef<Path> for DynamicPath {
    fn as_ref(&self) -> &Path {
        match self {
            DynamicPath::Posts(path) => path.as_ref(),
            DynamicPath::Site(path) => path.as_ref(),
            DynamicPath::Attachments(path) => path.as_ref(),
            DynamicPath::Cache(path) => path.as_ref(),
        }
    }
}

impl Serialize for DynamicPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.db_dep_table_path())
    }
}
impl<'de> Deserialize<'de> for DynamicPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(DynamicPathVisitor)
    }
}
struct DynamicPathVisitor;
impl<'de> Visitor<'de> for DynamicPathVisitor {
    type Value = DynamicPath;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string that is a site-root-relative path")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        DynamicPath::from_site_root_relative_path(v)
            .map_err(|e| E::custom(format!("failed to parse path: {e:?}")))
    }
}

pub fn hard_link_if_not_exists(
    existing: impl AsRef<Path>,
    new: impl AsRef<Path>,
) -> eyre::Result<()> {
    if let Err(error) = hard_link(existing, new) {
        if error.kind() != ErrorKind::AlreadyExists {
            Err(error).wrap_err("failed to create hard link")?;
        }
    }

    Ok(())
}

/// if the given string is a “path-relative-scheme-less-URL string”, returns that string after
/// the initial C0/space/tab/newline stripping, otherwise returns None.
///
/// - `foo/bar` → true
/// - `/foo/bar` → false
/// - `foo:/bar` → false
///
/// <https://url.spec.whatwg.org/#path-relative-scheme-less-url-string>
pub fn parse_path_relative_scheme_less_url_string(url: &str) -> Option<String> {
    // is it a “relative-URL string”? (case “Otherwise”)
    // <https://url.spec.whatwg.org/#relative-url-string>
    if Url::parse(url) == Err(url::ParseError::RelativeUrlWithoutBase) {
        // if so, it may be a “scheme-relative-URL string” or “path-absolute-URL string”, but we can
        // only check for that by running the first few steps of the “basic URL parser” on `url`
        // with an imaginary non-null “*base*”, but no “*encoding*”, “*url*”, or “*state override*”.
        //
        // the imaginary “*base*” in our case has the http or https scheme, so the scheme is a
        // “special scheme”, so “*base*” does not “have an [opaque path]”.
        //
        // <https://url.spec.whatwg.org/#scheme-relative-url-string>
        // <https://url.spec.whatwg.org/#path-absolute-url-string>
        // <https://url.spec.whatwg.org/#concept-basic-url-parser>

        // “Remove any leading and trailing [C0 control or space] from *input*.”
        let url = url.strip_prefix(|c| c <= '\x20').unwrap_or(url);
        let url = url.strip_suffix(|c| c <= '\x20').unwrap_or(url);

        // “Remove all [ASCII tab or newline] from *input*.”
        let url = url.replace(['\x09', '\x0A', '\x0D'], "");

        // “Let *state* be *state override* if given, or [scheme start state] otherwise.”
        #[derive(Debug)]
        enum State {
            #[allow(clippy::enum_variant_names)]
            SchemeStartState,
            Scheme,
            NoScheme,
            Relative,
            RelativeSlash,
        }
        let mut state = State::SchemeStartState;

        // “Let *pointer* be a [pointer] for *input*.”
        let mut pointer = &url[..];

        // “Keep running the following state machine by switching on state. If after a run
        // pointer points to the EOF code point, go to the next step.”
        loop {
            // “When a pointer is used, c references the code point the pointer points to as long
            // as it does not point nowhere. When the pointer points to nowhere c cannot be used.”
            // Some(char) = non-EOF code point; None = EOF code point; no need for a nowhere case.
            let c = pointer.chars().next();

            match state {
                State::SchemeStartState => {
                    if c.is_some_and(|c| c.is_ascii_alphabetic()) {
                        state = State::Scheme;
                    } else {
                        state = State::NoScheme;
                        continue; // skip pointer increase
                    }
                }
                State::Scheme => {
                    if c.is_some_and(|c| {
                        c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'
                    }) {
                        // do nothing
                    } else if c.is_some_and(|c| c == ':') {
                        // “Set url’s scheme to buffer.”
                        // we have an “absolute-URL string”.
                        return None;
                    } else {
                        // “Otherwise, if state override is not given, set buffer to the empty
                        // string, state to no scheme state, and start over (from the first code
                        // point in input).”
                        state = State::NoScheme;
                        pointer = &url[..];
                        continue; // skip pointer increase
                    }
                }
                State::NoScheme => {
                    // “Otherwise, if base’s scheme is not "file", set state to relative state
                    // and decrease pointer by 1.”
                    state = State::Relative;
                    continue; // skip pointer increase
                }
                State::Relative => {
                    #[allow(clippy::if_same_then_else)]
                    if c.is_some_and(|c| c == '/') {
                        state = State::RelativeSlash;
                    } else if c.is_some_and(|c| c == '\\') {
                        state = State::RelativeSlash;
                    } else {
                        // “Set [...], url’s path to a clone of base’s path, [...].”
                        // we have a “path-relative-scheme-less-URL string”.
                        return Some(url);
                    }
                }
                State::RelativeSlash => {
                    // we have a “scheme-relative-URL string” or “path-absolute-URL string”.
                    return None;
                }
            }
            if let Some(c) = c {
                // “Otherwise, increase pointer by 1 and continue with the state machine.”
                pointer = &pointer[c.len_utf8()..];
            } else {
                // “If after a run pointer points to the EOF code point, go to the next step.”
                break;
            }
        }
    }

    None
}

#[test]
fn test_is_path_relative_scheme_less_url_string() {
    assert_eq!(
        parse_path_relative_scheme_less_url_string(" http://host/absolute?query#fragment"),
        None
    );
    assert_eq!(
        parse_path_relative_scheme_less_url_string(" //host/absolute?query#fragment"),
        None
    );
    assert_eq!(
        parse_path_relative_scheme_less_url_string(" /absolute?query#fragment"),
        None
    );
    assert_eq!(
        parse_path_relative_scheme_less_url_string(" relative?query#fragment").as_deref(),
        Some("relative?query#fragment")
    );
    assert_eq!(
        parse_path_relative_scheme_less_url_string(" script.js").as_deref(),
        Some("script.js")
    );
    assert_eq!(
        parse_path_relative_scheme_less_url_string(" script2.js").as_deref(),
        Some("script2.js")
    );
    assert_eq!(
        parse_path_relative_scheme_less_url_string(" 2script.js").as_deref(),
        Some("2script.js")
    );
}
