use std::{fs::File, io::Read};

use jane_eyre::eyre::{self, bail};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Settings {
    pub base_url: String,
    pub site_title: String,
    pub interesting_tags: Vec<String>,
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
        let result: Settings = toml::from_str(&result)?;

        if !result.base_url.ends_with("/") {
            bail!("base_url setting must end with slash!");
        }

        Ok(result)
    }
}
