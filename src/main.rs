use std::{collections::BTreeMap, env::args, fs::File, io::Write, path::Path};

use askama::Template;
use autost::{cli_init, AtomFeedTemplate, PostGroup, PostsPageTemplate, TemplatedPost, SETTINGS};
use jane_eyre::eyre::{self};
use tracing::{info, trace};

fn main() -> eyre::Result<()> {
    cli_init()?;

    let mut post_groups = vec![];
    let mut post_groups_with_interesting_tags = vec![];
    let mut post_groups_by_interesting_tag = BTreeMap::default();
    let mut tags = BTreeMap::default();

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

        // TODO: skip post groups with other authors?
        // TODO: skip post groups with private or logged-in-only authors?
        // TODO: gate sensitive posts behind an interaction?

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

        for tag in post_group.meta.tags.iter() {
            *tags.entry(tag.clone()).or_insert(0usize) += 1;
        }
        post_groups.push(post_group.clone());
        for tag in post_group.meta.tags.iter() {
            if SETTINGS.interesting_tags.contains(tag) {
                post_groups_with_interesting_tags.push(post_group.clone());
                post_groups_by_interesting_tag
                    .entry(tag.clone())
                    .or_insert(vec![])
                    .push(post_group.clone());
                break;
            }
        }

        // reader step: generate post page.
        let template = PostsPageTemplate {
            post_groups: vec![post_group.clone()],
        };
        let path = output_path.join(filename);
        info!("writing post page: {path:?}");
        writeln!(File::create(path)?, "{}", template.render()?)?;
    }

    post_groups.sort_by(PostGroup::reverse_chronological);
    post_groups_with_interesting_tags.sort_by(PostGroup::reverse_chronological);
    for (_, post_groups) in post_groups_by_interesting_tag.iter_mut() {
        post_groups.sort_by(PostGroup::reverse_chronological);
    }
    trace!("post groups by tag: {post_groups_by_interesting_tag:#?}");

    // author step: generate atom feed.
    let template = AtomFeedTemplate {
        post_groups: post_groups.clone(),
    };
    let atom_feed_path = output_path.join("feed.xml");
    writeln!(File::create(atom_feed_path)?, "{}", template.render()?)?;

    let mut tags = tags.into_iter().collect::<Vec<_>>();
    tags.sort_by(|p, q| p.1.cmp(&q.1).reverse().then(p.0.cmp(&q.0)));
    info!("all tags: {tags:?}");
    info!(
        "interesting tags: {:?}",
        tags.iter()
            .filter(|(tag, _)| SETTINGS.interesting_tags.contains(tag))
            .collect::<Vec<_>>()
    );

    let interesting_tags_filenames = SETTINGS
        .interesting_tags
        .iter()
        .map(|tag| format!("{tag}.html"));
    let interesting_tags_posts_filenames = post_groups_with_interesting_tags
        .iter()
        .map(|post_group| post_group.href.clone());
    let interesting_filenames = interesting_tags_filenames
        .chain(interesting_tags_posts_filenames)
        .map(|filename| format!("'{}'", filename.replace("'", "'\\''")))
        .collect::<Vec<_>>()
        .join(" ");
    info!(
        "filenames reachable from interesting tags only: {}",
        interesting_filenames
    );

    // reader step: generate posts pages.
    let template = PostsPageTemplate { post_groups };
    let posts_page_path = output_path.join("all.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    let template = PostsPageTemplate {
        post_groups: post_groups_with_interesting_tags,
    };
    let posts_page_path = output_path.join("index.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    for (tag, post_groups) in post_groups_by_interesting_tag.into_iter() {
        let template = PostsPageTemplate { post_groups };
        let posts_page_path = output_path.join(format!("{tag}.html"));
        writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    }

    Ok(())
}
