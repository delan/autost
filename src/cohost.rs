use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::Author;

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct PostsResponse {
    pub nItems: usize,
    pub nPages: usize,
    pub items: Vec<Value>,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct Post {
    pub postId: usize,
    pub transparentShareOfPostId: Option<usize>,
    pub shareOfPostId: Option<usize>,
    pub filename: String,
    pub publishedAt: String,
    pub headline: String,
    pub tags: Vec<String>,
    pub postingProject: PostingProject,
    pub shareTree: Vec<Post>,

    /// markdown source only, without attachments or asks.
    pub plainTextBody: String,

    /// post body (markdown), attachments, and asks (markdown).
    pub blocks: Vec<Block>,

    /// fully rendered versions of markdown blocks.
    pub astMap: AstMap,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct PostingProject {
    pub handle: String,
    pub displayName: String,
    pub privacy: String,
    pub loggedOutPostVisibility: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type")]
#[allow(non_snake_case)]
pub enum Block {
    #[serde(rename = "markdown")]
    Markdown { markdown: Markdown },

    #[serde(rename = "attachment")]
    Attachment { attachment: Attachment },

    #[serde(rename = "attachment-row")]
    AttachmentRow { attachments: Vec<Block> },

    #[serde(rename = "ask")]
    Ask { ask: Ask },

    #[serde(untagged)]
    Unknown {
        #[serde(flatten)]
        fields: HashMap<String, Value>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct Markdown {
    pub content: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind")]
#[allow(non_snake_case)]
pub enum Attachment {
    #[serde(rename = "image")]
    Image {
        attachmentId: String,
        altText: Option<String>,
        width: Option<usize>,
        height: Option<usize>,
    },

    #[serde(rename = "audio")]
    Audio {
        attachmentId: String,
        artist: String,
        title: String,
    },

    #[serde(untagged)]
    Unknown {
        #[serde(flatten)]
        fields: HashMap<String, Value>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct Ask {
    pub content: String,
    pub askingProject: Option<AskingProject>,
    pub anon: bool,
    pub loggedIn: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct AskingProject {
    pub handle: String,
    pub displayName: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct AstMap {
    pub spans: Vec<Span>,
}

#[derive(Debug, Deserialize, Serialize)]
#[allow(non_snake_case)]
pub struct Span {
    pub ast: String,
    pub startIndex: usize,
    pub endIndex: usize,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct TrpcResponse<T> {
    pub result: TrpcResult<T>,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct TrpcResult<T> {
    pub data: T,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct ListEditedProjectsResponse {
    pub projects: Vec<EditedProject>,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct EditedProject {
    pub projectId: usize,
    pub handle: String,
    pub displayName: String,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct LoggedInResponse {
    pub projectId: usize,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct FollowedFeedResponse {
    pub projects: Vec<FeedProject>,
    pub nextCursor: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct FeedProject {
    pub project: FollowedProject,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct FollowedProject {
    pub projectId: usize,
    pub handle: String,
}

#[derive(Debug, Deserialize)]
pub struct LikedPostsState {
    #[serde(rename = "liked-posts-feed")]
    pub liked_posts_feed: LikedPostsFeed,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct LikedPostsFeed {
    pub posts: Vec<Post>,
    pub paginationMode: PaginationMode,
}

#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
pub struct PaginationMode {
    pub currentSkip: usize,
    pub idealPageStride: usize,
    pub mode: String,
    pub morePagesForward: bool,
    pub morePagesBackward: bool,
    pub pageUrlFactoryName: String,
    pub refTimestamp: usize,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
#[allow(non_snake_case)]
pub enum Ast {
    #[serde(rename = "root")]
    Root { children: Vec<Ast> },

    #[serde(rename = "element")]
    Element {
        tagName: String,
        properties: HashMap<String, Value>,
        children: Vec<Ast>,
    },

    #[serde(rename = "text")]
    Text { value: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cacheable<'url> {
    /// cohost attachment (staging.cohostcdn.org/attachment or an equivalent redirect)
    Attachment {
        id: &'url str,
        url: Option<&'url str>,
    },
    /// cohost emote, eggbug logo, or other static asset (cohost.org/static)
    Static { filename: &'url str, url: &'url str },
    /// cohost avatar (static.cohostcdn.org/avatar)
    Avatar { filename: &'url str, url: &'url str },
    /// cohost header (static.cohostcdn.org/header)
    Header { filename: &'url str, url: &'url str },
}

impl<'url> Cacheable<'url> {
    pub fn attachment(id: &'url str, original_url: impl Into<Option<&'url str>>) -> Self {
        Self::Attachment {
            id,
            url: original_url.into(),
        }
    }

    #[must_use] pub const fn r#static(filename: &'url str, url: &'url str) -> Self {
        Self::Static { filename, url }
    }

    #[must_use] pub const fn avatar(filename: &'url str, url: &'url str) -> Self {
        Self::Avatar { filename, url }
    }

    #[must_use] pub const fn header(filename: &'url str, url: &'url str) -> Self {
        Self::Header { filename, url }
    }

    pub fn from_url(url: &'url str) -> Option<Self> {
        // attachment redirects just have the uuid in a fixed location.
        if let Some(attachment_id) = url
            .strip_prefix("https://cohost.org/rc/attachment-redirect/")
            .or_else(|| url.strip_prefix("https://cohost.org/api/v1/attachments/"))
            .filter(|id_plus| id_plus.len() >= 36)
            .map(|id_plus| &id_plus[..36])
        {
            return Some(Self::attachment(attachment_id, url));
        }
        // raw attachment urls have a mandatory trailing path component for the original filename,
        // preceded by a path component for the uuid, preceded by zero or more extra garbage path
        // components, which the server still accepts. people have used this in real posts.
        if let Some(attachment_id_etc) =
            url.strip_prefix("https://staging.cohostcdn.org/attachment/")
        {
            // remove query string, if any
            let attachment_id_etc = attachment_id_etc
                .split_once('?')
                .map_or(attachment_id_etc, |(result, _query_string)| result);
            // remove original filename
            if let Some(attachment_id_etc) = attachment_id_etc
                .rsplit_once('/')
                .map(|(result, _original_filename)| result)
            {
                // remove path components preceding uuid, if any
                let attachment_id = attachment_id_etc
                    .rsplit_once('/')
                    .map_or(attachment_id_etc, |(_garbage, result)| result);
                return Some(Self::attachment(attachment_id, url));
            }
        }
        if let Some(static_filename) = url.strip_prefix("https://cohost.org/static/") {
            if static_filename.is_empty() {
                warn!(url, "skipping cohost static path without filename");
                return None;
            }
            if static_filename.contains(['/', '?']) {
                warn!(
                    url,
                    "skipping cohost static path with unexpected slash or query string",
                );
                return None;
            }
            return Some(Self::r#static(static_filename, url));
        }
        if let Some(avatar_filename) = url.strip_prefix("https://staging.cohostcdn.org/avatar/") {
            if avatar_filename.is_empty() {
                warn!(url, "skipping cohost avatar path without filename");
                return None;
            }
            if avatar_filename.contains('/') {
                warn!(url, "skipping cohost avatar path with unexpected slash");
                return None;
            }
            if let Some((avatar_filename, _query_string)) = avatar_filename.split_once('?') {
                // some chosts use avatars with query parameters to resize etc, such as
                // <https://cohost.org/srxl/post/4940861-p-style-padding-to>.
                // to make things simpler for us, we only bother archiving the original.
                // if the chost relies on the intrinsic size of the resized avatar, tough luck.
                warn!(url, "dropping query string from cohost avatar path");
                return Some(Self::avatar(avatar_filename, url));
            }
            return Some(Self::avatar(avatar_filename, url));
        }
        if let Some(header_filename) = url.strip_prefix("https://staging.cohostcdn.org/header/") {
            if header_filename.is_empty() {
                warn!(url, "skipping cohost header path without filename");
                return None;
            }
            if header_filename.contains('/') {
                warn!(url, "skipping cohost header path with unexpected slash");
                return None;
            }
            if let Some((header_filename, _query_string)) = header_filename.split_once('?') {
                // some chosts use headers with query parameters to resize etc, such as
                // <https://cohost.org/srxl/post/4940861-p-style-padding-to>.
                // to make things simpler for us, we only bother archiving the original.
                // if the chost relies on the intrinsic size of the resized header, tough luck.
                warn!(url, "dropping query string from cohost header path");
                return Some(Self::header(header_filename, url));
            }
            return Some(Self::header(header_filename, url));
        }

        None
    }
}

#[must_use] pub fn attachment_id_to_url(id: &str) -> String {
    format!("https://cohost.org/rc/attachment-redirect/{id}")
}

#[test]
fn test_cacheable() {
    assert_eq!(
        Cacheable::from_url(
            "https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444?query",
        ),
        Some(Cacheable::Attachment {
            id: "44444444-4444-4444-4444-444444444444",
            url: "https://cohost.org/rc/attachment-redirect/44444444-4444-4444-4444-444444444444?query".into(),
        }),
    );
    assert_eq!(
        Cacheable::from_url(
            "https://cohost.org/api/v1/attachments/44444444-4444-4444-4444-444444444444?query",
        ),
        Some(Cacheable::Attachment {
            id: "44444444-4444-4444-4444-444444444444",
            url: "https://cohost.org/api/v1/attachments/44444444-4444-4444-4444-444444444444?query"
                .into(),
        }),
    );
    assert_eq!(
        Cacheable::from_url(
            "https://staging.cohostcdn.org/attachment/44444444-4444-4444-4444-444444444444/file.jpg?query",
        ),
        Some(Cacheable::Attachment {
            id: "44444444-4444-4444-4444-444444444444",
            url: "https://staging.cohostcdn.org/attachment/44444444-4444-4444-4444-444444444444/file.jpg?query".into(),
        }),
    );
    assert_eq!(
        Cacheable::from_url(
            "https://staging.cohostcdn.org/attachment/https://staging.cohostcdn.org/attachment/d99a2208-5a1d-4212-b524-1d6e3493d6f4/silent_hills_pt_screen_20140814_02.jpg?query",
        ),
        Some(Cacheable::Attachment {
            id: "d99a2208-5a1d-4212-b524-1d6e3493d6f4",
            url: "https://staging.cohostcdn.org/attachment/https://staging.cohostcdn.org/attachment/d99a2208-5a1d-4212-b524-1d6e3493d6f4/silent_hills_pt_screen_20140814_02.jpg?query".into(),
        }),
    );
    assert_eq!(
        Cacheable::from_url("https://cohost.org/static/f0c56e99113f1a0731b4.svg"),
        Some(Cacheable::Static {
            filename: "f0c56e99113f1a0731b4.svg",
            url: "https://cohost.org/static/f0c56e99113f1a0731b4.svg",
        }),
    );
    assert_eq!(Cacheable::from_url("https://cohost.org/static/"), None);
    assert_eq!(
        Cacheable::from_url("https://cohost.org/static/f0c56e99113f1a0731b4.svg?query"),
        None
    );
    assert_eq!(
        Cacheable::from_url("https://cohost.org/static/subdir/f0c56e99113f1a0731b4.svg"),
        None
    );
}

impl From<&PostingProject> for Author {
    fn from(project: &PostingProject) -> Self {
        Self {
            href: format!("https://cohost.org/{}", project.handle),
            name: if project.displayName.is_empty() {
                format!("@{}", project.handle)
            } else {
                format!("{} (@{})", project.displayName, project.handle)
            },
            display_name: project.displayName.clone(),
            display_handle: format!("@{}", project.handle),
        }
    }
}

#[test]
fn test_author_from_posting_project() {
    assert_eq!(
        Author::from(&PostingProject {
            handle: "staff".to_owned(),
            displayName: "cohost dot org".to_owned(),
            privacy: "[any value]".to_owned(),
            loggedOutPostVisibility: "[any value]".to_owned(),
        }),
        Author {
            href: "https://cohost.org/staff".to_owned(),
            name: "cohost dot org (@staff)".to_owned(),
            display_name: "cohost dot org".to_owned(),
            display_handle: "@staff".to_owned(),
        }
    );
    assert_eq!(
        Author::from(&PostingProject {
            handle: "VinDuv".to_owned(),
            displayName: String::new(),
            privacy: "[any value]".to_owned(),
            loggedOutPostVisibility: "[any value]".to_owned(),
        }),
        Author {
            href: "https://cohost.org/VinDuv".to_owned(),
            name: "@VinDuv".to_owned(),
            display_name: String::new(),
            display_handle: "@VinDuv".to_owned(),
        }
    );
}
