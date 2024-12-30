use std::{
    env::{self},
    fs::{create_dir_all, File},
    path::Path,
    str,
    time::Duration,
};

use bytes::Bytes;
use jane_eyre::eyre::{self, bail, OptionExt};
use reqwest::{
    header::{self, HeaderMap, HeaderValue},
    Client, Response,
};
use scraper::{selector::Selector, Html};
use serde::de::DeserializeOwned;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::cohost::{
    LikedPostsState, ListEditedProjectsResponse, LoggedInResponse, Post, PostsResponse,
    TrpcResponse,
};

#[derive(clap::Args, Debug)]
pub struct Cohost2json {
    pub project_name: String,
    pub path_to_chosts: String,

    #[arg(long, help = "dump liked posts (requires COHOST_COOKIE)")]
    pub liked: bool,
}

pub async fn main(args: Cohost2json) -> eyre::Result<()> {
    let requested_project = args.project_name;
    let output_path = args.path_to_chosts;
    let output_path = Path::new(&output_path);
    let mut dump_liked = args.liked;
    create_dir_all(output_path)?;

    let client = if let Ok(connect_sid) = env::var("COHOST_COOKIE") {
        info!("COHOST_COOKIE is set; output will include private or logged-in-only chosts!");
        let mut cookie_value = HeaderValue::from_str(&format!("connect.sid={connect_sid}"))?;
        cookie_value.set_sensitive(true);
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie_value);
        let client = Client::builder().default_headers(headers).build()?;

        let edited_projects = get_json::<TrpcResponse<ListEditedProjectsResponse>>(
            &client,
            "https://cohost.org/api/v1/trpc/projects.listEditedProjects",
        )
        .await?
        .result
        .data
        .projects;
        let logged_in_project_id = get_json::<TrpcResponse<LoggedInResponse>>(
            &client,
            "https://cohost.org/api/v1/trpc/login.loggedIn",
        )
        .await?
        .result
        .data
        .projectId;
        let logged_in_project = edited_projects
            .iter()
            .find(|project| project.projectId == logged_in_project_id)
            .ok_or_eyre("you seem to be logged in as a project you don’t own")?;
        info!(
            "you are currently logged in as @{}",
            logged_in_project.handle
        );

        if let Some(requested_project) = edited_projects
            .iter()
            .find(|project| project.handle == requested_project)
        {
            if requested_project.projectId != logged_in_project_id {
                bail!(
                    "you wanted to dump chosts for @{}, but you are logged in as @{}",
                    requested_project.handle,
                    logged_in_project.handle,
                );
            } else {
                info!(
                    "dumping chosts for @{}, which you own and are logged in as",
                    requested_project.handle
                );
            }
        } else {
            info!(
                "dumping chosts for @{}, which you don’t own",
                requested_project
            );
            if dump_liked {
                warn!(
                    "you requested liked chosts, but not your own logged in project (@{}); skipping liked chosts",
                    logged_in_project.handle
                );
                dump_liked = false;
            }
        }

        client
    } else {
        info!("COHOST_COOKIE not set; output will exclude private or logged-in-only chosts!");
        Client::builder().build()?
    };

    for page in 0.. {
        let url =
            format!("https://cohost.org/api/v1/project/{requested_project}/posts?page={page}");
        let response: PostsResponse = get_json(&client, &url).await?;

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

    if dump_liked {
        if env::var("COHOST_COOKIE").is_err() {
            warn!("requested liked posts, but COHOST_COOKIE not provided - skipping");
        } else {
            info!("dumping liked chosts for @{}", requested_project);
            for liked_page in 0.. {
                let url = format!(
                    "https://cohost.org/rc/liked-posts?skipPosts={}",
                    liked_page * 20
                );

                let response = get_text(&client, &url).await?;
                let document = Html::parse_document(&response);

                let selector = Selector::parse("script#__COHOST_LOADER_STATE__")
                    .expect("guaranteed by argument");
                let node = document
                    .select(&selector)
                    .next()
                    .ok_or_eyre("failed to find script#__COHOST_LOADER_STATE__")?;
                let texts = node.text().collect::<Vec<_>>();
                let (text, rest) = texts
                    .split_first()
                    .ok_or_eyre("script element has no text nodes")?;
                if !rest.is_empty() {
                    error!("script element has more than one text node");
                }

                let liked_store = serde_json::from_str::<LikedPostsState>(text)?.liked_posts_feed;

                if !liked_store.paginationMode.morePagesForward {
                    break;
                }

                for post in liked_store.posts {
                    let path = output_path.join(format!("{}.json", post.postId));
                    info!("Writing {path:?}");
                    let output_file = File::create(path)?;
                    serde_json::to_writer(output_file, &post)?;
                }
            }
        }
    }

    Ok(())
}

async fn get_text(client: &Client, url: &str) -> eyre::Result<String> {
    get_with_retries(client, url, text).await
}

async fn get_json<T: DeserializeOwned>(client: &Client, url: &str) -> eyre::Result<T> {
    get_with_retries(client, url, json).await
}

async fn get_with_retries<T>(
    client: &Client,
    url: &str,
    mut and_then: impl FnMut(Bytes) -> eyre::Result<T>,
) -> eyre::Result<T> {
    let mut retries = 4;
    let mut wait = Duration::from_secs(4);
    loop {
        let result = get_response_once(client, url).await;
        let status = result
            .as_ref()
            .map_or(None, |response| Some(response.status()));
        let result = match match result {
            Ok(response) => Ok(response.bytes().await),
            Err(error) => Err(error),
        } {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(error)) | Err(error) => Err::<Bytes, eyre::Report>(error.into()),
        };
        // retry requests if they are neither client errors (http 4xx), nor if they are successful
        // (http 2xx) and the given fallible transformation fails. this includes server errors
        // (http 5xx), and requests that failed in a way that yields no response.
        let error = if status.is_some_and(|s| s.is_client_error()) {
            // client errors (http 4xx) should not be retried.
            bail!("GET request failed (no retries): http {:?}: {url}", status);
        } else if status.is_some_and(|s| s.is_success()) {
            // apply the given fallible transformation to the response body.
            // if that succeeds, we succeed, otherwise we retry.
            let result = result.and_then(&mut and_then);
            if result.is_ok() {
                return result;
            }
            result.err()
        } else {
            // when retrying server errors (http 5xx), error is None.
            // when retrying failures with no response, error is Some.
            result.err()
        };
        if retries == 0 {
            bail!(
                "GET request failed (after retries): http {:?}: {url}",
                status,
            );
        }
        warn!(?wait, ?status, url, ?error, "retrying failed GET request");
        sleep(wait).await;
        wait *= 2;
        retries -= 1;
    }
}

async fn get_response_once(client: &Client, url: &str) -> reqwest::Result<Response> {
    info!("GET {url}");
    client.get(url).send().await
}

fn text(body: Bytes) -> eyre::Result<String> {
    Ok(str::from_utf8(&body)?.to_owned())
}

fn json<T: DeserializeOwned>(body: Bytes) -> eyre::Result<T> {
    Ok(serde_json::from_slice(&body)?)
}
