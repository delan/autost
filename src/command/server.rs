use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{self, Read, Write},
    net::IpAddr,
    path::{Path, PathBuf},
    str::FromStr,
};

use askama::Template;
use chrono::{SecondsFormat, Utc};
use http::{Response, StatusCode, Uri};
use jane_eyre::eyre::{self, eyre, Context, OptionExt};
use tracing::{error, info, warn};
use warp::{
    filters::{any::any, path::Peek, reply::header},
    path,
    redirect::{see_other, temporary},
    reject::{custom, Reject, Rejection},
    reply::{self, Reply},
    Filter,
};

use crate::{output::ThreadsContentTemplate, path::AttachmentsPath, SETTINGS};
use crate::{
    path::{PostsPath, SitePath},
    render_markdown, PostMeta, TemplatedPost, Thread,
};

use crate::command::render::render_all;

#[derive(clap::Args, Debug)]
pub struct Server {
    #[arg(short, long)]
    port: Option<u16>,
}

static HTML: &'static str = "text/html; charset=utf-8";

/// - site routes (all under `base_url`)
///   - `GET <base_url>compose` (`compose_route`)
///   - `POST <base_url>preview` (`preview_route`)
///   - `POST <base_url>publish` (`publish_route`)
///   - `GET <base_url><path>` (`static_route`)
/// - `GET /` (`root_route`)
/// - `<METHOD> <path>` (`not_found_route`)
pub async fn main(args: Server) -> eyre::Result<()> {
    render_all()?;

    let compose_route = warp::path!("compose")
        .and(warp::filters::method::get())
        .and(warp::filters::query::query())
        .and_then(|query_vec: Vec<(String, String)>| async move {
            let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
            // convert query params from ordered pairs to map of lists
            let mut query = BTreeMap::<String, Vec<String>>::default();
            for (key, value) in query_vec {
                query.entry(key).or_default().push(value);
            }
            let references = if let Some(reply_to) = query.remove("reply_to") {
                let [ref reply_to] = reply_to[..] else {
                    return Err(Rejection::from(InternalError(eyre!(
                        "multiple reply_to query parameters not allowed"
                    ))));
                };
                let reply_to = PostsPath::ROOT.join(&reply_to).map_err(BadRequest)?;
                let post = TemplatedPost::load(&reply_to).map_err(InternalError)?;
                let thread = Thread::try_from(post).map_err(InternalError)?;
                thread
                    .posts
                    .into_iter()
                    .flat_map(|post| post.path)
                    .collect()
            } else {
                vec![]
            };
            let meta = PostMeta {
                archived: None,
                references,
                title: Some("headline".to_owned()),
                published: Some(now),
                author: SETTINGS.self_author.clone(),
                tags: query.remove("tags").unwrap_or_default(),
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
        .with(header("Content-Type", HTML))
        .recover(recover);

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
            let post = TemplatedPost::filter(&unsafe_html, None).map_err(InternalError)?;
            let thread = Thread::try_from(post).map_err(InternalError)?;
            let result = ThreadsContentTemplate::render_normal(&thread)
                .wrap_err("failed to render template")
                .map_err(InternalError)?;
            Ok::<_, Rejection>(result)
        })
        .with(header("Content-Type", HTML))
        .recover(recover);

    // POST /publish[?js] with urlencoded body: source=...
    let publish_route = warp::path!("publish")
        .and(warp::filters::method::post())
        .and(warp::filters::query::query())
        .and(warp::filters::body::form())
        .and_then(
            |query: HashMap<String, String>, mut form: HashMap<String, String>| async move {
                let unsafe_source = form
                    .remove("source")
                    .ok_or_eyre("form field missing: source")
                    .map_err(BadRequest)?;

                // try rendering the post before writing it, to catch any errors.
                let unsafe_html = render_markdown(&unsafe_source);
                let post = TemplatedPost::filter(&unsafe_html, None).map_err(InternalError)?;
                let _thread = Thread::try_from(post).map_err(InternalError)?;

                // cohost post ids are all less than 10000000.
                let (mut file, path) = (10000000..)
                    .map(|id| {
                        let path = PostsPath::markdown_post_path(id);
                        File::create_new(&path).map(|file| (file, path))
                    })
                    .filter(|file| !matches!(file, Err(error) if error.kind() == io::ErrorKind::AlreadyExists))
                    .next()
                    .expect("too many posts :(")
                    .wrap_err("failed to create post")
                    .map_err(InternalError)?;

                file.write_all(unsafe_source.as_bytes())
                    .wrap_err("failed to write post file")
                    .map_err(InternalError)?;
                render_all().map_err(InternalError)?;

                let post = TemplatedPost::load(&path).map_err(InternalError)?;
                let _thread = Thread::try_from(post).map_err(InternalError)?;
                let url = path.rendered_path()
                    .map_err(InternalError)?
                    .ok_or_eyre("path has no rendered path")
                    .map_err(InternalError)?
                    .internal_url();

                // fetch api does not expose the redirect ‘location’ to scripts.
                // <https://github.com/whatwg/fetch/issues/763>
                let response = if query.contains_key("js") {
                    Box::new(url) as Box<dyn Reply>
                } else {
                    let url = Uri::from_str(&url)
                        .wrap_err("failed to build Uri")
                        .map_err(InternalError)?;
                    Box::new(see_other(url)) as Box<dyn Reply>
                };

                Ok::<_, Rejection>(response)
            },
        )
        .with(header("Content-Type", HTML))
        .recover(recover);

    let static_route = warp::filters::method::get()
        .and(warp::filters::path::peek())
        .and_then(|peek: Peek| async move {
            let mut segments = peek.segments().peekable();
            // serve attachments out of main attachment store, in case we need to preview a post
            // that refers to an attachment for the first time. otherwise they will 404, since
            // render won’t have hard-linked it into the site output dir.
            let mut path: PathBuf = if segments.peek() == Some(&"attachments") {
                segments.next();
                (&*AttachmentsPath::ROOT).as_ref().to_owned()
            } else {
                (&*SitePath::ROOT).as_ref().to_owned()
            };
            for component in segments {
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
                            Some("js") => "text/javascript; charset=utf-8",
                            Some("mp3") => "audio/mpeg",
                            Some("mp4") => "video/mp4",
                            Some("png") => "image/png",
                            Some("svg") => "image/svg+xml",
                            Some("webp") => "image/webp",
                            Some("woff2") => "font/woff2",
                            Some("xml") => "text/xml",
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
        })
        .recover(recover);

    // prepend base_url to all site routes.
    let mut site_routes = any().boxed();
    for component in SETTINGS.base_url_path_components() {
        site_routes = site_routes.and(path(component)).boxed();
    }
    // successful responses are in their own types. error responses are in plain text.
    let site_routes = site_routes.and(
        compose_route
            .or(preview_route)
            .or(publish_route)
            .or(static_route),
    );

    // if the base_url setting is not /, redirect / to base_url.
    let root_route = warp::path!()
        .and(warp::filters::method::get())
        .and_then(|| async {
            let url = Uri::from_str(&SitePath::ROOT.internal_url())
                .wrap_err("failed to build Uri")
                .map_err(InternalError)?;
            Ok::<_, Rejection>(temporary(url))
        })
        .recover(recover);

    let not_found_route = any()
        .and(warp::filters::path::peek())
        .and_then(|peek: Peek| async move {
            Err::<Box<dyn Reply>, Rejection>(custom(NotFound(peek.as_str().to_owned())))
        })
        .recover(recover);

    let routes = site_routes.or(root_route).or(not_found_route);

    let port = args.port.unwrap_or(SETTINGS.server_port());
    info!("starting server on http://[::1]:{}", port);
    warp::serve(routes)
        .run(("::1".parse::<IpAddr>()?, port))
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

/// map all errors, other than errors due to none of the routes’ path and method filters matching,
/// from `Err(Rejection)` to `Ok(Reply)`, so we don’t try any other routes.
///
/// every route needs its own `.recover(recover)` for this to work correctly.
async fn recover(error: Rejection) -> Result<impl Reply, Rejection> {
    // if the error was due to none of the routes’ path and method filters matching, return that
    // error, allowing the default route to try serving a static file.
    if error.is_not_found() {
        return Err(error);
    }
    // for all other errors, convert Err(Rejection) to Ok(Reply), so we don’t try any other routes.
    Ok(if let Some(error) = error.find::<BadRequest>() {
        error!(
            ?error,
            "BadRequest: responding with http 400 bad request: {}", error.0,
        );
        reply::with_status(
            format!("bad request: {:?}", error.0),
            StatusCode::BAD_REQUEST,
        )
    } else if let Some(error) = error.find::<NotFound>() {
        error!(
            ?error,
            "NotFound: responding with http 404 not found: {}", error.0,
        );
        reply::with_status(format!("not found: {:?}", error.0), StatusCode::NOT_FOUND)
    } else if let Some(error) = error.find::<InternalError>() {
        error!(
            ?error,
            "InternalError: responding with http 500 internal server error: {}", error.0,
        );
        reply::with_status(
            format!("internal error: {:?}", error.0),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    } else {
        error!(
            ?error,
            "unknown error: responding with http 500 internal server error",
        );
        reply::with_status(
            format!("unknown error: {error:?}"),
            StatusCode::INTERNAL_SERVER_ERROR,
        )
    })
}
