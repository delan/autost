use std::{fs::create_dir_all, path::Path};

use clap::Parser as _;
use jane_eyre::eyre;
use tracing::info;

use crate::{
    attachments::{AttachmentsContext, RealAttachmentsContext},
    migrations::run_migrations,
    path::ATTACHMENTS_PATH_ROOT,
    Command,
};

#[derive(clap::Args, Debug)]
pub struct Attach {
    paths: Vec<String>,
}

#[tokio::main]
pub async fn main() -> eyre::Result<()> {
    let Command::Attach(args) = Command::parse() else {
        unreachable!("guaranteed by subcommand call in entry point")
    };
    run_migrations()?;
    create_dir_all(&*ATTACHMENTS_PATH_ROOT)?;

    for path in args.paths {
        let attachment_path = RealAttachmentsContext.store(Path::new(&path))?;
        info!(
            "created attachment: <{}>",
            attachment_path.site_path()?.base_relative_url()
        );
    }

    Ok(())
}
