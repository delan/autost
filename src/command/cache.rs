use jane_eyre::eyre;
use rayon::iter::{IntoParallelIterator as _, ParallelIterator as _};
use sha2::{
    digest::{ExtendableOutput, XofReader},
    Digest,
};

use std::{fs::read, path::Path};

use crate::path::{ATTACHMENTS_PATH_ROOT, POSTS_PATH_ROOT};

#[derive(clap::Subcommand, Debug)]
pub enum Cache {
    Benchmark(Benchmark),
    Test(Test),
}

#[derive(clap::Args, Debug)]
pub struct Benchmark {
    dir: Dir,
    algorithm: Algorithm,
    count: usize,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq)]
pub enum Dir {
    Posts,
    PostsRecursive,
    Attachments,
}

#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq)]
pub enum Algorithm {
    SumPathsLen,
    SumReadLen,
    Sha256,
    Sha512,
    Sha3_256,
    TurboShake128,
    Blake3,
    Blake3MmapRayon,
}

#[derive(clap::Args, Debug)]
pub struct Test {
    #[arg(long)]
    pub use_cache: bool,
    #[arg(long)]
    pub use_packs: bool,
    #[arg(long)]
    pub list_threads_in_tag: Option<String>,
}

pub async fn main(args: Cache) -> eyre::Result<()> {
    match args {
        Cache::Benchmark(args) => do_benchmark(args).await,
        Cache::Test(args) => crate::cache::test(args).await,
    }
}

async fn do_benchmark(args: Benchmark) -> eyre::Result<()> {
    let mut sum_paths_len = 0;
    let mut sum_read_len = 0;
    let mut sum_sha256 = sha2::Sha256::new();
    let mut sum_sha512 = sha2::Sha512::new();
    let mut sum_sha3_256 = sha3::Sha3_256::new();
    let mut sum_turboshake128 = turboshake128();
    let mut sum_blake3 = blake3::Hasher::new();
    for _ in 0..args.count {
        let paths: Vec<String> = match args.dir {
            Dir::Posts => POSTS_PATH_ROOT
                .read_dir_flat()?
                .into_iter()
                .map(|path| AsRef::<Path>::as_ref(&path).to_str().unwrap().to_owned())
                .collect(),
            Dir::PostsRecursive => POSTS_PATH_ROOT
                .read_dir_recursive()?
                .into_iter()
                .map(|path| AsRef::<Path>::as_ref(&path).to_str().unwrap().to_owned())
                .collect(),
            Dir::Attachments => ATTACHMENTS_PATH_ROOT
                .read_dir_recursive()?
                .into_iter()
                .map(|path| AsRef::<Path>::as_ref(&path).to_str().unwrap().to_owned())
                .collect(),
        };
        match args.algorithm {
            Algorithm::SumPathsLen => sum_paths_len += paths.len(),
            Algorithm::SumReadLen => {
                let results = paths
                    .into_par_iter()
                    .map(|path| -> eyre::Result<usize> { Ok(read(path)?.len()) })
                    .collect::<eyre::Result<Vec<usize>>>()?;
                sum_read_len += results.into_iter().sum::<usize>();
            }
            Algorithm::Sha256 => {
                let results = paths
                    .into_par_iter()
                    .map(|path| -> eyre::Result<[u8; 32]> {
                        let mut hasher = sha2::Sha256::new();
                        sha2::Digest::update(&mut hasher, read(path)?);
                        Ok(hasher.finalize().into())
                    })
                    .collect::<eyre::Result<Vec<[u8; 32]>>>()?;
                for hash in results {
                    sha2::Digest::update(&mut sum_sha256, hash);
                }
            }
            Algorithm::Sha512 => {
                let results = paths
                    .into_par_iter()
                    .map(|path| -> eyre::Result<[u8; 64]> {
                        let mut hasher = sha2::Sha512::new();
                        sha2::Digest::update(&mut hasher, read(path)?);
                        Ok(hasher.finalize().into())
                    })
                    .collect::<eyre::Result<Vec<[u8; 64]>>>()?;
                for hash in results {
                    sha2::Digest::update(&mut sum_sha512, hash);
                }
            }
            Algorithm::Sha3_256 => {
                let results = paths
                    .into_par_iter()
                    .map(|path| -> eyre::Result<[u8; 32]> {
                        let mut hasher = sha3::Sha3_256::new();
                        sha2::Digest::update(&mut hasher, read(path)?);
                        Ok(hasher.finalize().into())
                    })
                    .collect::<eyre::Result<Vec<[u8; 32]>>>()?;
                for hash in results {
                    sha3::Digest::update(&mut sum_sha3_256, hash);
                }
            }
            Algorithm::TurboShake128 => {
                let results = paths
                    .into_par_iter()
                    .map(|path| -> eyre::Result<[u8; 16]> {
                        let mut hasher = turboshake128();
                        sha2::digest::Update::update(&mut hasher, &read(path)?);
                        let mut reader = hasher.finalize_xof();
                        let mut hash = [0u8; 16];
                        reader.read(&mut hash);
                        Ok(hash)
                    })
                    .collect::<eyre::Result<Vec<[u8; 16]>>>()?;
                for hash in results {
                    sha2::digest::Update::update(&mut sum_turboshake128, &hash);
                }
            }
            Algorithm::Blake3 => {
                let results = paths
                    .into_par_iter()
                    .map(|path| -> eyre::Result<[u8; 32]> {
                        Ok(blake3::hash(&read(path)?).as_bytes().to_owned())
                    })
                    .collect::<eyre::Result<Vec<[u8; 32]>>>()?;
                for hash in results {
                    sum_blake3.update(&hash);
                }
            }
            Algorithm::Blake3MmapRayon => {
                let results = paths
                    .into_par_iter()
                    .map(|path| -> eyre::Result<[u8; 32]> {
                        let mut hasher = blake3::Hasher::new();
                        hasher.update_mmap_rayon(path)?;
                        Ok(hasher.finalize().as_bytes().to_owned())
                    })
                    .collect::<eyre::Result<Vec<[u8; 32]>>>()?;
                for hash in results {
                    sum_blake3.update(&hash);
                }
            }
        }
    }
    match args.algorithm {
        Algorithm::SumPathsLen => {
            dbg!(sum_paths_len);
        }
        Algorithm::SumReadLen => {
            dbg!(sum_read_len);
        }
        Algorithm::Sha256 => {
            dbg!(sum_sha256.finalize());
        }
        Algorithm::Sha512 => {
            dbg!(sum_sha512.finalize());
        }
        Algorithm::Sha3_256 => {
            dbg!(sum_sha3_256.finalize());
        }
        Algorithm::TurboShake128 => {
            let mut reader = sum_turboshake128.finalize_xof();
            let mut hash = [0u8; 16];
            reader.read(&mut hash);
            dbg!(hash);
        }
        Algorithm::Blake3 | Algorithm::Blake3MmapRayon => {
            dbg!(sum_blake3.finalize());
        }
    }

    Ok(())
}

fn turboshake128() -> sha3::TurboShake128 {
    // “Users that do not require multiple instances can take as default D = 0x1F.”
    // <https://keccak.team/files/TurboSHAKE.pdf>
    let core = sha3::TurboShake128Core::new(0x1F);
    sha3::TurboShake128::from_core(core)
}
