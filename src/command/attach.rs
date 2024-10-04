use std::{fs::create_dir_all, path::Path};

use jane_eyre::eyre;
use tracing::info;

use crate::{
    attachments::{AttachmentsContext, RealAttachmentsContext},
    migrations::run_migrations,
    path::AttachmentsPath,
};

pub async fn main(args: impl Iterator<Item = String>) -> eyre::Result<()> {
    run_migrations()?;
    create_dir_all(&*AttachmentsPath::ROOT)?;

    for path in args {
        let attachment_path = RealAttachmentsContext.store(&Path::new(&path))?;
        info!(
            "created attachment: <{}>",
            attachment_path.site_path()?.base_relative_url()
        );
    }

    Ok(())
}
