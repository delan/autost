use std::{
    fs::{create_dir_all, read_dir, File},
    io::{Read, Write},
};

use jane_eyre::eyre::{self, bail};
use reqwest::redirect::Policy;
use tracing::{debug, trace};

use crate::{
    cohost::attachment_id_to_url,
    path::{AttachmentsPath, SitePath},
};

pub trait AttachmentsContext {
    fn cache_cohost_file(&self, id: &str) -> eyre::Result<SitePath>;
    fn cache_cohost_thumb(&self, id: &str) -> eyre::Result<SitePath>;
}

pub struct RealAttachmentsContext;
impl AttachmentsContext for RealAttachmentsContext {
    #[tracing::instrument(skip(self))]
    fn cache_cohost_file(&self, id: &str) -> eyre::Result<SitePath> {
        let url = attachment_id_to_url(id);
        let dir = &*AttachmentsPath::ROOT;
        let path = dir.join(id)?;
        create_dir_all(&path)?;
        cache_cohost_attachment(&url, &path, None)?;
        let attachments_path = cached_attachment_url(id, dir)?;

        attachments_path.site_path()
    }

    #[tracing::instrument(skip(self))]
    fn cache_cohost_thumb(&self, id: &str) -> eyre::Result<SitePath> {
        fn thumb(url: &str) -> String {
            format!("{url}?width=675")
        }

        let url = attachment_id_to_url(id);
        let dir = &*AttachmentsPath::THUMBS;
        let path = dir.join(id)?;
        create_dir_all(&path)?;
        cache_cohost_attachment(&url, &path, Some(thumb))?;
        let attachments_path = cached_attachment_url(id, dir)?;

        attachments_path.site_path()
    }
}

fn cached_attachment_url(id: &str, dir: &AttachmentsPath) -> eyre::Result<AttachmentsPath> {
    let path = dir.join(id)?;
    let mut entries = read_dir(&path)?;
    let Some(entry) = entries.next() else {
        bail!("directory is empty: {path:?}");
    };

    Ok(path.join_dir_entry(&entry?)?)
}

/// given a cohost attachment redirect (`url`) and path to a uuid dir (`path`),
/// return the cached attachment path (`path/original-filename.ext`).
///
/// on cache miss, download the attachment from `url`, after first resolving the
/// redirect and transforming the resultant url (`transform_redirect_target`).
fn cache_cohost_attachment(
    url: &str,
    path: &AttachmentsPath,
    transform_redirect_target: Option<fn(&str) -> String>,
) -> eyre::Result<AttachmentsPath> {
    // if the attachment id directory exists...
    if let Ok(mut entries) = read_dir(path) {
        // and the directory contains a file...
        if let Some(entry) = entries.next() {
            // and we can open the file...
            // TODO: move this logic into path module
            let path = path.join_dir_entry(&entry?)?;
            if let Ok(mut file) = File::open(&path) {
                trace!("cache hit: {url}");
                // check if we can read the file.
                let mut result = Vec::default();
                file.read_to_end(&mut result)?;
                return Ok(path);
            }
        }
    }

    trace!("cache miss: {url}");
    debug!("downloading attachment");

    let client = reqwest::blocking::Client::builder()
        .redirect(Policy::none())
        .build()?;
    let redirect = client.head(url).send()?;

    let Some(url) = redirect.headers().get("location") else {
        bail!("expected redirect but got {}: {url}", redirect.status());
    };
    let url = url.to_str()?;

    let Some((_, original_filename)) = url.rsplit_once("/") else {
        bail!("redirect target has no slashes: {url}");
    };
    let original_filename = urlencoding::decode(original_filename)?;
    trace!("original filename: {original_filename}");

    // cohost attachment redirects don’t preserve query params, so if we want to add any,
    // we need to add them to the destination of the redirect.
    // FIXME: this will silently misbehave if the endpoint introduces a second redirect!
    let url = if let Some(transform) = transform_redirect_target {
        let transformed_url = transform(url);
        trace!("transformed redirect target: {transformed_url}");
        transformed_url
    } else {
        url.to_owned()
    };

    let path = path.join(original_filename.as_ref())?;
    let result = reqwest::blocking::get(url)?.bytes()?.to_vec();
    File::create(&path)?.write_all(&result)?;

    Ok(path)
}
