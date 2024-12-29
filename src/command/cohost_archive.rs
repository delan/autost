use std::{
    env::{self, set_current_dir},
    fs::{create_dir_all, exists, File},
    io::Write,
    path::Path,
};

use jane_eyre::eyre::{self, Context, OptionExt};
use reqwest::{
    header::{self, HeaderMap, HeaderValue},
    Client,
};
use tracing::{info, warn};

use crate::{
    cohost::{FollowedFeedResponse, ListEditedProjectsResponse, LoggedInResponse, TrpcResponse},
    command::{cohost2autost::Cohost2autost, cohost2json::Cohost2json},
};

#[derive(clap::Args, Debug)]
pub struct CohostArchive {
    output_path: String,
    project_names: Vec<String>,

    #[arg(long, help = "archive your liked posts")]
    liked: bool,
}

pub async fn main(args: CohostArchive) -> eyre::Result<()> {
    create_dir_all(&args.output_path)?;
    set_current_dir(args.output_path)?;

    let connect_sid = env::var("COHOST_COOKIE").wrap_err("failed to get COHOST_COOKIE")?;
    info!("COHOST_COOKIE is set; output will include private or logged-in-only chosts!");
    let mut cookie_value = HeaderValue::from_str(&format!("connect.sid={connect_sid}"))?;
    cookie_value.set_sensitive(true);
    let mut headers = HeaderMap::new();
    headers.insert(header::COOKIE, cookie_value);
    let client = Client::builder().default_headers(headers).build()?;

    info!("GET https://cohost.org/api/v1/trpc/projects.listEditedProjects");
    let edited_projects = client
        .get("https://cohost.org/api/v1/trpc/projects.listEditedProjects")
        .send()
        .await?
        .json::<TrpcResponse<ListEditedProjectsResponse>>()
        .await?
        .result
        .data
        .projects;
    info!("GET https://cohost.org/api/v1/trpc/login.loggedIn");
    let logged_in_project_id = client
        .get("https://cohost.org/api/v1/trpc/login.loggedIn")
        .send()
        .await?
        .json::<TrpcResponse<LoggedInResponse>>()
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

    let project_names = if args.project_names.is_empty() {
        info!("GET https://cohost.org/api/v1/trpc/projects.followedFeed.query?input=%7B%22sortOrder%22:%22followed-asc%22,%22limit%22:1000,%22beforeTimestamp%22:1735199148430%7D");
        let followed_feed = client
            .get("https://cohost.org/api/v1/trpc/projects.followedFeed.query?input=%7B%22sortOrder%22:%22followed-asc%22,%22limit%22:1000,%22beforeTimestamp%22:1735199148430%7D")
            .send()
            .await?
            .json::<TrpcResponse<FollowedFeedResponse>>()
            .await?
            .result
            .data;
        assert_eq!(
            followed_feed.nextCursor, None,
            "too many follows (needs pagination)"
        );
        let mut handles = followed_feed
            .projects
            .into_iter()
            .map(|p| p.project.handle)
            .collect::<Vec<_>>();
        handles.sort();
        handles.insert(0, logged_in_project.handle.clone());
        handles
    } else {
        args.project_names
    };
    info!(?project_names, "starting archive");

    let project_names = project_names
        .into_iter()
        .filter(|handle| {
            let is_edited_project = edited_projects.iter().any(|p| p.handle == *handle);
            let is_logged_in_project = logged_in_project.handle == *handle;
            if is_edited_project && !is_logged_in_project {
                warn!(
                    handle,
                    "skipping project that you edit but are not logged in as"
                );
            }
            is_edited_project == is_logged_in_project
        })
        .collect::<Vec<_>>();

    if args.liked && !project_names.contains(&logged_in_project.handle) {
        warn!("requested liked posts, but not the logged in project - skipping liked posts");
    }

    for project_name in project_names {
        // only try to archive likes for the logged-in project
        let archive_likes = args.liked && project_name == logged_in_project.handle;
        archive_cohost_project(&project_name, archive_likes).await?;
    }

    Ok(())
}

#[tracing::instrument(level = "error")]
async fn archive_cohost_project(project_name: &str, archive_likes: bool) -> eyre::Result<()> {
    info!("archiving");
    let project_path = Path::new(project_name);
    create_dir_all(project_path)?;
    set_current_dir(project_path)?;

    let mut autost_toml = File::create("autost.toml")?;
    writeln!(autost_toml, r#"base_url = "/""#)?;
    writeln!(autost_toml, r#"external_base_url = "https://example.com/""#)?;
    writeln!(autost_toml, r#"site_title = "@{project_name}""#)?;
    writeln!(autost_toml, r#"other_self_authors = []"#)?;
    writeln!(autost_toml, r#"interesting_tags = []"#)?;
    writeln!(autost_toml, r#"[self_author]"#)?;
    writeln!(autost_toml, r#"href = "https://cohost.org/{project_name}""#)?;
    writeln!(autost_toml, r#"name = """#)?;
    writeln!(autost_toml, r#"display_name = """#)?;
    writeln!(autost_toml, r#"display_handle = "@{project_name}""#)?;
    writeln!(autost_toml, r#"[[nav]]"#)?;
    writeln!(autost_toml, r#"href = ".""#)?;
    writeln!(autost_toml, r#"text = "posts""#)?;

    if !exists("cohost2json.done")? {
        info!("autost cohost2json {project_name} chosts");
        crate::command::cohost2json::main(Cohost2json {
            project_name: project_name.to_owned(),
            path_to_chosts: "chosts".to_owned(),
            liked: archive_likes,
        })
        .await?;
        File::create("cohost2json.done")?;
    }

    if !exists("cohost2autost.done")? {
        info!("autost cohost2autost chosts");
        crate::command::cohost2autost::main(Cohost2autost {
            path_to_chosts: "chosts".to_owned(),
            specific_chost_filenames: vec![],
        })?;
        File::create("cohost2autost.done")?;
    }

    set_current_dir("..")?;

    Ok(())
}
