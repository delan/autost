use std::{
    collections::{BTreeMap, BTreeSet},
    fs::read,
    mem::take,
};

use jane_eyre::eyre;
use rayon::iter::{IntoParallelIterator, ParallelIterator as _};
use sqlx::{Connection, Row, SqliteConnection};
use tracing::info;

use crate::{
    cache::{hash_bytes, parse_hash_hex},
    output::ThreadsContentTemplate,
    path::{DynamicPath, PostsPath, ATTACHMENTS_PATH_ROOT, POSTS_PATH_ROOT},
    FilteredPost, Thread, UnsafeExtractedPost, UnsafePost,
};

#[derive(Clone, Debug, PartialEq)]
pub struct PostNode {
    path: PostsPath,
    hash: String,
    needs: BTreeSet<DynamicPath>,
}

pub async fn build_dep_tree(mut db: SqliteConnection) -> eyre::Result<()> {
    let mut tx = db.begin().await?;
    let mut cached_hash: BTreeMap<DynamicPath, String> = BTreeMap::default();
    let mut cached_deps: BTreeMap<DynamicPath, BTreeSet<DynamicPath>> = BTreeMap::default();
    let mut cached_dependents: BTreeMap<DynamicPath, BTreeSet<DynamicPath>> = BTreeMap::default();
    let posts_paths = POSTS_PATH_ROOT
        .read_dir_flat()?
        .into_iter()
        .map(|path| path.into_dynamic_path());
    let attachments_paths = ATTACHMENTS_PATH_ROOT
        .read_dir_flat()?
        .into_iter()
        .map(|path| path.into_dynamic_path());
    let mut queue = BTreeSet::default();

    let attachment_cache = sqlx::query(r#"SELECT "path", "hash" FROM "attachment_cache""#)
        .fetch_all(&mut *tx)
        .await?;
    for row in attachment_cache {
        let path = DynamicPath::from_site_root_relative_path(row.get("path"))?;
        let hash: &str = row.get("hash");
        cached_hash.insert(path.clone(), hash.to_owned());
    }

    let dep_cache = sqlx::query(r#"SELECT "file_cache"."path" "path", "file_cache"."hash" "hash", "needs_path" FROM "file_cache" LEFT JOIN "dep_cache" ON "file_cache"."path" = "dep_cache"."path" AND "file_cache"."hash" = "dep_cache"."hash""#)
        .fetch_all(&mut *tx)
        .await?;
    for row in dep_cache {
        let path = DynamicPath::from_site_root_relative_path(row.get("path"))?;
        let hash = row.get("hash");
        cached_hash.insert(path.clone(), hash);
        if let Some(needs_path) = row.get("needs_path") {
            let needs_path = DynamicPath::from_site_root_relative_path(needs_path)?;
            cached_deps
                .entry(path.clone())
                .or_default()
                .insert(needs_path.clone());
            cached_dependents
                .entry(needs_path.clone())
                .or_default()
                .insert(path.clone());
        }
    }

    for dynamic_path in posts_paths.chain(attachments_paths) {
        match &dynamic_path {
            DynamicPath::Posts(path) => {
                if let Some(hash) = cached_hash.get(&dynamic_path) {
                    if hash_bytes(read(path)?) != parse_hash_hex(hash)? {
                        queue.insert(dynamic_path);
                    }
                } else {
                    queue.insert(dynamic_path);
                }
            }
            DynamicPath::Site(_path) => {
                unreachable!()
            }
            DynamicPath::Attachments(_path) => { /* do nothing */ }
        }
    }

    while !queue.is_empty() {
        for path in queue.iter() {
            info!(?path, "need to rebuild");
        }
        let results = take(&mut queue)
            .into_par_iter()
            .map(|path| -> eyre::Result<_> {
                Ok(match path {
                    DynamicPath::Posts(path) => {
                        let post = UnsafePost::load(&path)?;
                        let post = UnsafeExtractedPost::new(post)?;
                        let needs_posts = post
                            .meta
                            .front_matter
                            .references
                            .clone()
                            .into_iter()
                            .map(|path| path.into_dynamic_path());
                        let needs_attachments = post
                            .meta
                            .needs_attachments
                            .into_iter()
                            .map(|path| path.into_dynamic_path());
                        let needs = needs_posts
                            .chain(needs_attachments)
                            .collect::<BTreeSet<_>>();
                        Some(PostNode {
                            path,
                            hash: post.post.hash.to_string(),
                            needs: needs.clone(),
                        })
                    }
                    DynamicPath::Site(_) => None,
                    DynamicPath::Attachments(_) => None,
                })
            })
            .filter_map(|result| result.transpose())
            .collect::<eyre::Result<Vec<_>>>()?;
        for node in results {
            sqlx::query(r#"INSERT INTO "file_cache" ("path", "hash") VALUES ($1, $2) ON CONFLICT DO UPDATE SET "hash" = "excluded"."hash""#)
                .bind(node.path.to_dynamic_path().db_dep_table_path())
                .bind(node.hash.clone())
                .execute(&mut *tx)
                .await?;
            sqlx::query(r#"DELETE FROM "dep_cache" WHERE "path" = $1"#)
                .bind(node.path.to_dynamic_path().db_dep_table_path())
                .execute(&mut *tx)
                .await?;
            for needs_path in node.needs {
                sqlx::query(
                    r#"INSERT INTO "dep_cache" ("path", "hash", "needs_path") VALUES ($1, $2, $3)"#,
                )
                .bind(node.path.to_dynamic_path().db_dep_table_path())
                .bind(node.hash.clone())
                .bind(needs_path.db_dep_table_path())
                .execute(&mut *tx)
                .await?;
            }
            let post = FilteredPost::load(&node.path)?;
            let thread = Thread::try_from(post)?;
            let normal = ThreadsContentTemplate::render_normal(&thread)?;
            let simple = ThreadsContentTemplate::render_simple(&thread)?;
            sqlx::query(r#"INSERT INTO "threads_content_cache" ("path", "hash", "normal", "simple") VALUES ($1, $2, $3, $4) ON CONFLICT DO UPDATE SET "hash" = "excluded"."hash", "normal" = "excluded"."normal", "simple" = "excluded"."simple""#)
                .bind(node.path.to_dynamic_path().db_dep_table_path())
                .bind(node.hash.clone())
                .bind(normal)
                .bind(simple)
                .execute(&mut *tx)
                .await?;
            if let Some(dependents) = cached_dependents.get(&node.path.to_dynamic_path()) {
                queue.extend(dependents.iter().cloned());
            }
        }
    }

    tx.commit().await?;
    info!("done!");

    Ok(())
}
