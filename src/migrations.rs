use std::{
    fs::{create_dir_all, hard_link, read_dir},
    io::ErrorKind,
};

use jane_eyre::eyre::{self, bail, Context};
use tracing::{info, trace};

use crate::path::SitePath;

#[tracing::instrument]
pub fn run_migrations() -> eyre::Result<()> {
    info!("hard linking attachments out of site/attachments");
    create_dir_all(&*SitePath::ATTACHMENTS)?;
    let mut dirs = vec![SitePath::ATTACHMENTS.to_owned()];
    let mut files: Vec<SitePath> = vec![];
    while !dirs.is_empty() || !files.is_empty() {
        for site_path in files.drain(..) {
            trace!("site path: {:?}", site_path);
            let Some(attachments_path) = site_path.attachments_path()? else {
                bail!("path is not an attachment path: {site_path:?}");
            };
            let Some(parent) = attachments_path.parent() else {
                bail!("path has no parent: {site_path:?}");
            };
            create_dir_all(parent)?;
            if let Err(error) = hard_link(site_path, attachments_path) {
                if error.kind() != ErrorKind::AlreadyExists {
                    Err(error).wrap_err("failed to create hard link")?;
                }
            }
        }
        if let Some(dir) = dirs.pop() {
            for entry in read_dir(&dir)? {
                let entry = entry?;
                let r#type = entry.file_type()?;
                let path = dir.join_dir_entry(&entry)?;
                if r#type.is_dir() {
                    dirs.push(path);
                } else if r#type.is_file() {
                    files.push(path);
                } else {
                    bail!(
                        "file in site/attachments with unexpected type: {:?}: {:?}",
                        r#type,
                        entry.path()
                    );
                }
            }
        }
    }

    Ok(())
}
