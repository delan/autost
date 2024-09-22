use std::{collections::HashMap, fs::File, io::Read, net::IpAddr, path::PathBuf};

use askama::Template;
use autost::{render_markdown, TemplatedPost};
use http::{Response, StatusCode};
use jane_eyre::eyre::{self, eyre, Context, OptionExt};
use tracing::error;
use warp::{
    filters::{path::Peek, reply::header},
    reject::{custom, Reject, Rejection},
    reply::{self, Reply},
    Filter,
};

static OCTET_STREAM: &'static str = "application/octet-stream";
static HTML: &'static str = "text/html; charset=utf-8";
static CSS: &'static str = "text/css; charset=utf-8";

pub async fn main(mut _args: impl Iterator<Item = String>) -> eyre::Result<()> {
    let home_route = warp::path!()
        .and(warp::filters::method::get())
        .and_then(|| async {
            || -> eyre::Result<String> { Ok(HomeTemplate::default().render()?) }()
                .map_err(|error| custom(InternalError(error)))
        })
        .with(header("Content-Type", HTML));

    // POST /preview with urlencoded body: source=...[&bare]
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
            if form.contains_key("bare") {
                return Ok(post.safe_html);
            }
            let result = HomeTemplate {
                source: unsafe_source.clone(),
                preview: post.safe_html,
            }
            .render()
            .wrap_err("failed to render template")
            .map_err(InternalError)?;
            Ok::<_, Rejection>(result)
        })
        .with(header("Content-Type", HTML));

    let default_route = warp::filters::method::get()
        .and(warp::filters::path::peek())
        .and_then(|peek: Peek| async move {
            let mut path = PathBuf::from("site");
            for component in peek.segments() {
                if component == ".." {
                    return Err(custom(BadRequest(eyre!("path component not allowed: .."))));
                } else if component == "." {
                    continue;
                }
                path.push(component);
            }

            let content_type = match path.extension().and_then(|x| x.to_str()) {
                Some("html") => HTML,
                Some("css") => CSS,
                _ => OCTET_STREAM,
            };

            let mut body = Vec::default();
            if let Ok(mut file) = File::open(path) {
                file.read_to_end(&mut body)
                    .wrap_err("failed to read file")
                    .map_err(InternalError)?;
            } else {
                return Err(custom(NotFound(peek.as_str().to_owned())));
            }

            let response = Response::builder()
                .header("Content-Type", content_type)
                .body(body)
                .wrap_err("failed to build response")
                .map_err(InternalError)?;

            Ok(response)
        });

    // successful responses are in their own types. error responses are in plain text.
    let routes = home_route.or(preview_route).or(default_route);
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

#[derive(Default, Template)]
#[template(path = "home.html")]
struct HomeTemplate {
    source: String,
    preview: String,
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
