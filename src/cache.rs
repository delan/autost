use std::{
    collections::BTreeSet,
    env::current_exe,
    fmt::Display,
    fs::{read, File},
    io::Write,
    path::Path,
    sync::LazyLock,
};

use atomic_write_file::AtomicWriteFile;
use jane_eyre::eyre::{self, bail, Context};
use rayon::iter::{IndexedParallelIterator, IntoParallelRefIterator, ParallelIterator as _};
use serde::{de::Visitor, Deserialize, Serialize};

use crate::{
    path::{CachePath, DynamicPath, CACHE_PATH_ROOT, POSTS_PATH_ROOT},
    FilteredPost, UnsafePost,
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
struct Id(blake3::Hash);
trait ComputeId {
    fn compute_id(&self) -> Id;
}
impl PartialOrd for Id {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Id {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.as_bytes().cmp(other.0.as_bytes())
    }
}
impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_hex().as_str())
    }
}
impl Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.to_hex().as_str())
    }
}
impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(IdVisitor)
    }
}
struct IdVisitor;
impl<'de> Visitor<'de> for IdVisitor {
    type Value = Id;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string that is 64 hex digits")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        let result = blake3::Hash::from_hex(v)
            .map_err(|e| E::custom(format!("failed to parse hash: {e:?}")))?;
        Ok(Id(result))
    }
}

#[derive(Debug, Serialize)]
struct DerivationInit {
    input_derivations: BTreeSet<Id>,
    input_sources: BTreeSet<DynamicPath>,
    builder: Builder,
}
impl ComputeId for DerivationInit {
    fn compute_id(&self) -> Id {
        let result = serde_json::to_vec(self).expect("guaranteed by derive Serialize");
        Id(blake3::hash(&result))
    }
}
#[derive(Debug, Deserialize, Serialize)]
struct Derivation {
    output: Id,
    input_derivations: BTreeSet<Id>,
    input_sources: BTreeSet<DynamicPath>,
    builder: Builder,
}
impl From<DerivationInit> for Derivation {
    fn from(value: DerivationInit) -> Self {
        let output = value.compute_id();
        Self {
            output,
            input_derivations: value.input_derivations,
            input_sources: value.input_sources,
            builder: value.builder,
        }
    }
}
impl Display for Derivation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&serde_json::to_string(self).expect("guaranteed by derive Serialize"))
    }
}
impl Derivation {
    fn read_file(path: DynamicPath) -> Self {
        Self::from(DerivationInit {
            input_derivations: [].into_iter().collect(),
            input_sources: [path].into_iter().collect(),
            builder: Builder::ReadFile,
        })
    }

    fn filtered_post(post_file: &Derivation) -> Self {
        Self::from(DerivationInit {
            input_derivations: [post_file.id()].into_iter().collect(),
            input_sources: [].into_iter().collect(),
            builder: Builder::FilteredPost,
        })
    }

    fn id(&self) -> Id {
        self.output
    }

    fn derivation_path(id: Id) -> eyre::Result<CachePath> {
        CACHE_PATH_ROOT.join(&format!("{id}.drv"))
    }

    fn output_path(&self) -> eyre::Result<CachePath> {
        CACHE_PATH_ROOT.join(&format!("{}.out", self.id()))
    }

    fn load(id: Id) -> eyre::Result<Self> {
        Ok(serde_json::from_reader(File::open(
            Self::derivation_path(id)?,
        )?)?)
    }

    fn store(self) -> eyre::Result<Self> {
        let mut file = AtomicWriteFile::open(Self::derivation_path(self.id())?)?;
        serde_json::to_writer(&mut file, &self)?;
        file.commit()?;

        Ok(self)
    }

    fn realise(&self) -> eyre::Result<Vec<u8>> {
        // realise any derivations this derivation depends on.
        let mut input_derivations = vec![];
        for id in &self.input_derivations {
            let derivation = Self::load(*id)?;
            let content = derivation.realise()?;
            input_derivations.push((derivation, content));
        }
        // use cached output, if previously realised.
        if let Ok(result) = read(self.output_path()?) {
            return Ok(result);
        }
        // build the derivation and cache its output.
        let result = (|| {
            let content = match self.builder {
                Builder::ReadFile => {
                    let [path] = self.input_sources.iter().collect::<Vec<_>>()[..] else {
                        bail!("expected exactly one path in `input_sources`");
                    };
                    read(path)?
                }
                Builder::FilteredPost => {
                    let [(_derivation, source)] = input_derivations.iter().collect::<Vec<_>>()[..]
                    else {
                        bail!("expected exactly one derivation in `input_derivations`");
                    };
                    // TODO: handle html case
                    let source = str::from_utf8(source)?;
                    let post = UnsafePost::with_markdown(source);
                    let post = FilteredPost::filter(post)?;
                    serde_json::to_vec(&post)?
                }
            };
            let mut file = AtomicWriteFile::open(self.output_path()?)?;
            file.write_all(&content)?;
            file.commit()?;
            Ok(content)
        })();
        result.wrap_err_with(|| format!("failed to realise derivation: {self:?}"))
    }

    fn realise_string(&self) -> eyre::Result<String> {
        Ok(String::from_utf8(self.realise()?)?)
    }
}

#[derive(Debug, Deserialize, Serialize)]
enum Builder {
    ReadFile,
    FilteredPost,
}
impl Builder {}

pub async fn test() -> eyre::Result<()> {
    let top_level_post_paths = POSTS_PATH_ROOT.read_dir_flat()?;
    let results = top_level_post_paths
        .par_iter()
        .enumerate()
        .map(|(i, path)| -> eyre::Result<_> {
            let post_file = Derivation::read_file(path.to_dynamic_path()).store()?;
            let post_meta = Derivation::filtered_post(&post_file).store()?;
            let output = post_meta.realise_string()?;
            Ok((i, output.len()))
        })
        .collect::<Vec<_>>();
    for result in results {
        dbg!(result?);
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use jane_eyre::eyre;

    use crate::{cache::Derivation, path::DynamicPath};

    #[test]
    fn test_derivation() -> eyre::Result<()> {
        let derivation = Derivation::read_file(DynamicPath::from_site_root_relative_path("posts")?);
        assert_eq!(serde_json::to_string(&derivation)?, "{\"output\":\"9f4a6eab337807103c4b31e7f1cfb30706e5662f6af4ce060db12d6625075247\",\"input_derivations\":[],\"input_sources\":[\"posts\"],\"builder\":\"ReadFile\"}");

        Ok(())
    }
}
