use std::collections::BTreeSet;

use jane_eyre::eyre;

use crate::path::{AttachmentsPath, PostsPath, RelativePath, SitePath, POSTS_PATH_ROOT};

pub enum DepNode<Kind> {
    Path(RelativePath<Kind>),
    Resolved(Resolved<Kind>),
}

#[allow(unused)]
pub struct Resolved<Kind> {
    path: RelativePath<Kind>,
    hash: String,
    depends_on_posts: BTreeSet<PostsPath>,
    depends_on_site: BTreeSet<SitePath>,
    depends_on_attachments: BTreeSet<AttachmentsPath>,
}

pub async fn build_dep_tree() -> eyre::Result<()> {
    for path in POSTS_PATH_ROOT.read_dir_flat()? {
        dbg!(path);
    }

    Ok(())
}
