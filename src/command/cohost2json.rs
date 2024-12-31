use std::{
    env::{self},
    fs::{create_dir_all, File},
    io::Write,
    path::Path,
    str,
};

use jane_eyre::eyre::{self, bail, OptionExt};
use reqwest::{
    header::{self, HeaderMap, HeaderValue},
    Client,
};
use scraper::{selector::Selector, Html};
use tracing::{error, info, warn};

use crate::{
    cohost::{
        LikedPostsState, ListEditedProjectsResponse, LoggedInResponse, Post, PostsResponse,
        TrpcResponse,
    },
    http::{get_json, get_with_retries},
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

    let mut own_chosts = File::create(output_path.join("own_chosts.txt"))?;
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
            let filename = format!("{}.json", post.postId);
            let path = output_path.join(&filename);
            info!("Writing {path:?}");
            let output_file = File::create(path)?;
            serde_json::to_writer(output_file, &post_value)?;
            writeln!(own_chosts, "{filename}")?;
        }
    }

    if dump_liked {
        if env::var("COHOST_COOKIE").is_err() {
            warn!("requested liked posts, but COHOST_COOKIE not provided - skipping");
        } else {
            info!("dumping liked chosts for @{}", requested_project);
            let mut liked_chosts = File::create(output_path.join("liked_chosts.txt"))?;
            for liked_page in 0.. {
                let url = format!(
                    "https://cohost.org/rc/liked-posts?skipPosts={}",
                    liked_page * 20
                );

                let liked_store = get_with_retries(&client, &url, |body| {
                    let body = str::from_utf8(&body)?;
                    let document = Html::parse_document(body);
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
                    let liked_store =
                        serde_json::from_str::<LikedPostsState>(text)?.liked_posts_feed;
                    Ok(liked_store)
                })
                .await?;

                if !liked_store.paginationMode.morePagesForward {
                    break;
                }

                for post in liked_store.posts {
                    let filename = format!("{}.json", post.postId);
                    let path = output_path.join(&filename);
                    info!("Writing {path:?}");
                    let output_file = File::create(path)?;
                    serde_json::to_writer(output_file, &post)?;
                    writeln!(liked_chosts, "{filename}")?;
                }
            }
        }
    }

    Ok(())
}
