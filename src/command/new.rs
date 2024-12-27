use std::{
    fs::{create_dir_all, read_dir, File},
    io::Write,
    path::Path,
};

use jane_eyre::eyre::{self, bail};
use tracing::info;

#[derive(clap::Args, Debug)]
pub struct New {
    path: Option<String>,
}

pub fn main(args: New) -> eyre::Result<()> {
    let path = args.path.unwrap_or(".".to_owned());
    let path = Path::new(&path);
    info!("creating new site in {path:?}");

    create_dir_all(path)?;
    for entry in read_dir(path)? {
        bail!("directory is not empty: {:?}", entry?.path());
    }
    let mut settings = File::create_new(path.join("autost.toml"))?;
    settings.write_all(include_bytes!("../../autost.toml.example"))?;

    Ok(())
}
