use std::{fs::File, io::Write};

use askama::Template;
use jane_eyre::eyre::{self, OptionExt};
use serde::Deserialize;
use tracing::{info, warn};

use crate::{path::PostsPath, Author, PostMeta};

#[derive(clap::Args, Debug)]
pub struct Fedi2autost {
    pub path_to_json: String,
    pub path_to_posts: String,
}

pub fn main(args: Fedi2autost) -> eyre::Result<()> {
    let json = File::open(&args.path_to_json)?;
    let json: Outbox = serde_json::from_reader(json)?;

    let output_dir = PostsPath::from_site_root_relative_path(&args.path_to_posts)?;
    let write_note = |note: &Note| -> eyre::Result<()> {
        let (_, id) = note.id.rsplit_once("/").ok_or_eyre("id has no slashes")?;
        let output_path = output_dir.join(&format!("{id}.html"))?;
        info!(?output_path, "writing post");
        let tags = note
            .tag
            .iter()
            .flat_map(|tag| match tag {
                Tag::Hashtag { name } => name.strip_prefix("#"),
                Tag::Other => {
                    warn!("");
                    None
                }
            })
            .map(|tag| tag.to_owned());
        let meta = PostMeta {
            archived: Some(note.url.to_owned()),
            references: vec![], // TODO
            title: None,
            published: Some(note.published.to_owned()),
            author: Some(Author {
                href: note.attributedTo.to_owned(),
                name: note.attributedTo.to_owned(),         // TODO
                display_name: note.attributedTo.to_owned(), // TODO
                display_handle: note.attributedTo.to_owned(), // TODO
            }), // TODO
            tags: tags.collect(),        // TODO
            is_transparent_share: false, // TODO
        };

        let mut output = File::create(output_path)?;
        output.write_all(meta.render()?.as_bytes())?;
        output.write_all(b"\n\n")?;
        output.write_all(note.content.as_bytes())?;
        output.write_all(b"\n")?;

        Ok(())
    };

    for item in json.orderedItems.iter() {
        match item {
            Item::Create { object } => match object {
                Object::String(_) => warn!(""),
                Object::Other(other) => match other {
                    OtherObject::Note(note) => write_note(note)?,
                    OtherObject::Other => warn!(""),
                },
            },
            Item::Other => warn!(""),
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct Outbox {
    orderedItems: Vec<Item>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Item {
    Create {
        object: Object,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Object {
    String(String),
    Other(OtherObject),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OtherObject {
    Note(Note),
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct Note {
    id: String,
    url: String,
    published: String,
    attributedTo: String,
    tag: Vec<Tag>,
    content: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(non_snake_case)]
enum Tag {
    Hashtag {
        name: String,
    },
    #[serde(other)]
    Other,
}
