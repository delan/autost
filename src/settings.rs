use std::{
    collections::{BTreeSet, HashMap},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

use jane_eyre::eyre::{self, bail};
use serde::Deserialize;
use tracing::warn;

use crate::{path::parse_path_relative_scheme_less_url_string, Author, TemplatedPost, Thread};

#[derive(Deserialize)]
pub struct Settings {
    pub base_url: String,
    pub external_base_url: String,
    pub server_port: Option<u16>,
    pub site_title: String,
    pub other_self_authors: Vec<String>,
    pub interesting_tags: Vec<Vec<String>>,
    archived_thread_tags_path: Option<String>,
    pub archived_thread_tags: Option<HashMap<String, Vec<String>>>,
    pub interesting_output_filenames_list_path: Option<String>,
    interesting_archived_threads_list_path: Option<String>,
    interesting_archived_threads_list: Option<Vec<String>>,
    excluded_archived_threads_list_path: Option<String>,
    excluded_archived_threads_list: Option<Vec<String>>,
    pub self_author: Option<Author>,
    pub renamed_tags: Option<HashMap<String, String>>,
    pub implied_tags: Option<HashMap<String, Vec<String>>>,
    pub nav: Vec<NavLink>,

    #[deprecated(since = "0.3.0", note = "use path_to_static")]
    path_to_autost: Option<String>,
    path_to_static: Option<String>,
}

#[derive(Default, Deserialize)]
pub struct TagDefinition {
    pub rename: Option<String>,
    pub implies: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct NavLink {
    pub href: String,
    pub text: String,
}

impl Settings {
    pub fn load_default() -> eyre::Result<Self> {
        Self::load("autost.toml")
    }

    pub fn load_example() -> eyre::Result<Self> {
        Self::load("autost.toml.example")
    }

    pub fn load(path: impl AsRef<Path>) -> eyre::Result<Self> {
        let mut result = String::default();
        File::open(path)?.read_to_string(&mut result)?;
        let mut result: Self = toml::from_str(&result)?;

        if !result.base_url.starts_with('/') {
            bail!("base_url setting must start with slash!");
        }
        if result.base_url.starts_with("//") {
            bail!("base_url setting must not start with two slashes!");
        }
        if !result.base_url.ends_with('/') {
            bail!("base_url setting must end with slash!");
        }
        if !result.external_base_url.ends_with('/') {
            bail!("external_base_url setting must end with slash!");
        }
        if let Some(path) = result.archived_thread_tags_path.as_ref() {
            let entries = BufReader::new(File::open(path)?)
                .lines()
                .collect::<Result<Vec<_>, _>>()?;
            let entries = entries
                .iter()
                .filter_map(|entry| entry.split_once(' '))
                .map(|(archived, tags)| (archived, tags.split(',')))
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
        #[allow(deprecated)]
        if result.path_to_autost.is_some() {
            warn!("path_to_autost setting is deprecated; use path_to_static instead");
            if result.path_to_static.is_some() {
                bail!("path_to_autost and path_to_static settings are mutually exclusive");
            }
        }

        Ok(result)
    }

    pub fn base_url_path_components(&self) -> impl Iterator<Item = &str> {
        debug_assert_eq!(self.base_url.as_bytes()[0], b'/');
        debug_assert_eq!(self.base_url.as_bytes()[self.base_url.len() - 1], b'/');
        if self.base_url.len() > 1 {
            self.base_url[0..(self.base_url.len() - 1)]
                .split('/')
                .skip(1)
        } else {
            "".split('/').skip(1)
        }
    }

    #[must_use]
    pub fn base_url_relativise(&self, url: &str) -> String {
        parse_path_relative_scheme_less_url_string(url)
            .map_or_else(|| url.to_owned(), |url| format!("{}{}", self.base_url, url))
    }

    #[must_use]
    pub fn server_port(&self) -> u16 {
        self.server_port.unwrap_or(8420)
    }

    #[must_use]
    pub fn page_title(&self, title: Option<&str>) -> String {
        title.map_or_else(
            || self.site_title.clone(),
            |title| format!("{} — {}", title, self.site_title),
        )
    }

    #[must_use]
    pub fn is_main_self_author(&self, author: &Author) -> bool {
        // compare href only, ignoring other fields
        self.self_author
            .as_ref()
            .map_or(false, |a| a.href == author.href)
    }

    #[must_use]
    pub fn is_any_self_author(&self, author: &Author) -> bool {
        // compare href only, ignoring other fields
        self.is_main_self_author(author)
            || self.other_self_authors.iter().any(|a| *a == author.href)
    }

    #[must_use]
    pub fn tag_is_interesting(&self, tag: &str) -> bool {
        self.interesting_tags_iter()
            .any(|interesting_tag| interesting_tag == tag)
    }

    pub fn interesting_tags_iter(&self) -> impl Iterator<Item = &str> {
        self.interesting_tags.iter().flatten().map(|tag| &**tag)
    }

    pub fn interesting_tag_groups_iter(&self) -> impl Iterator<Item = &[String]> {
        self.interesting_tags.iter().map(|tag| &**tag)
    }

    #[must_use]
    pub fn thread_is_on_interesting_archived_list(&self, thread: &Thread) -> bool {
        self.interesting_archived_threads_list
            .as_ref()
            .zip(thread.meta.archived.as_ref())
            .is_some_and(|(list, archived)| list.iter().any(|x| x == archived))
    }

    #[must_use]
    pub fn thread_is_on_excluded_archived_list(&self, thread: &Thread) -> bool {
        self.excluded_archived_threads_list
            .as_ref()
            .zip(thread.meta.archived.as_ref())
            .is_some_and(|(list, archived)| list.iter().any(|x| x == archived))
    }

    #[must_use]
    pub fn extra_archived_thread_tags(&self, post: &TemplatedPost) -> &[String] {
        self.archived_thread_tags
            .as_ref()
            .zip(post.meta.archived.as_ref())
            .and_then(|(tags, archived)| tags.get(archived))
            .map_or(&[], |result| &**result)
    }

    #[must_use]
    pub fn resolve_tags(&self, tags: Vec<String>) -> Vec<String> {
        let mut seen = BTreeSet::default();
        let mut result = tags;
        let mut old_len = 0;

        // loop until we fail to add any more tags.
        while result.len() > old_len {
            let old = result;
            old_len = old.len();
            result = vec![];
            for tag in old {
                let tag = self.renamed_tag(tag);
                if seen.insert(tag.clone()) {
                    // prepend implied tags, such that more general tags go first.
                    result.extend(self.implied_tags_shallow(&tag).to_vec());
                }
                result.push(tag);
            }
        }

        let old = result;
        let mut result = vec![];
        for tag in old {
            if !result.contains(&tag) {
                result.push(tag);
            }
        }

        result
    }

    fn renamed_tag(&self, tag: String) -> String {
        if let Some(tags) = &self.renamed_tags {
            if let Some(result) = tags.get(&tag) {
                return result.clone();
            }
        }

        tag
    }

    fn implied_tags_shallow(&self, tag: &str) -> &[String] {
        if let Some(tags) = &self.implied_tags {
            if let Some(result) = tags.get(tag) {
                return result;
            }
        }

        &[]
    }

    #[must_use]
    pub fn path_to_static(&self) -> Option<PathBuf> {
        #[allow(deprecated)]
        if let Some(path_to_autost) = self.path_to_autost.as_deref() {
            return Some(Path::new(path_to_autost).join("static"));
        }
        if let Some(path_to_static) = self.path_to_static.as_deref() {
            return Some(path_to_static.into());
        }
        None
    }
}

#[test]
fn test_example() -> eyre::Result<()> {
    Settings::load_example()?;

    Ok(())
}

#[test]
fn test_resolve_tags() -> eyre::Result<()> {
    let mut settings = Settings::load_example()?;
    settings.renamed_tags = Some(
        [
            ("Foo".to_owned(), "foo".to_owned()),
            ("deep".to_owned(), "deep tag".to_owned()),
        ]
        .into_iter()
        .collect(),
    );
    settings.implied_tags = Some(
        [
            ("foo".to_owned(), vec!["bar".to_owned(), "baz".to_owned()]),
            ("bar".to_owned(), vec!["bar".to_owned(), "deep".to_owned()]),
            ("baz".to_owned(), vec!["foo".to_owned()]),
        ]
        .into_iter()
        .collect(),
    );
    // resolving tags means
    // - implied tags are prepended in order
    // - implied tags are resolved recursively, avoiding cycles
    // - duplicate tags are removed by keeping the first occurrence
    assert_eq!(
        settings.resolve_tags(vec!["Foo".to_owned()]),
        ["bar", "deep tag", "foo", "baz"]
    );

    Ok(())
}

#[test]
fn test_base_url_path_components() -> eyre::Result<()> {
    let mut settings = Settings::load_example()?;
    assert_eq!(
        settings.base_url_path_components().collect::<Vec<_>>(),
        Vec::<&str>::default()
    );

    "/posts/".clone_into(&mut settings.base_url);
    assert_eq!(
        settings.base_url_path_components().collect::<Vec<_>>(),
        ["posts"]
    );

    Ok(())
}
