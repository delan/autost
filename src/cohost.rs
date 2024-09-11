use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct PostsResponse {
    pub nItems: usize,
    pub nPages: usize,
    pub items: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct Post {
    pub postId: usize,
    pub transparentShareOfPostId: Option<usize>,
    pub shareOfPostId: Option<usize>,
    pub publishedAt: String,
    pub headline: String,
    pub blocks: Vec<Block>,
    pub plainTextBody: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(non_snake_case)]
pub enum Block {
    #[serde(rename = "markdown")]
    Markdown { markdown: Markdown },

    #[serde(rename = "attachment")]
    Attachment { attachment: Attachment },

    #[serde(untagged)]
    Unknown {
        #[serde(flatten)]
        fields: HashMap<String, Value>,
    },
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct Markdown {
    pub content: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
#[allow(non_snake_case)]
pub enum Attachment {
    #[serde(rename = "image")]
    Image {
        attachmentId: String,
        altText: String,
        width: usize,
        height: usize,
    },

    #[serde(untagged)]
    Unknown {
        #[serde(flatten)]
        fields: HashMap<String, Value>,
    },
}
