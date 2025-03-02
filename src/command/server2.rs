use crate::{command::server::Server, SETTINGS};

use jane_eyre::eyre;
use rocket::{
    fs::{FileServer, Options},
    get,
    response::Redirect,
    routes, Config,
};

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
pub async fn main(args: Server) -> eyre::Result<()> {
    let port = args.port.unwrap_or(SETTINGS.server_port());
    let _rocket = rocket::custom(Config::figment().merge(("port", port)))
        .mount(
            &SETTINGS.base_url,
            FileServer::new(
                "./site",
                // DotFiles because attachments can start with a .
                // NormalizeDirs because relative links rely on folders ending with a "/"
                Options::Index | Options::DotFiles | Options::NormalizeDirs,
            ),
        )
        .mount("/", routes![root_route])
        .launch()
        .await?;

    Ok(())
}
