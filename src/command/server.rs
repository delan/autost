use std::{
    fs::File,
    io::{self, Write as _},
};

use crate::{
    command::render::render_all,
    output::ThreadsContentTemplate,
    path::{PostsPath, POSTS_PATH_ROOT},
    render_markdown,
    rocket_eyre::{self, EyreReport},
    Command, PostMeta, TemplatedPost, Thread, SETTINGS,
};

use askama_rocket::Template;
use chrono::{SecondsFormat, Utc};
use clap::Parser as _;
use jane_eyre::eyre::{Context, OptionExt as _};
use rocket::{
    form::Form,
    fs::{FileServer, Options},
    get, post,
    response::{content, Redirect},
    routes, Config, FromForm, Responder,
};

#[derive(clap::Args, Debug)]
pub struct Server {
    #[arg(short, long)]
    port: Option<u16>,
}
#[derive(askama_rocket::Template)]
#[template(path = "compose.html")]
struct ComposeTemplate {
    source: String,
}
#[get("/compose?<reply_to>&<tags>&<is_transparent_share>")]
fn compose_route(
    reply_to: Option<String>,
    tags: Vec<String>,
    is_transparent_share: Option<bool>,
) -> rocket_eyre::Result<ComposeTemplate> {
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let references = if let Some(reply_to) = reply_to {
        let reply_to = POSTS_PATH_ROOT
            .join(&reply_to)
            .map_err(EyreReport::BadRequest)?;
        let post = TemplatedPost::load(&reply_to)?;
        let thread = Thread::try_from(post)?;
        thread.posts.into_iter().filter_map(|x| x.path).collect()
    } else {
        vec![]
    };
    let is_transparent_share = is_transparent_share.unwrap_or_default();

    let meta = PostMeta {
        archived: None,
        references,
        title: (!is_transparent_share).then_some("headline".to_owned()),
        published: Some(now),
        author: SETTINGS.self_author.clone(),
        tags,
        is_transparent_share,
    };
    let meta = meta.render().wrap_err("failed to render template")?;

    let source = if is_transparent_share {
        meta
    } else {
        format!("{meta}\n\npost body (accepts markdown!)")
    };

    Ok(ComposeTemplate { source })
}

#[derive(FromForm, Debug)]
struct Body<'r> {
    source: &'r str,
}

#[post("/preview", data = "<body>")]
fn preview_route(body: Form<Body<'_>>) -> rocket_eyre::Result<content::RawHtml<String>> {
    let unsafe_source = body.source;
    let unsafe_html = render_markdown(unsafe_source);
    let post = TemplatedPost::filter(&unsafe_html, None)?;
    let thread = Thread::try_from(post)?;
    Ok(content::RawHtml(
        ThreadsContentTemplate::render_normal(&thread).wrap_err("failed to render template")?,
    ))
}

#[derive(Responder)]
enum PublishResponse {
    Redirect(Box<Redirect>),
    Text(String),
}

#[post("/publish?<js>", data = "<body>")]
fn publish_route(js: Option<bool>, body: Form<Body<'_>>) -> rocket_eyre::Result<PublishResponse> {
    let js = js.unwrap_or_default();
    let unsafe_source = body.source;

    // try rendering the post before writing it, to catch any errors.
    let unsafe_html = render_markdown(unsafe_source);
    let post = TemplatedPost::filter(&unsafe_html, None)?;
    let _thread = Thread::try_from(post)?;

    // cohost post ids are all less than 10000000.
    let (mut file, path) = (10_000_000..)
        .map(|id| {
            let path = PostsPath::markdown_post_path(id);
            File::create_new(&path).map(|file| (file, path))
        })
        .find(|file| !matches!(file, Err(error) if error.kind() == io::ErrorKind::AlreadyExists))
        .expect("too many posts :(")
        .wrap_err("failed to create post")?;

    file.write_all(unsafe_source.as_bytes())
        .wrap_err("failed to write post file")?;
    render_all()?;

    let post = TemplatedPost::load(&path)?;
    let _thread = Thread::try_from(post)?;
    let url = path
        .rendered_path()?
        .ok_or_eyre("path has no rendered path")?
        .internal_url();

    // fetch api does not expose the redirect ‘location’ to scripts.
    // <https://github.com/whatwg/fetch/issues/763>
    if js {
        Ok(PublishResponse::Text(url))
    } else {
        Ok(PublishResponse::Redirect(Box::new(Redirect::to(url))))
    }
}

// lower than FileServer, which uses rank 10 by default
#[get("/", rank = 100)]
fn root_route() -> Redirect {
    Redirect::to(&SETTINGS.base_url)
}

/// - site routes (all under `base_url`)
///   - `GET <base_url>compose` (`compose_route`)
///     - `?reply_to=<PostsPath>` (optional; zero or one)
///     - `?tags=<str>` (optional; any number of times)
///     - `?is_transparent_share` (optional)
///   - `POST <base_url>preview` (`preview_route`)
///   - `POST <base_url>publish` (`publish_route`)
///   - `GET <base_url><path>` (`static_route`)
/// - `GET /` (`root_route`)
#[rocket::main]
pub async fn main() -> jane_eyre::eyre::Result<()> {
    let Command::Server(args) = Command::parse() else {
        unreachable!("guaranteed by subcommand call in entry point")
    };

    render_all()?;

    let port = args.port.unwrap_or(SETTINGS.server_port());
    let _rocket = rocket::custom(
        Config::figment()
            .merge(("port", port))
            .merge(("address", "::1")),
    )
    .mount(
        &SETTINGS.base_url,
        routes![compose_route, preview_route, publish_route],
    )
    .mount("/", routes![root_route])
    // serve attachments out of main attachment store, in case we need to preview a post
    // that refers to an attachment for the first time. otherwise they will 404, since
    // render won’t have hard-linked it into the site output dir.
    .mount(
        format!("{}attachments/", SETTINGS.base_url),
        FileServer::new(
            "./attachments",
            // DotFiles because attachment filenames can start with `.`
            // NormalizeDirs because relative links rely on directories ending with a `/`
            Options::Index | Options::DotFiles | Options::NormalizeDirs,
        )
        .rank(9),
    )
    // serve all other files out of `SITE_PATH_ROOT`.
    .mount(
        &SETTINGS.base_url,
        FileServer::new(
            "./site",
            // DotFiles because attachment filenames can start with `.`
            // NormalizeDirs because relative links rely on directories ending with a `/`
            Options::Index | Options::DotFiles | Options::NormalizeDirs,
        )
        .rank(10),
    )
    .launch()
    .await;

    Ok(())
}
