use std::{
    env::current_exe,
    fmt::Display,
    fs::{exists, read, File},
    io::Write,
    mem::take,
    path::Path,
    sync::LazyLock,
};

use atomic_write_file::{unix::OpenOptionsExt, AtomicWriteFile};
use bincode::config::standard;
use jane_eyre::eyre::{self, bail, Context};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator as _};
use serde::{de::Visitor, Deserialize, Serialize};
use tracing::{debug, info};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        write!(f, "{}", self.0.to_hex().as_str())
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Id(Hash);
trait ComputeId {
    fn compute_id(&self) -> Id;
}
impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
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
    post: Vec<u8>,
    references: Vec<Vec<u8>>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
struct Derivation {
    output: Id,
    builder: Builder,
}
impl From<Builder> for Derivation {
    fn from(builder: Builder) -> Self {
        let output = builder.compute_id();
        Self { output, builder }
    }
}
impl Derivation {
    fn read_file(path: DynamicPath) -> eyre::Result<Self> {
        let hash = Hash(blake3::hash(&read(&path)?));
        Ok(Self::from(Builder::ReadFile { path, hash }))
    }

    fn render_markdown(path: DynamicPath) -> eyre::Result<Self> {
        Ok(Self::from(Builder::RenderMarkdown {
            file: Self::read_file(path)?.store()?.into(),
        }))
    }

    fn filtered_post(path: DynamicPath) -> eyre::Result<Self> {
        let DynamicPath::Posts(posts_path) = &path else {
            bail!("path is not a posts path")
        };
        let file = if posts_path.is_markdown_post() {
            Self::render_markdown(path)?
        } else {
            Self::read_file(path)?
        };
        Ok(Self::from(Builder::FilteredPost {
            file: file.store()?.into(),
        }))
    }

    fn thread(path: DynamicPath) -> eyre::Result<Self> {
        let post_derivation = Self::filtered_post(path)?.store()?;
        // TODO: can we avoid realise() during evaluation?
        // (probably not, because it’s like we’re forced to do an IFD in this situation?)
        let post = post_derivation.realise()?;
        let (post, _): (FilteredPost, _) = bincode::serde::decode_from_slice(&post, standard())?;
        let mut references = vec![];
        for path in post.meta.front_matter.references.iter() {
            references.push(Self::filtered_post(path.to_dynamic_path())?.store()?);
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

    fn load(id: Id) -> eyre::Result<Self> {
        Ok(bincode::serde::decode_from_std_read(
            &mut File::open(Self::derivation_path(id))?,
            standard(),
        )?)
    }

    fn store(self) -> eyre::Result<Self> {
        let path = Self::derivation_path(self.id());
        if !exists(&path)? {
            let mut file = atomic_writer(path)?;
            bincode::serde::encode_into_std_write(&self, &mut file, standard())?;
            file.commit()?;
        }

        Ok(self)
    }

    fn expect(&self) -> eyre::Result<Vec<u8>> {
        Ok(read(self.output_path())?)
    }

    fn realise(&self) -> eyre::Result<Vec<u8>> {
        // use cached output, if previously realised.
        if let Ok(result) = read(self.output_path()) {
            return Ok(result);
        }
        // build the derivation and cache its output.
        info!("building {self:?}");
        let result = (|| {
            let content = match &self.builder {
                Builder::ReadFile { path, hash } => {
                    let output = read(path)?;
                    let actual_hash = Hash(blake3::hash(&output));
                    if &actual_hash != hash {
                        bail!("hash mismatch! expected {hash}, actual {actual_hash}");
                    }
                    output
                }
                Builder::RenderMarkdown { file } => {
                    let input = RenderMarkdownInput {
                        file: Self::load(file.id())?.expect()?,
                    };
                    let unsafe_markdown = input.file;
                    render_markdown(str::from_utf8(&unsafe_markdown)?).into_bytes()
                }
                Builder::FilteredPost { file } => {
                    let input = FilteredPostInput {
                        file: Self::load(file.id())?.expect()?,
                    };
                    let unsafe_html = input.file;
                    let unsafe_html = str::from_utf8(&unsafe_html)?;
                    let post = UnsafePost::with_html(unsafe_html);
                    let post = FilteredPost::filter(post)?;
                    bincode::serde::encode_to_vec(&post, standard())?
                }
                Builder::Thread { post, references } => {
                    let input = ThreadInput {
                        post: Self::load(post.id())?.expect()?,
                        references: references
                            .iter()
                            .map(|post| Self::load(post.id())?.expect())
                            .collect::<eyre::Result<_>>()?,
                    };
                    let (post, _): (FilteredPost, _) =
                        bincode::serde::decode_from_slice(&input.post, standard())?;
                    let references = input
                        .references
                        .iter()
                        .map(|post| Ok(bincode::serde::decode_from_slice(post, standard())?.0))
                        .collect::<eyre::Result<Vec<FilteredPost>>>()?;
                    let thread = Thread::new(post, references);
                    bincode::serde::encode_to_vec(&thread, standard())?
                }
            };
            atomic_write(self.output_path(), &content)?;
            Ok(content)
        })();
        result.wrap_err_with(|| format!("failed to realise derivation: {self:?}"))
    }
}

pub async fn test() -> eyre::Result<()> {
    let top_level_post_paths = POSTS_PATH_ROOT.read_dir_flat()?;
    let filtered_posts = top_level_post_paths
        .par_iter()
        .map(|path| Derivation::filtered_post(path.to_dynamic_path()))
        .collect::<eyre::Result<Vec<_>>>()?;
    build(filtered_posts)?;
    let threads = top_level_post_paths
        .par_iter()
        .map(|path| Derivation::thread(path.to_dynamic_path()))
        .collect::<eyre::Result<Vec<_>>>()?;
    build(threads)?;

    Ok(())
}

fn build(mut new_derivations: Vec<Derivation>) -> eyre::Result<()> {
    // TODO: do we need to avoid cycles somehow?
    let mut derivation_tiers = Vec::default();
    for depth in 0.. {
        if new_derivations.is_empty() {
            break;
        }
        let mut derivation_tier = vec![];
        // TODO: parallel?
        for derivation in take(&mut new_derivations) {
            new_derivations.extend(derivation.needs().into_iter().cloned());
            debug!("[{depth}] {derivation:?}");
            derivation_tier.push(derivation);
        }
        derivation_tiers.push(derivation_tier);
    }
    for (i, tier) in derivation_tiers.into_iter().enumerate().rev() {
        let results = tier
            .into_par_iter()
            .map(|derivation| derivation.realise())
            .collect::<Vec<_>>();
        info!("tier {}, len {}", i, results.len());
        for result in results {
            result?;
        }
    }

    Ok(())
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
