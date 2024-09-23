use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
};

use askama::Template;
use autost::{render_markdown, PostMeta, TemplatedPost, Thread, ThreadsContentTemplate};
use chrono::{SecondsFormat, Utc};
use http::{Response, StatusCode};
use jane_eyre::eyre::{self, bail, eyre, Context, OptionExt};
use tracing::{error, warn};
use warp::{
    filters::{any::any, path::Peek, reply::header},
    path,
    reject::{custom, Reject, Rejection},
    reply::{self, Reply},
    Filter,
};

use autost::SETTINGS;

use crate::command::render::render_all;

static HTML: &'static str = "text/html; charset=utf-8";

pub async fn main(mut _args: impl Iterator<Item = String>) -> eyre::Result<()> {
    let compose_route = warp::path!("compose")
        .and(warp::filters::method::get())
        .and_then(|| async {
            let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            let meta = PostMeta {
                archived: None,
                references: vec![],
                title: Some("headline".to_owned()),
                published: Some(now),
                author: SETTINGS.self_author.clone(),
                tags: vec![],
                is_transparent_share: false,
            };
            let meta = meta
                .render()
                .wrap_err("failed to render template")
                .map_err(InternalError)?;
            let source = format!("{meta}\npost body (accepts markdown!)");
            let result = ComposeTemplate { source };
            let result = result
                .render()
                .wrap_err("failed to render template")
                .map_err(InternalError)?;
            Ok::<_, Rejection>(result)
        })
        .with(header("Content-Type", HTML));

    // POST /preview with urlencoded body: source=...
    let preview_route = warp::path!("preview")
        .and(warp::filters::method::post())
        .and(warp::filters::body::form())
        .and_then(|mut form: HashMap<String, String>| async move {
            let unsafe_source = form
                .remove("source")
                .ok_or_eyre("form field missing: source")
                .map_err(BadRequest)?;
            let unsafe_html = render_markdown(&unsafe_source);
            let post = TemplatedPost::filter(&unsafe_html, "").map_err(InternalError)?;
            let thread = Thread::try_from(post).map_err(InternalError)?;
            let template = ThreadsContentTemplate {
                threads: vec![thread],
            };
            let result = template
                .render()
                .wrap_err("failed to render template")
                .map_err(InternalError)?;
            Ok::<_, Rejection>(result)
        })
        .with(header("Content-Type", HTML));

    // POST /publish with urlencoded body: source=...
    let publish_route = warp::path!("publish")
        .and(warp::filters::method::post())
        .and(warp::filters::body::form())
        .and_then(|mut form: HashMap<String, String>| async move {
            fn create_post() -> eyre::Result<(File, String)> {
                // cohost post ids are all less than 10000000.
                let posts = PathBuf::from("posts");
                for id in 10000000.. {
                    let filename = format!("{id}.md");
                    match File::create_new(posts.join(&filename)) {
                        Ok(result) => return Ok((result, filename)),
                        Err(error) => match error.kind() {
                            std::io::ErrorKind::AlreadyExists => continue,
                            _ => bail!("failed to create post: {error}"),
                        },
                    }
                }

                unreachable!()
            }

            let unsafe_source = form
                .remove("source")
                .ok_or_eyre("form field missing: source")
                .map_err(BadRequest)?;
            let (mut file, filename) = create_post().map_err(InternalError)?;
            file.write_all(unsafe_source.as_bytes())
                .wrap_err("failed to write post file")
                .map_err(InternalError)?;
            render_all(Path::new("site")).map_err(InternalError)?;
            Ok::<_, Rejection>(filename)
        })
        .with(header("Content-Type", HTML));

    let default_route = warp::filters::method::get()
        .and(warp::filters::path::peek())
        .and_then(|peek: Peek| async move {
            let mut path = PathBuf::from("site");
            for component in peek.segments() {
                let component = urlencoding::decode(component)
                    .wrap_err("failed to decode url path component")
                    .map_err(BadRequest)?;
                if component == ".." {
                    return Err(custom(BadRequest(eyre!("path component not allowed: .."))));
                } else if component == "." {
                    continue;
                }
                path.push(&*component);
            }

            enum Error {
                Internal(eyre::Report),
                NotFound,
            }
            fn read_file_or_index(body: &mut Vec<u8>, path: &Path) -> Result<&'static str, Error> {
                if let Ok(mut file) = File::open(path) {
                    let metadata = file.metadata()
                        .wrap_err("failed to get file metadata")
                        .map_err(Error::Internal)?;
                    if metadata.is_dir() {
                        return read_file_or_index(body, &path.join("index.html"))
                    } else {
                        file.read_to_end(body)
                            .wrap_err("failed to read file")
                            .map_err(Error::Internal)?;
                        let extension = path.extension().and_then(|x| x.to_str());
                        let extension = extension.map(|x| x.to_ascii_lowercase());
                        let content_type = match extension.as_deref() {
                            Some("css") => "text/css; charset=utf-8",
                            Some("gif") => "image/gif",
                            Some("html") => HTML,
                            Some("jpg" | "jpeg") => "image/jpeg",
                            Some("png") => "image/png",
                            Some("webp") => "image/webp",
                            Some("woff2") => "font/woff2",
                            Some(other) => {
                                warn!("unknown file extension {other}; treating as application/octet-stream");
                                "application/octet-stream"
                            },
                            None => {
                                warn!("no file extension; treating as application/octet-stream");
                                "application/octet-stream"
                            },
                        };
                        return Ok(content_type);
                    }
                } else {
                    return Err(Error::NotFound);
                }
            }

            let mut body = Vec::default();
            let content_type = match read_file_or_index(&mut body, &path) {
                Ok(result) => Ok(result),
                Err(Error::Internal(error)) => Err(custom(InternalError(error))),
                Err(Error::NotFound) => Err(custom(NotFound(peek.as_str().to_owned()))),
            }?;

            let response = Response::builder()
                .header("Content-Type", content_type)
                .body(body)
                .wrap_err("failed to build response")
                .map_err(InternalError)?;

            Ok(response)
        });

    // successful responses are in their own types. error responses are in plain text.
    let mut routes = any().boxed();
    for component in SETTINGS.base_url_path_components() {
        routes = routes.and(path(component)).boxed();
    }
    let routes = routes.and(
        compose_route
            .or(preview_route)
            .or(publish_route)
            .or(default_route),
    );
    let routes = routes.recover(recover);

    warp::serve(routes)
        .run(("::1".parse::<IpAddr>()?, 8420))
        .await;

    Ok(())
}

#[derive(Debug)]
struct InternalError(eyre::Report);
impl Reject for InternalError {}
impl From<eyre::Report> for InternalError {
    fn from(value: eyre::Report) -> Self {
        Self(value)
    }
}

#[derive(Debug)]
struct BadRequest(eyre::Report);
impl Reject for BadRequest {}

#[derive(Debug)]
struct NotFound(String);
impl Reject for NotFound {}

#[derive(Template)]
#[template(path = "compose.html")]
struct ComposeTemplate {
    source: String,
}

async fn recover(error: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    Ok(if let Some(error) = error.find::<BadRequest>() {
        error!(
            "BadRequest: responding with http 400 bad request: {}",
            error.0
        );
        reply::with_status(format!("bad request: {}", error.0), StatusCode::BAD_REQUEST)
    } else if let Some(error) = error.find::<NotFound>() {
        error!("NotFound: responding with http 404 not found: {}", error.0);
        reply::with_status(format!("not found: {}", error.0), StatusCode::NOT_FOUND)
    } else if let Some(error) = error.find::<InternalError>() {
        error!(
            "InternalError: responding with http 500 internal server error: {}",
            error.0
        );
        reply::with_status(
            format!("internal error: {}", error.0),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    } else {
        error!(
            "unknown error: responding with http 500 internal server error: {:?}",
            error
        );
        reply::with_status(
            format!("unknown error: {error:?}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })
}
