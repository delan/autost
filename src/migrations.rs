use std::fs::{create_dir_all, read_dir};

use jane_eyre::eyre::{self, bail};
use sqlx::{
    migrate::Migrate as _, sqlite::SqliteConnectOptions, ConnectOptions as _, Connection as _,
    Sqlite, SqliteConnection, Transaction,
};
use tracing::{info, trace};

use crate::path::{
    hard_link_if_not_exists, PostsPath, SitePath, POSTS_PATH_IMPORTED, POSTS_PATH_ROOT,
    SITE_PATH_ATTACHMENTS,
};

#[tracing::instrument]
pub async fn run_migrations() -> eyre::Result<SqliteConnection> {
    // since 0.3.0
    info!("hard linking attachments out of site/attachments");
    create_dir_all(&*SITE_PATH_ATTACHMENTS)?;
    let mut dirs = vec![SITE_PATH_ATTACHMENTS.to_owned()];
    let mut files: Vec<SitePath> = vec![];
    while !dirs.is_empty() || !files.is_empty() {
        for site_path in files.drain(..) {
            trace!(?site_path);
            let Some(attachments_path) = site_path.attachments_path()? else {
                bail!("path is not an attachment path: {site_path:?}");
            };
            let Some(parent) = attachments_path.parent() else {
                bail!("path has no parent: {site_path:?}");
            };
            create_dir_all(parent)?;
            hard_link_if_not_exists(site_path, attachments_path)?;
        }
        if let Some(dir) = dirs.pop() {
            for entry in read_dir(&dir)? {
                let entry = entry?;
                let r#type = entry.file_type()?;
                let path = dir.join_dir_entry(&entry)?;
                if r#type.is_dir() {
                    dirs.push(path);
                } else if r#type.is_file() {
                    files.push(path);
                } else {
                    bail!(
                        "file in site/attachments with unexpected type: {:?}: {:?}",
                        r#type,
                        entry.path()
                    );
                }
            }
        }
    }

    // since ?.?.?
    info!("running database migrations (or creating database)");
    let mut conn = SqliteConnectOptions::new()
        .filename("autost.sqlite")
        .create_if_missing(true)
        .connect()
        .await?;
    let mut tx = conn.begin().await?;
    tx.ensure_migrations_table().await?;
    let previously_applied_migrations = tx.list_applied_migrations().await?;
    sqlx::migrate!().run(&mut tx).await?;

    // since ?.?.?: backfill `post` and `import` tables
    if !previously_applied_migrations
        .iter()
        .any(|m| m.version == 20250815040702)
    {
        info!("database post-migration step: backfilling `post` table");
        create_dir_all(&*POSTS_PATH_ROOT)?;
        backfill_post_table(&mut tx, || {
            Ok(read_dir(&*POSTS_PATH_ROOT)?.map(|entry| POSTS_PATH_ROOT.join_dir_entry(&entry?)))
        })
        .await?;

        info!("database post-migration step: backfilling `import` table");
        create_dir_all(&*POSTS_PATH_IMPORTED)?;
        backfill_import_table(&mut tx, || {
            Ok(read_dir(&*POSTS_PATH_IMPORTED)?
                .map(|entry| POSTS_PATH_IMPORTED.join_dir_entry(&entry?)))
        })
        .await?;
    }

    // commit all database migrations as a single transaction
    tx.commit().await?;
    info!("finished running migrations");

    Ok(conn)
}

async fn backfill_post_table<Paths: Iterator<Item = eyre::Result<PostsPath>>>(
    tx: &mut Transaction<'_, Sqlite>,
    mut top_level_posts_paths: impl FnMut() -> eyre::Result<Paths>,
) -> eyre::Result<()> {
    // ensure that potential cohost post ids (less than 10000000) are never used except when
    // importing chosts, by inserting and deleting a dummy row with post id 9999999.
    sqlx::query(r#"INSERT INTO "post" ("post_id", "path") VALUES (9999999, "")"#)
        .execute(&mut **tx)
        .await?;
    sqlx::query(r#"DELETE FROM "post""#)
        .execute(&mut **tx)
        .await?;

    // insert top-level posts of the form `<usize>.{md,html}`, with their numbers as `post_id`.
    for path in top_level_posts_paths()? {
        let path = path?;
        if let Some(post_id) = path.top_level_numeric_post_id() {
            trace!(?post_id, ?path, "INSERT INTO post");
            match sqlx::query(
                r#"INSERT INTO "post" ("post_id", "path", "rendered_path") VALUES ($1, $2, $3)"#,
            )
            .bind(i64::try_from(post_id)?)
            .bind(path.db_post_table_path())
            .bind(path.rendered_path()?.map(|path| path.db_post_table_path()))
            .execute(&mut **tx)
            .await
            {
                Ok(_result) => {}
                Err(error) => {
                    if let Some(error) = error.as_database_error() {
                        if error.code().as_deref() == /* SQLITE_CONSTRAINT_PRIMARYKEY */ Some("1555")
                        {
                            let path = path.rendered_path();
                            bail!("must not have two top-level posts with the same rendered path: {path:?}")
                        }
                    }
                    Err(error)?
                }
            }
        }
    }

    // insert all other top-level posts, with sequential `post_id`.
    for path in top_level_posts_paths()? {
        let path = path?;
        if path.is_top_level_post() && path.top_level_numeric_post_id().is_none() {
            trace!(?path, "INSERT INTO post");
            match sqlx::query(r#"INSERT INTO "post" ("path", "rendered_path") VALUES ($1, $2)"#)
                .bind(path.db_post_table_path())
                .bind(path.rendered_path()?.map(|path| path.db_post_table_path()))
                .execute(&mut **tx)
                .await
            {
                Ok(_result) => {}
                Err(error) => {
                    if let Some(error) = error.as_database_error() {
                        if error.code().as_deref() == /* SQLITE_CONSTRAINT_UNIQUE */ Some("2067") {
                            let path = path.rendered_path();
                            bail!("must not have two top-level posts with the same rendered path: {path:?}")
                        }
                    }
                    Err(error)?
                }
            }
        }
    }

    Ok(())
}

async fn backfill_import_table<Paths: Iterator<Item = eyre::Result<PostsPath>>>(
    tx: &mut Transaction<'_, Sqlite>,
    mut imported_posts_paths: impl FnMut() -> eyre::Result<Paths>,
) -> eyre::Result<()> {
    // insert imported posts of the form `imported/<usize>.html`, with their numbers as `import_id`.
    for path in imported_posts_paths()? {
        let path = path?;
        if let Some(import_id) = path.import_id() {
            trace!(?import_id, "INSERT INTO import");
            sqlx::query(r#"INSERT INTO "import" ("import_id") VALUES ($1)"#)
                .bind(i64::try_from(import_id)?)
                .execute(&mut **tx)
                .await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use jane_eyre::eyre;
    use sqlx::{Connection as _, Row as _, Sqlite, SqliteConnection, Transaction};

    use crate::{
        migrations::{backfill_import_table, backfill_post_table},
        path::PostsPath,
    };

    async fn conn() -> eyre::Result<SqliteConnection> {
        Ok(SqliteConnection::connect("sqlite::memory:").await?)
    }

    async fn migration_tx(conn: &mut SqliteConnection) -> eyre::Result<Transaction<'_, Sqlite>> {
        let mut tx = conn.begin().await?;
        sqlx::migrate!().run(&mut tx).await?;
        Ok(tx)
    }

    async fn select_from_post(
        tx: &mut Transaction<'_, Sqlite>,
    ) -> eyre::Result<Vec<(i64, String, Option<String>)>> {
        let result = sqlx::query(r#"SELECT * FROM "post""#)
            .fetch_all(&mut **tx)
            .await?
            .into_iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect();
        Ok(result)
    }

    async fn select_from_import(tx: &mut Transaction<'_, Sqlite>) -> eyre::Result<Vec<i64>> {
        let result = sqlx::query(r#"SELECT * FROM "import""#)
            .fetch_all(&mut **tx)
            .await?
            .into_iter()
            .map(|row| (row.get(0)))
            .collect();
        Ok(result)
    }

    #[tokio::test]
    async fn test_backfill_post_table() -> eyre::Result<()> {
        let mut conn = conn().await?;

        // numeric posts get their numbers as `post_id`.
        let mut tx = migration_tx(&mut conn).await?;
        let paths = ["posts/2.md"];
        backfill_post_table(&mut tx, || {
            Ok(paths
                .iter()
                .map(|path| PostsPath::from_site_root_relative_path(path)))
        })
        .await?;
        assert_eq!(
            select_from_post(&mut tx).await?,
            [(2, "2.md".to_owned(), Some("2.html".to_owned()))]
        );
        tx.rollback().await?;

        // same goes for numeric html posts.
        let mut tx = migration_tx(&mut conn).await?;
        let paths = ["posts/2.html"];
        backfill_post_table(&mut tx, || {
            Ok(paths
                .iter()
                .map(|path| PostsPath::from_site_root_relative_path(path)))
        })
        .await?;
        assert_eq!(
            select_from_post(&mut tx).await?,
            [(2, "2.html".to_owned(), Some("2.html".to_owned()))]
        );
        tx.rollback().await?;

        // two numeric posts must not have the same path modulo extension
        let mut tx = migration_tx(&mut conn).await?;
        let paths = ["posts/2.md", "posts/2.html"];
        assert!(backfill_post_table(&mut tx, || Ok(paths
            .iter()
            .map(|path| PostsPath::from_site_root_relative_path(path))))
        .await
        .is_err());
        tx.rollback().await?;

        // non-numeric posts start from 10000000.
        let mut tx = migration_tx(&mut conn).await?;
        let paths = ["posts/hello.md"];
        backfill_post_table(&mut tx, || {
            Ok(paths
                .iter()
                .map(|path| PostsPath::from_site_root_relative_path(path)))
        })
        .await?;
        assert_eq!(
            select_from_post(&mut tx).await?,
            [(
                10000000,
                "hello.md".to_owned(),
                Some("hello.html".to_owned())
            )]
        );
        tx.rollback().await?;

        // same goes for non-numeric html posts.
        let mut tx = migration_tx(&mut conn).await?;
        let paths = ["posts/hello.html"];
        backfill_post_table(&mut tx, || {
            Ok(paths
                .iter()
                .map(|path| PostsPath::from_site_root_relative_path(path)))
        })
        .await?;
        assert_eq!(
            select_from_post(&mut tx).await?,
            [(
                10000000,
                "hello.html".to_owned(),
                Some("hello.html".to_owned())
            )]
        );
        tx.rollback().await?;

        // two non-numeric posts must not have the same path modulo extension.
        let mut tx = migration_tx(&mut conn).await?;
        let paths = ["posts/hello.md", "posts/hello.html"];
        assert!(backfill_post_table(&mut tx, || Ok(paths
            .iter()
            .map(|path| PostsPath::from_site_root_relative_path(path))))
        .await
        .is_err());
        tx.rollback().await?;

        // all together now.
        let mut tx = migration_tx(&mut conn).await?;
        let paths = ["posts/hello.html", "posts/3.md", "posts/5.html"];
        backfill_post_table(&mut tx, || {
            Ok(paths
                .iter()
                .map(|path| PostsPath::from_site_root_relative_path(path)))
        })
        .await?;
        assert_eq!(
            select_from_post(&mut tx).await?,
            [
                (3, "3.md".to_owned(), Some("3.html".to_owned())),
                (5, "5.html".to_owned(), Some("5.html".to_owned())),
                (
                    10000000,
                    "hello.html".to_owned(),
                    Some("hello.html".to_owned())
                ),
            ]
        );
        tx.rollback().await?;

        // new posts with start from 10000000.
        let mut tx = migration_tx(&mut conn).await?;
        backfill_post_table(&mut tx, || {
            Ok([].into_iter().map(PostsPath::from_site_root_relative_path))
        })
        .await?;
        sqlx::query(r#"INSERT INTO "post" ("path") VALUES ("")"#)
            .execute(&mut *tx)
            .await?;
        assert_eq!(
            select_from_post(&mut tx).await?,
            [(10000000, "".to_owned(), None)]
        );
        tx.rollback().await?;

        Ok(())
    }

    #[tokio::test]
    async fn test_backfill_import_table() -> eyre::Result<()> {
        let mut conn = conn().await?;

        // imported posts get their numbers as `import_id`.
        let mut tx = migration_tx(&mut conn).await?;
        let paths = ["posts/imported/3.html", "posts/imported/5.html"];
        backfill_import_table(&mut tx, || {
            Ok(paths
                .iter()
                .map(|path| PostsPath::from_site_root_relative_path(path)))
        })
        .await?;
        assert_eq!(select_from_import(&mut tx).await?, [3, 5]);
        tx.rollback().await?;

        Ok(())
    }
}
