use std::{env::args, fs::File, path::Path};

use autost::cohost::{Post, PostsResponse};
use jane_eyre::eyre;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn main() -> eyre::Result<()> {
    jane_eyre::install()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let project = args().nth(1).unwrap();
    let output_path = args().nth(2).unwrap();
    let output_path = Path::new(&output_path);

    for page in 0.. {
        let url = format!("https://cohost.org/api/v1/project/{project}/posts?page={page}");
        info!("GET {url}");
        let response: PostsResponse = reqwest::blocking::get(url)?.json()?;

        // nItems may be zero if none of the posts on this page are currently visible,
        // but nPages will only be zero when we have run out of pages.
        if response.nPages == 0 {
            break;
        }

        for post_value in response.items {
            let post: Post = serde_json::from_value(post_value.clone())?;
            let path = output_path.join(format!("{}.json", post.postId));
            info!("Writing {path:?}");
            let output_file = File::create(path)?;
            serde_json::to_writer(output_file, &post_value)?;
        }
    }

    Ok(())
}
