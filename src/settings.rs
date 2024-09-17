use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
};

use jane_eyre::eyre::{self, bail};
use serde::Deserialize;

use crate::PostGroup;

#[derive(Deserialize)]
pub struct Settings {
    pub base_url: String,
    pub external_base_url: String,
    pub site_title: String,
    pub self_authors: Vec<String>,
    pub interesting_tags: Vec<String>,
    interesting_archived_post_groups_list_path: Option<String>,
    pub interesting_archived_post_groups_list: Option<Vec<String>>,
    excluded_archived_post_groups_list_path: Option<String>,
    pub excluded_archived_post_groups_list: Option<Vec<String>>,
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
        if let Some(path) = result.interesting_archived_post_groups_list_path.as_ref() {
            let list = BufReader::new(File::open(path)?)
                .lines()
                .collect::<Result<Vec<_>, _>>()?;
            result.interesting_archived_post_groups_list = Some(list);
        }
        if let Some(path) = result.excluded_archived_post_groups_list_path.as_ref() {
            let list = BufReader::new(File::open(path)?)
                .lines()
                .collect::<Result<Vec<_>, _>>()?;
            result.excluded_archived_post_groups_list = Some(list);
        }

        Ok(result)
    }

    pub fn post_group_is_on_interesting_archived_list(&self, post_group: &PostGroup) -> bool {
        self.interesting_archived_post_groups_list
            .as_ref()
            .zip(post_group.meta.archived.as_ref())
            .is_some_and(|(list, archived)| list.iter().any(|x| x == archived))
    }

    pub fn post_group_is_on_excluded_archived_list(&self, post_group: &PostGroup) -> bool {
        self.excluded_archived_post_groups_list
            .as_ref()
            .zip(post_group.meta.archived.as_ref())
            .is_some_and(|(list, archived)| list.iter().any(|x| x == archived))
    }
}
