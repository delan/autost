use std::{
    fs::{hard_link, DirEntry},
    io::ErrorKind,
    path::{Component, Path, PathBuf},
    sync::LazyLock,
};

use jane_eyre::eyre::{self, bail, Context};

use crate::SETTINGS;

pub type PostsPath = RelativePath<PostsKind>;
pub type SitePath = RelativePath<SiteKind>;
pub type AttachmentsPath = RelativePath<AttachmentsKind>;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[allow(private_bounds)]
pub struct RelativePath<Kind: PathKind> {
    inner: PathBuf,
    kind: Kind,
}

trait PathKind: Sized {
    const ROOT: &'static str;
    fn new(path: &Path) -> eyre::Result<Self>;
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum PostsKind {
    Post { is_markdown: bool },
    Other,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SiteKind {
    Attachments,
    Other,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AttachmentsKind {}

impl PathKind for PostsKind {
    const ROOT: &'static str = "posts";

    fn new(path: &Path) -> eyre::Result<Self> {
        let mut components = path.components().skip(1);
        if let Some(component) = components.next().and_then(|c| c.as_os_str().to_str()) {
            if components.next().is_none() {
                if component.ends_with(".html") {
                    return Ok(Self::Post { is_markdown: false });
                }
                if component.ends_with(".md") {
                    return Ok(Self::Post { is_markdown: true });
                }
            }
        }

        Ok(Self::Other)
    }
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

impl PostsPath {
    pub const ROOT: LazyLock<Self> =
        LazyLock::new(|| Self::new(PostsKind::ROOT.into()).expect("guaranteed by argument"));

    /// creates a path from `<link rel=references href>`, which is relative to
    /// the posts directory, but percent-encoded as a url.
    pub fn from_references_url(references: &str) -> eyre::Result<Self> {
        let references = urlencoding::decode(references)?;
        let path = Path::new(PostsKind::ROOT).join(&*references);

        Self::new(path)
    }

    pub fn markdown_post_path(post_id: usize) -> Self {
        Self::ROOT
            .join(&format!("{post_id}.md"))
            .expect("guaranteed by argument")
    }

    pub fn generated_post_path(post_id: usize) -> Self {
        Self::ROOT
            .join(&format!("{post_id}.html"))
            .expect("guaranteed by argument")
    }

    pub fn references_dir(post_id: usize) -> Self {
        Self::ROOT
            .join(&format!("{post_id}"))
            .expect("guaranteed by argument")
    }

    pub fn references_post_path(post_id: usize, references_post_id: usize) -> Self {
        Self::ROOT
            .join(&format!("{post_id}/{references_post_id}.html"))
            .expect("guaranteed by argument")
    }

    pub fn references_url(&self) -> String {
        self.relative_url()
    }

    pub fn rendered_path(&self) -> eyre::Result<Option<SitePath>> {
        match self.kind {
            PostsKind::Post { .. } => {
                let (basename, _) = self
                    .filename()
                    .rsplit_once(".")
                    .expect("guaranteed by PostKind::new");
                let filename = format!("{basename}.html");
                Ok(Some(SitePath::ROOT.join(&filename)?))
            }
            PostsKind::Other => Ok(None),
        }
    }

    pub fn is_markdown_post(&self) -> bool {
        matches!(self.kind, PostsKind::Post { is_markdown: true })
    }
}

impl SitePath {
    pub const ROOT: LazyLock<Self> =
        LazyLock::new(|| Self::new(SiteKind::ROOT.into()).expect("guaranteed by argument"));
    pub const TAGGED: LazyLock<Self> =
        LazyLock::new(|| Self::ROOT.join("tagged").expect("guaranteed by argument"));
    pub const ATTACHMENTS: LazyLock<Self> = LazyLock::new(|| {
        Self::ROOT
            .join("attachments")
            .expect("guaranteed by argument")
    });
    pub const THUMBS: LazyLock<Self> = LazyLock::new(|| {
        Self::ATTACHMENTS
            .join("thumbs")
            .expect("guaranteed by argument")
    });
    pub const DUMMY_POST: LazyLock<Self> =
        LazyLock::new(|| Self::ROOT.join("0.html").expect("guaranteed by argument"));

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

impl AttachmentsPath {
    pub const ROOT: LazyLock<Self> =
        LazyLock::new(|| Self::new(AttachmentsKind::ROOT.into()).expect("guaranteed by argument"));
    pub const THUMBS: LazyLock<Self> =
        LazyLock::new(|| Self::ROOT.join("thumbs").expect("guaranteed by argument"));

    pub fn site_path(&self) -> eyre::Result<SitePath> {
        let mut result = SitePath::ATTACHMENTS.to_owned();
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
