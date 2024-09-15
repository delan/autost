use std::{env::args, fs::File, io::Write, path::Path};

use askama::Template;
use autost::{cli_init, PostGroup, PostsPageTemplate, TemplatedPost};
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

        let post_group = PostGroup {
            href: filename.clone(),
            posts,
            meta,
        };

        // generate post page.
        let template = PostsPageTemplate {
            post_groups: vec![post_group.clone()],
        };
        let path = output_path.join(filename);
        info!("writing post page: {path:?}");
        writeln!(File::create(path)?, "{}", template.render()?)?;

        post_groups.push(post_group);
    }

    // reader step: generate posts page.
    post_groups.sort_by(|p, q| p.meta.published.cmp(&q.meta.published).reverse());
    let template = PostsPageTemplate { post_groups };
    let posts_page_path = output_path.join("index.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;

    Ok(())
}
