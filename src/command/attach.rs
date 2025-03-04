use std::{fs::create_dir_all, path::Path};

use jane_eyre::eyre;
use tracing::info;

use crate::{
    attachments::{AttachmentsContext, RealAttachmentsContext},
    migrations::run_migrations,
    path::ATTACHMENTS_PATH_ROOT,
};

#[derive(clap::Args, Debug)]
pub struct Attach {
    paths: Vec<String>,
}

pub async fn main(args: Attach) -> eyre::Result<()> {
    run_migrations()?;
    create_dir_all(&*ATTACHMENTS_PATH_ROOT)?;

    for path in args.paths {
        let attachment_path = RealAttachmentsContext.store(&Path::new(&path))?;
        info!(
            "created attachment: <{}>",
            attachment_path.site_path()?.base_relative_url()
        );
    }

    Ok(())
}
