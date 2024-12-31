use std::{
    fs::{copy, create_dir_all, read_dir, File},
    io::{Read, Write},
    path::Path,
    thread::sleep,
    time::Duration,
};

use jane_eyre::eyre::{self, bail, OptionExt};
use reqwest::{redirect::Policy, StatusCode};
use sha2::{digest::generic_array::functional::FunctionalSequence, Digest, Sha256};
use tracing::{debug, error, trace, warn};
use uuid::Uuid;

use crate::{
    cohost::{attachment_id_to_url, Cacheable},
    path::{AttachmentsPath, SitePath},
};

#[derive(Debug)]
pub enum CachedFileResult<T> {
    CachedPath(T),
    UncachedUrl(String),
}

impl CachedFileResult<AttachmentsPath> {
    pub fn site_path(&self) -> eyre::Result<CachedFileResult<SitePath>> {
        Ok(match self {
            Self::CachedPath(inner) => CachedFileResult::CachedPath(inner.site_path()?),
            Self::UncachedUrl(url) => CachedFileResult::UncachedUrl(url.to_owned()),
        })
    }
}

impl CachedFileResult<SitePath> {
    pub fn base_relative_url(&self) -> String {
        match self {
            CachedFileResult::CachedPath(inner) => inner.base_relative_url(),
            CachedFileResult::UncachedUrl(url) => url.to_owned(),
        }
    }
}

pub trait AttachmentsContext {
    fn store(&self, input_path: &Path) -> eyre::Result<AttachmentsPath>;
    fn cache_imported(&self, url: &str, post_basename: &str) -> eyre::Result<AttachmentsPath>;
    fn cache_cohost_resource(
        &self,
        cacheable: &Cacheable,
    ) -> eyre::Result<CachedFileResult<AttachmentsPath>>;
    fn cache_cohost_thumb(&self, id: &str) -> eyre::Result<CachedFileResult<AttachmentsPath>>;
}

pub struct RealAttachmentsContext;
impl AttachmentsContext for RealAttachmentsContext {
    #[tracing::instrument(skip(self))]
    fn store(&self, input_path: &Path) -> eyre::Result<AttachmentsPath> {
        let dir = AttachmentsPath::ROOT.join(&Uuid::new_v4().to_string())?;
        create_dir_all(&dir)?;
        let filename = input_path.file_name().ok_or_eyre("no filename")?;
        let filename = filename.to_str().ok_or_eyre("unsupported filename")?;
        let path = dir.join(filename)?;
        copy(input_path, &path)?;

        Ok(path)
    }

    #[tracing::instrument(skip(self))]
    fn cache_imported(&self, url: &str, post_basename: &str) -> eyre::Result<AttachmentsPath> {
        let mut hash = Sha256::new();
        hash.update(url);
        let hash = hash.finalize().map(|o| format!("{o:02x}")).join("");
        let path = AttachmentsPath::ROOT.join(&format!("imported-{post_basename}-{hash}"))?;
        trace!(?path);
        create_dir_all(&path)?;

        cache_imported_attachment(url, &path)
    }

    #[tracing::instrument(skip(self))]
    fn cache_cohost_resource(
        &self,
        cacheable: &Cacheable,
    ) -> eyre::Result<CachedFileResult<AttachmentsPath>> {
        match cacheable {
            Cacheable::Attachment { id, url } => {
                let redirect_url = attachment_id_to_url(id);
                let dir = &*AttachmentsPath::ROOT;
                let path = dir.join(id)?;
                create_dir_all(&path)?;

                if cache_cohost_attachment(&redirect_url, &path, None)? {
                    Ok(CachedFileResult::CachedPath(cached_attachment_url(
                        id, dir,
                    )?))
                } else if let Some(original_url) = url {
                    Ok(CachedFileResult::UncachedUrl((*original_url).to_owned()))
                } else {
                    Ok(CachedFileResult::UncachedUrl(redirect_url))
                }
            }

            Cacheable::Static { filename, url } => {
                let dir = &*AttachmentsPath::COHOST_STATIC;
                create_dir_all(dir)?;
                let path = dir.join(filename)?;
                trace!(?path);

                cache_other_cohost_resource(url, &path).map(CachedFileResult::CachedPath)
            }

            Cacheable::Avatar { filename, url } => {
                let dir = &*AttachmentsPath::COHOST_AVATAR;
                create_dir_all(dir)?;
                let path = dir.join(filename)?;
                trace!(?path);

                cache_other_cohost_resource(url, &path).map(CachedFileResult::CachedPath)
            }

            Cacheable::Header { filename, url } => {
                let dir = &*AttachmentsPath::COHOST_HEADER;
                create_dir_all(dir)?;
                let path = dir.join(filename)?;
                trace!(?path);

                cache_other_cohost_resource(url, &path).map(CachedFileResult::CachedPath)
            }
        }
    }

    #[tracing::instrument(skip(self))]
    fn cache_cohost_thumb(&self, id: &str) -> eyre::Result<CachedFileResult<AttachmentsPath>> {
        fn thumb(url: &str) -> String {
            format!("{url}?width=675")
        }

        let redirect_url = attachment_id_to_url(id);
        let dir = &*AttachmentsPath::THUMBS;
        let path = dir.join(id)?;
        create_dir_all(&path)?;

        if cache_cohost_attachment(&redirect_url, &path, Some(thumb))? {
            Ok(CachedFileResult::CachedPath(cached_attachment_url(
                id, dir,
            )?))
        } else {
            Ok(CachedFileResult::UncachedUrl(redirect_url))
        }
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

fn cache_imported_attachment(url: &str, path: &AttachmentsPath) -> eyre::Result<AttachmentsPath> {
    // if the attachment id directory exists...
    if let Ok(mut entries) = read_dir(&path) {
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

    trace!("cache miss");
    debug!("downloading attachment");

    let response = reqwest::blocking::get(url)?;
    let extension = match response.headers().get("Content-Type") {
        Some(x) if x == "image/gif" => "gif",
        Some(x) if x == "image/jpeg" => "jpg",
        Some(x) if x == "image/png" => "png",
        Some(x) if x == "image/svg+xml" => "svg",
        Some(x) if x == "image/webp" => "webp",
        other => {
            warn!("unknown attachment mime type: {other:?}");
            "bin"
        }
    };
    let path = path.join(&format!("file.{extension}"))?;
    debug!(?path);

    let result = response.bytes()?.to_vec();
    File::create(&path)?.write_all(&result)?;

    Ok(path)
}

/// given a cohost attachment redirect (`url`) and path to a uuid dir (`path`),
/// return the cached attachment path (`path/original-filename.ext`).
///
/// on cache miss, download the attachment from `url`, after first resolving the
/// redirect and transforming the resultant url (`transform_redirect_target`).
///
/// returns true iff the attachment exists and was successfully retrieved or
/// stored in the attachment store.
fn cache_cohost_attachment(
    url: &str,
    path: &AttachmentsPath,
    transform_redirect_target: Option<fn(&str) -> String>,
) -> eyre::Result<bool> {
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
                return Ok(true);
            }
        }
    }

    trace!("cache miss: {url}");
    debug!("downloading attachment");

    let client = reqwest::blocking::Client::builder()
        .redirect(Policy::none())
        .build()?;

    let mut retries = 4;
    let mut wait = Duration::from_secs(4);
    let mut redirect;
    let url = loop {
        let result = client.head(url).send();
        match result {
            Ok(response) => redirect = response,
            Err(error) => {
                if retries == 0 {
                    bail!("failed to get attachment redirect (after retries): {url}: {error:?}");
                } else {
                    warn!(?wait, url, ?error, "retrying failed request");
                    sleep(wait);
                    wait *= 2;
                    retries -= 1;
                    continue;
                }
            }
        }
        let Some(url) = redirect.headers().get("location") else {
            // error without panicking if the chost refers to a 404 Not Found.
            // retry other requests if they are not client errors (http 4xx).
            // the attachment redirect endpoint occasionally returns 406 Not Acceptable,
            // so we retry those too.
            if redirect.status() == StatusCode::NOT_FOUND {
                error!(
                    "bogus attachment redirect: http {}: {url}",
                    redirect.status()
                );
                return Ok(false);
            } else if redirect.status().is_client_error()
                && redirect.status() != StatusCode::NOT_ACCEPTABLE
            {
                bail!(
                    "failed to get attachment redirect (no retries): http {}: {url}",
                    redirect.status()
                );
            } else if retries == 0 {
                bail!(
                    "failed to get attachment redirect (after retries): http {}: {url}",
                    redirect.status()
                );
            } else {
                warn!(?wait, url, status = ?redirect.status(), "retrying failed request");
                sleep(wait);
                wait *= 2;
                retries -= 1;
                continue;
            }
        };
        break url.to_str()?;
    };

    let Some((_, original_filename)) = url.rsplit_once("/") else {
        bail!("redirect target has no slashes: {url}");
    };
    let original_filename = urlencoding::decode(original_filename)?;

    // On Windows, `:` characters are not allowed in filenames (because it's used as a drive
    // separator)
    #[cfg(windows)]
    let original_filename = original_filename.replace(":", "-");

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

    Ok(true)
}

fn cache_other_cohost_resource(url: &str, path: &AttachmentsPath) -> eyre::Result<AttachmentsPath> {
    // if we can open the cached file...
    if let Ok(mut file) = File::open(path) {
        trace!("cache hit: {url}");
        // check if we can read the file.
        let mut result = Vec::default();
        file.read_to_end(&mut result)?;
        return Ok(path.clone());
    }

    trace!("cache miss");
    debug!("downloading resource");

    let response = reqwest::blocking::get(url)?;
    let result = response.bytes()?.to_vec();
    File::create(path)?.write_all(&result)?;

    Ok(path.clone())
}
