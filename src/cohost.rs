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
    pub headline: String,
    pub plainTextBody: String,
}
