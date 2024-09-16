use std::{fs::File, io::Read};

use jane_eyre::eyre::{self};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Settings {
    pub interesting_tags: Vec<String>,
}

impl Settings {
    pub fn load() -> eyre::Result<Self> {
        let mut result = String::default();
        File::open("autost.toml")?.read_to_string(&mut result)?;

        Ok(toml::from_str(&result)?)
    }
}
