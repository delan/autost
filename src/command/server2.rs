use crate::{
    command::server::Server, path::POSTS_PATH_ROOT, PostMeta, TemplatedPost, Thread, SETTINGS,
};

use askama_rocket::Template;
use chrono::{SecondsFormat, Utc};
use jane_eyre::eyre::Context;
use rocket::{
    fs::{FileServer, Options},
    get,
    response::Redirect,
    routes, Config,
};
use rocket_errors::eyre;

#[derive(askama_rocket::Template)]
#[template(path = "compose.html")]
struct ComposeTemplate {
    source: String,
}
// FIXME: Errors only reply with InternalError, not others like BadRequest
#[get("/compose?<reply_to>&<tags>&<is_transparent_share>")]
fn compose_route(
    reply_to: Option<String>,
    tags: Vec<String>,
    is_transparent_share: Option<bool>,
) -> eyre::Result<ComposeTemplate> {
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);

    let references = if let Some(reply_to) = reply_to {
        let reply_to = POSTS_PATH_ROOT.join(&reply_to)?;
        let post = TemplatedPost::load(&reply_to)?;
        let thread = Thread::try_from(post)?;
        thread.posts.into_iter().flat_map(|x| x.path).collect()
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
pub async fn main(args: Server) -> jane_eyre::eyre::Result<()> {
    let port = args.port.unwrap_or(SETTINGS.server_port());
    let _rocket = rocket::custom(Config::figment().merge(("port", port)))
        .mount(&SETTINGS.base_url, routes![compose_route])
        .mount("/", routes![root_route])
        .mount(
            &SETTINGS.base_url,
            FileServer::new(
                "./site",
                // DotFiles because attachments can start with a .
                // NormalizeDirs because relative links rely on folders ending with a "/"
                Options::Index | Options::DotFiles | Options::NormalizeDirs,
            ),
        )
        .launch()
        .await?;

    Ok(())
}
