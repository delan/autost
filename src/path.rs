use std::{
    fs::{hard_link, DirEntry},
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    sync::LazyLock,
};

use jane_eyre::eyre::{self, bail, Context, OptionExt};
use url::Url;

use crate::SETTINGS;

#[allow(clippy::module_name_repetitions)]
pub type PostsPath = RelativePath<PostsKind>;
#[allow(clippy::module_name_repetitions)]
pub type SitePath = RelativePath<SiteKind>;
#[allow(clippy::module_name_repetitions)]
pub type AttachmentsPath = RelativePath<AttachmentsKind>;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[allow(private_bounds)]
#[allow(clippy::module_name_repetitions)]
pub struct RelativePath<Kind: PathKind> {
    inner: PathBuf,
    kind: Kind,
}

trait PathKind: Sized {
    const ROOT: &'static str;
    fn new(path: &Path) -> eyre::Result<Self>;
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PostsKind {
    Post {
        is_markdown: bool,
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
            [c] if std::path::Path::new(c)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("html")) =>
            {
                Self::Post {
                    is_markdown: false,
                    in_imported_dir: false,
                }
            }
            [c] if std::path::Path::new(c)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md")) =>
            {
                Self::Post {
                    is_markdown: true,
                    in_imported_dir: false,
                }
            }
            ["imported", c]
                if std::path::Path::new(c)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("html")) =>
            {
                Self::Post {
                    is_markdown: false,
                    in_imported_dir: true,
                }
            }
            _ => Self::Other,
        })
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
            in_imported_dir: false
        }
    );
    assert_eq!(
        PostsKind::new(Path::new("posts/1.md"))?,
        PostsKind::Post {
            is_markdown: true,
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
}

impl PathKind for AttachmentsKind {
    const ROOT: &'static str = "attachments";

    fn new(_path: &Path) -> eyre::Result<Self> {
        Ok(Self {})
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

    #[must_use]
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
                    .rsplit_once('.')
                    .expect("guaranteed by PostsKind::new");
                let filename = format!("{basename}.html");
                Ok(Some(SITE_PATH_ROOT.join(&filename)?))
            }
            PostsKind::Other => Ok(None),
        }
    }

    #[must_use]
    pub const fn is_markdown_post(&self) -> bool {
        matches!(
            self.kind,
            PostsKind::Post {
                is_markdown: true,
                ..
            }
        )
    }

    #[must_use]
    pub fn basename(&self) -> Option<&str> {
        if let PostsKind::Post {
            in_imported_dir: true,
            ..
        } = self.kind
        {
            let (basename, _) = self
                .filename()
                .rsplit_once('.')
                .expect("guaranteed by PostsKind::new");
            return Some(basename);
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

    /// use this only in post authoring contexts, like the output of importers.
    #[must_use]
    pub fn base_relative_url(&self) -> String {
        self.relative_url()
    }

    #[must_use]
    pub fn internal_url(&self) -> String {
        format!("{}{}", SETTINGS.base_url, self.relative_url())
    }

    #[must_use]
    pub fn external_url(&self) -> String {
        format!("{}{}", SETTINGS.external_base_url, self.relative_url())
    }

    #[must_use]
    pub fn atom_feed_entry_id(&self) -> String {
        // TODO: this violates the atom spec (#6)
        self.relative_url()
    }

    #[must_use]
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
#[must_use]
pub fn parse_path_relative_scheme_less_url_string(url: &str) -> Option<String> {
    #[derive(Debug)]
    enum State {
        SchemeStart,
        Scheme,
        NoScheme,
        Relative,
        RelativeSlash,
    }
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
        let mut state = State::SchemeStart;

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
                State::SchemeStart => {
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
                    if c.is_some_and(|c| c == '/') || c.is_some_and(|c| c == '\\') {
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
