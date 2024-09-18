use std::{
    collections::HashMap,
    fs::File,
    io::{BufRead, BufReader, Read},
};

use jane_eyre::eyre::{self, bail};
use serde::Deserialize;

use crate::{TemplatedPost, Thread};

#[derive(Deserialize)]
pub struct Settings {
    pub base_url: String,
    pub external_base_url: String,
    pub site_title: String,
    pub self_authors: Vec<String>,
    pub interesting_tags: Vec<String>,
    archived_thread_tags_path: Option<String>,
    pub archived_thread_tags: Option<HashMap<String, Vec<String>>>,
    pub interesting_output_filenames_list_path: Option<String>,
    interesting_archived_threads_list_path: Option<String>,
    interesting_archived_threads_list: Option<Vec<String>>,
    excluded_archived_threads_list_path: Option<String>,
    excluded_archived_threads_list: Option<Vec<String>>,
    pub nav: Vec<NavLink>,
}

#[derive(Deserialize)]
pub struct NavLink {
    pub href: String,
    pub text: String,
}

impl Settings {
    pub fn load() -> eyre::Result<Self> {
        let mut result = String::default();
        File::open("autost.toml")?.read_to_string(&mut result)?;
        let mut result: Settings = toml::from_str(&result)?;

        if !result.base_url.ends_with("/") {
            bail!("base_url setting must end with slash!");
        }
        if !result.external_base_url.ends_with("/") {
            bail!("external_base_url setting must end with slash!");
        }
        if let Some(path) = result.archived_thread_tags_path.as_ref() {
            let entries = BufReader::new(File::open(path)?)
                .lines()
                .collect::<Result<Vec<_>, _>>()?;
            let entries = entries
                .iter()
                .filter_map(|entry| entry.split_once(" "))
                .map(|(archived, tags)| (archived, tags.split(",")))
                .map(|(archived, tags)| {
                    (
                        archived.to_owned(),
                        tags.map(ToOwned::to_owned).collect::<Vec<_>>(),
                    )
                })
                .collect();
            result.archived_thread_tags = Some(entries);
        }
        if let Some(path) = result.interesting_archived_threads_list_path.as_ref() {
            let list = BufReader::new(File::open(path)?)
                .lines()
                .collect::<Result<Vec<_>, _>>()?;
            result.interesting_archived_threads_list = Some(list);
        }
        if let Some(path) = result.excluded_archived_threads_list_path.as_ref() {
            let list = BufReader::new(File::open(path)?)
                .lines()
                .collect::<Result<Vec<_>, _>>()?;
            result.excluded_archived_threads_list = Some(list);
        }

        Ok(result)
    }

    pub fn thread_is_on_interesting_archived_list(&self, thread: &Thread) -> bool {
        self.interesting_archived_threads_list
            .as_ref()
            .zip(thread.meta.archived.as_ref())
            .is_some_and(|(list, archived)| list.iter().any(|x| x == archived))
    }

    pub fn thread_is_on_excluded_archived_list(&self, thread: &Thread) -> bool {
        self.excluded_archived_threads_list
            .as_ref()
            .zip(thread.meta.archived.as_ref())
            .is_some_and(|(list, archived)| list.iter().any(|x| x == archived))
    }

    pub fn extra_archived_thread_tags(&self, post: &TemplatedPost) -> &[String] {
        self.archived_thread_tags
            .as_ref()
            .zip(post.meta.archived.as_ref())
            .and_then(|(tags, archived)| tags.get(archived))
            .map(|result| &**result)
            .unwrap_or(&[])
    }
}
