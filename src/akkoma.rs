use askama::Template;
use serde::Deserialize;

use crate::Author;

/// <https://docs.joinmastodon.org/entities/Instance/>
#[derive(Deserialize)]
pub struct ApiInstance {
    pub version: String,
    pub uri: String,
}

/// <https://docs.joinmastodon.org/entities/Status/>
#[derive(Deserialize)]
pub struct ApiStatus {
    pub content: String,
    pub url: String,
    pub account: ApiAccount,
    pub media_attachments: Vec<ApiMediaAttachment>,
    pub tags: Vec<ApiStatusTag>,
    pub created_at: String,
}

/// <https://docs.joinmastodon.org/entities/Status/#Tag>
#[derive(Deserialize)]
pub struct ApiStatusTag {
    pub name: String,
    pub url: String,
}

/// <https://docs.joinmastodon.org/entities/MediaAttachment/>
#[derive(Deserialize)]
pub struct ApiMediaAttachment {
    pub r#type: String,
    pub description: String,
    pub url: String,
    pub preview_url: String,
}

/// <https://docs.joinmastodon.org/entities/Account/>
#[derive(Deserialize)]
pub struct ApiAccount {
    pub url: String,
    pub display_name: String,
    pub username: String,
    pub acct: String,
    pub fqn: String,
}

#[derive(Template)]
#[template(path = "akkoma-img.html")]
pub struct AkkomaImgTemplate {
    pub data_akkoma_src: String,
    pub href: String,
    pub src: String,
    pub alt: String,
}

impl From<&ApiAccount> for Author {
    fn from(account: &ApiAccount) -> Self {
        Self {
            href: account.url.clone(),
            name: if account.display_name.is_empty() {
                format!("@{}", account.fqn)
            } else {
                format!("{} (@{})", account.display_name, account.fqn)
            },
            display_name: account.display_name.clone(),
            display_handle: format!("@{}", account.fqn),
        }
    }
}

#[test]
fn test_author_from_api_account() {
    assert_eq!(
        Author::from(&ApiAccount {
            url: "https://posting.isincredibly.gay/users/ruby".to_owned(),
            display_name: "srxl".to_owned(),
            username: "ruby".to_owned(),
            acct: "ruby".to_owned(),
            fqn: "ruby@posting.isincredibly.gay".to_owned(),
        }),
        Author {
            href: "https://posting.isincredibly.gay/users/ruby".to_owned(),
            name: "srxl (@ruby@posting.isincredibly.gay)".to_owned(),
            display_name: "srxl".to_owned(),
            display_handle: "@ruby@posting.isincredibly.gay".to_owned(),
        }
    );

    // not allowed by akkoma frontend, but theoretically possible
    assert_eq!(
        Author::from(&ApiAccount {
            url: "https://posting.isincredibly.gay/users/ruby".to_owned(),
            display_name: "".to_owned(),
            username: "ruby".to_owned(),
            acct: "ruby".to_owned(),
            fqn: "ruby@posting.isincredibly.gay".to_owned(),
        }),
        Author {
            href: "https://posting.isincredibly.gay/users/ruby".to_owned(),
            name: "@ruby@posting.isincredibly.gay".to_owned(),
            display_name: "".to_owned(),
            display_handle: "@ruby@posting.isincredibly.gay".to_owned(),
        }
    );
}
