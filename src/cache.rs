use std::{
    env::current_exe, fmt::{Debug, Display}, fs::{exists}, io::Write, path::Path, pin::Pin, sync::LazyLock
};

use atomic_write_file::{unix::OpenOptionsExt, AtomicWriteFile};
use bincode::config::standard;
use dashmap::DashMap;
use futures::FutureExt;
use jane_eyre::eyre::{self, bail, Context};
use serde::{de::Visitor, Deserialize, Serialize};
use tokio::{fs::read, spawn};
use tracing::debug;

use crate::{
    path::{DynamicPath, POSTS_PATH_ROOT},
    render_markdown, FilteredPost, Thread, UnsafePost,
};

pub static HASHER: LazyLock<blake3::Hasher> = LazyLock::new(|| {
    let mut hasher = blake3::Hasher::new();
    let exe = current_exe().expect("failed to get path to executable");
    hasher
        .update_mmap_rayon(exe)
        .expect("failed to hash executable");
    hasher.update(hasher.finalize().as_bytes());
    hasher
});

static DERIVATION_CACHE: LazyLock<DashMap<Id, Derivation>> = LazyLock::new(DashMap::new);
static FILTERED_POST_CACHE: LazyLock<DashMap<Id, FilteredPost>> = LazyLock::new(DashMap::new);

pub fn hash_bytes(bytes: impl AsRef<[u8]>) -> blake3::Hash {
    HASHER.clone().update(bytes.as_ref()).finalize()
}

pub fn hash_file(path: impl AsRef<Path>) -> eyre::Result<blake3::Hash> {
    let mut hasher = HASHER.clone();
    hasher.update_mmap_rayon(path)?;

    Ok(hasher.finalize())
}

pub fn parse_hash_hex(input: &str) -> eyre::Result<blake3::Hash> {
    Ok(blake3::Hash::from_hex(input)?)
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct Hash(blake3::Hash);
impl PartialOrd for Hash {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Hash {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}
impl Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hash = self.0.to_hex();
        write!(f, "{}...", &hash.as_str()[0..13])
    }
}
impl Serialize for Hash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.to_hex().as_str())
    }
}
impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(HashVisitor)
    }
}
struct HashVisitor;
impl<'de> Visitor<'de> for HashVisitor {
    type Value = Hash;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string that is 64 hex digits")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let result = blake3::Hash::from_hex(v)
            .map_err(|e| E::custom(format!("failed to parse hash: {e:?}")))?;
        Ok(Hash(result))
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct Id(Hash);
trait ComputeId {
    fn compute_id(&self) -> Id;
}
impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
enum Builder {
    ReadFile {
        path: DynamicPath,
        hash: Hash,
    },
    RenderMarkdown {
        file: Box<Derivation>,
    },
    FilteredPost {
        file: Box<Derivation>,
    },
    Thread {
        post: Box<Derivation>,
        references: Vec<Derivation>,
    },
}
impl Display for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Builder::ReadFile { path, hash } => f
                .debug_struct("ReadFile")
                .field("path", &UseDisplay(path))
                .field("hash", &UseDisplay(hash))
                .finish(),
            Builder::RenderMarkdown { file } => f
                .debug_struct("RenderMarkdown")
                .field("file", &UseDisplay(&**file))
                .finish(),
            Builder::FilteredPost { file } => f
                .debug_struct("FilteredPost")
                .field("file", &UseDisplay(&**file))
                .finish(),
            Builder::Thread { post, references } => f
                .debug_struct("Thread")
                .field("post", &UseDisplay(&**post))
                .field("references", &VecDisplay(references))
                .finish(),
        }
    }
}
impl ComputeId for Builder {
    fn compute_id(&self) -> Id {
        let result = bincode::serde::encode_to_vec(self, standard())
            .expect("guaranteed by derive Serialize");
        Id(Hash(blake3::hash(&result)))
    }
}
#[derive(Debug)]
struct RenderMarkdownInput {
    file: Vec<u8>,
}
#[derive(Debug)]
struct FilteredPostInput {
    file: Vec<u8>,
}
#[derive(Debug)]
struct ThreadInput {
    post: FilteredPost,
    references: Vec<FilteredPost>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Derivation {
    output: Id,
    builder: Builder,
}
impl Display for Derivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Derivation({} -> {})", self.output.0, self.builder)
    }
}
struct UseDisplay<'d, D: Display>(&'d D);
struct VecDisplay<'d, D: Display>(&'d [D]);
impl<'d, D: Display> Debug for UseDisplay<'d, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl<'d, D: Display> Debug for VecDisplay<'d, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|value| UseDisplay(value)))
            .finish()
    }
}
impl From<Builder> for Derivation {
    fn from(builder: Builder) -> Self {
        let output = builder.compute_id();
        Self { output, builder }
    }
}
impl Derivation {
    async fn read_file(path: DynamicPath) -> eyre::Result<Self> {
        let hash = Hash(blake3::hash(&read(&path).await?));
        Ok(Self::from(Builder::ReadFile { path, hash }))
    }

    async fn render_markdown(path: DynamicPath) -> eyre::Result<Self> {
        Ok(Self::from(Builder::RenderMarkdown {
            file: Self::read_file(path).await?.store()?.into(),
        }))
    }

    async fn filtered_post(path: DynamicPath) -> eyre::Result<Self> {
        let DynamicPath::Posts(posts_path) = &path else {
            bail!("path is not a posts path")
        };
        let file = if posts_path.is_markdown_post() {
            Self::render_markdown(path).await?
        } else {
            Self::read_file(path).await?
        };
        Ok(Self::from(Builder::FilteredPost {
            file: file.store()?.into(),
        }))
    }

    async fn thread(path: DynamicPath) -> eyre::Result<Self> {
        let post_derivation = Self::filtered_post(path).await?.store()?;
        // TODO: can we avoid realise() during evaluation?
        // (probably not, because it’s like we’re forced to do an IFD in this situation?)
        let post = if let Some(post) = FILTERED_POST_CACHE.get(&post_derivation.id()) {
            post.clone()
        } else {
            let post = Self::load(post_derivation.id()).await?.expect().await?;
            bincode::serde::decode_from_slice(&post, standard())?.0
        };
        let mut references = vec![];
        for path in post.meta.front_matter.references.iter() {
            references.push(Self::filtered_post(path.to_dynamic_path()).await?.store()?);
        }
        Ok(Self::from(Builder::Thread {
            post: post_derivation.into(),
            references,
        }))
    }

    fn id(&self) -> Id {
        self.output
    }

    fn derivation_path(id: Id) -> String {
        format!("cache/{id}.drv")
    }

    fn output_path(&self) -> String {
        format!("cache/{}.out", self.id())
    }

    fn needs(&self) -> Vec<&Derivation> {
        match &self.builder {
            Builder::ReadFile { .. } => vec![],
            Builder::RenderMarkdown { file } => vec![&**file],
            Builder::FilteredPost { file } => vec![&**file],
            Builder::Thread { post, references } => {
                let mut result = vec![&**post];
                result.extend(references.iter());
                result
            }
        }
    }

    async fn load(id: Id) -> eyre::Result<Self> {
        if let Some(result) = DERIVATION_CACHE.get(&id) {
            Ok(result.clone())
        } else {
            Ok(bincode::serde::decode_from_slice(
                &read(Self::derivation_path(id)).await?,
                standard(),
            )?.0)
        }
    }

    fn store(self) -> eyre::Result<Self> {
        let path = Self::derivation_path(self.id());
        if !exists(&path)? {
            let mut file = atomic_writer(path)?;
            bincode::serde::encode_into_std_write(&self, &mut file, standard())?;
            file.commit()?;
            DERIVATION_CACHE.insert(self.id(), self.clone());
        }

        Ok(self)
    }

    async fn expect(&self) -> eyre::Result<Vec<u8>> {
        Ok(read(self.output_path()).await?)
    }

    async fn realise(&self) -> eyre::Result<Vec<u8>> {
        // use cached output, if previously realised.
        if let Ok(result) = read(self.output_path()).await {
            return Ok(result);
        }
        // build the derivation and cache its output.
        debug!("building {self}");
        let result = (async || {
            let content = match &self.builder {
                Builder::ReadFile { path, hash } => {
                    let output = read(path).await?;
                    let actual_hash = Hash(blake3::hash(&output));
                    if &actual_hash != hash {
                        bail!("hash mismatch! expected {hash}, actual {actual_hash}");
                    }
                    output
                }
                Builder::RenderMarkdown { file } => {
                    let input = RenderMarkdownInput {
                        file: Self::load(file.id()).await?.expect().await?,
                    };
                    let unsafe_markdown = input.file;
                    render_markdown(str::from_utf8(&unsafe_markdown)?).into_bytes()
                }
                Builder::FilteredPost { file } => {
                    let input = FilteredPostInput {
                        file: Self::load(file.id()).await?.expect().await?,
                    };
                    let unsafe_html = input.file;
                    let unsafe_html = str::from_utf8(&unsafe_html)?;
                    let post = UnsafePost::with_html(unsafe_html);
                    let post = FilteredPost::filter(post)?;
                    let output = bincode::serde::encode_to_vec(&post, standard())?;
                    FILTERED_POST_CACHE.insert(self.id(), post);
                    output
                }
                Builder::Thread { post, references } => {
                    let load_filtered_post_cached = async |id| -> eyre::Result<_> {
                        if let Some(post) = FILTERED_POST_CACHE.get(&id) {
                            Ok(post.clone())
                        } else {
                            let post = Self::load(id).await?.expect().await?;
                            Ok(bincode::serde::decode_from_slice(&post, standard())?.0)
                        }
                    };
                    let post_result = spawn(load_filtered_post_cached(post.id()));
                    let mut references_results = vec![];
                    for post in references
                        .iter()
                        .map(|post| spawn(load_filtered_post_cached(post.id())))
                        .collect::<Vec<_>>() {
                        references_results.push(post.await??);
                    }
                    let input = ThreadInput {
                        post: post_result.await??,
                        references: references_results,
                    };
                    let thread = Thread::new(input.post, input.references);
                    bincode::serde::encode_to_vec(&thread, standard())?
                }
            };
            atomic_write(self.output_path(), &content)?;
            Ok(content)
        })();
        result.await.wrap_err_with(|| format!("failed to realise derivation: {self:?}"))
    }
}

pub async fn test() -> eyre::Result<()> {
    let top_level_post_paths = POSTS_PATH_ROOT.read_dir_flat()?;
    let filtered_posts = top_level_post_paths
        .clone()
        .into_iter()
        .enumerate()
        .map(|(i, path)| (i, spawn(async move { Derivation::filtered_post(path.to_dynamic_path()).await.map(build) })))
        .collect::<Vec<_>>();
    let len = filtered_posts.len();
    for (i, post) in filtered_posts {
        eprint!("... {i}/{len}\r");
        post.await??.await?;
    }
    eprintln!();
    let threads = top_level_post_paths
        .clone()
        .into_iter()
        .enumerate()
        .map(|(i, path)| (i, spawn(async move { Derivation::thread(path.to_dynamic_path()).await.map(build) })))
        .collect::<Vec<_>>();
    let len = threads.len();
    for (i, thread) in threads {
        eprint!("... {i}/{len}\r");
        thread.await??.await?;
    }
    eprintln!();

    Ok(())
}

fn build(derivation: Derivation) -> Pin<Box<dyn futures::Future<Output = eyre::Result<()>> + std::marker::Send>> {
    async move {
        let needs = derivation
            .needs()
            .into_iter()
            .map(|dependency| spawn(build(dependency.clone())))
            .collect::<Vec<_>>();
        for dependency in needs {
            dependency.await??;
        }
        derivation.realise().await?;
        Ok(())
    }.boxed()
}

fn atomic_writer(path: impl AsRef<Path>) -> eyre::Result<AtomicWriteFile> {
    Ok(AtomicWriteFile::options()
        .preserve_mode(false)
        .preserve_owner(false)
        .try_preserve_owner(false)
        .open(path)?)
}

fn atomic_write(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> eyre::Result<()> {
    let mut file = atomic_writer(path)?;
    file.write_all(content.as_ref())?;
    file.commit()?;

    Ok(())
}
