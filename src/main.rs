use std::{env::args, fs::File, io::Write, path::Path};

use askama::Template;
use autost::{cli_init, AtomFeedTemplate, PostGroup, PostsPageTemplate, TemplatedPost};
use jane_eyre::eyre::{self};
use tracing::info;

fn main() -> eyre::Result<()> {
    cli_init()?;

    let mut post_groups = vec![];

    let output_path = args().nth(1).unwrap();
    let output_path = Path::new(&output_path);

    for path in args().skip(2) {
        let path = Path::new(&path);

        let post = TemplatedPost::load(path)?;
        let filename = post.filename.clone();
        let meta = post.meta.clone();

        let mut posts = post
            .meta
            .references
            .iter()
            .flat_map(|filename| path.parent().map(|path| path.join(filename)))
            .map(|path| TemplatedPost::load(&path))
            .collect::<Result<Vec<_>, _>>()?;
        posts.push(post);

        let overall_title = posts
            .iter()
            .rev()
            .find(|post| !post.meta.is_transparent_share)
            .and_then(|post| post.meta.title.clone())
            .unwrap_or("".to_owned());

        let post_group = PostGroup {
            href: filename.clone(),
            posts,
            meta,
            overall_title,
        };

        // reader step: generate post page.
        let template = PostsPageTemplate {
            post_groups: vec![post_group.clone()],
        };
        let path = output_path.join(filename);
        info!("writing post page: {path:?}");
        writeln!(File::create(path)?, "{}", template.render()?)?;

        post_groups.push(post_group);
    }

    post_groups.sort_by(|p, q| p.meta.published.cmp(&q.meta.published).reverse());

    // author step: generate atom feed.
    let template = AtomFeedTemplate {
        post_groups: post_groups.clone(),
    };
    let atom_feed_path = output_path.join("feed.xml");
    writeln!(File::create(atom_feed_path)?, "{}", template.render()?)?;

    // reader step: generate posts page.
    let template = PostsPageTemplate { post_groups };
    let posts_page_path = output_path.join("index.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;

    Ok(())
}
