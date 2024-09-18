use std::{
    collections::BTreeMap,
    env::args,
    fs::{create_dir_all, File},
    io::Write,
    path::Path,
};

use askama::Template;
use autost::{cli_init, AtomFeedTemplate, TemplatedPost, Thread, ThreadsTemplate, SETTINGS};
use chrono::{SecondsFormat, Utc};
use jane_eyre::eyre::{self};
use tracing::{debug, info, trace};

fn main() -> eyre::Result<()> {
    cli_init()?;

    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let mut threads = vec![];
    let mut interesting_threads = vec![];
    let mut marked_interesting_threads = vec![];
    let mut excluded_threads = vec![];
    let mut skipped_own_threads = vec![];
    let mut skipped_other_threads = vec![];
    let mut threads_by_interesting_tag = BTreeMap::default();
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

        // TODO: skip threads with other authors?
        // TODO: skip threads with private or logged-in-only authors?
        // TODO: gate sensitive posts behind an interaction?

        let overall_title = posts
            .iter()
            .rev()
            .find(|post| !post.meta.is_transparent_share)
            .and_then(|post| post.meta.title.clone())
            .unwrap_or("".to_owned());

        let thread = Thread {
            href: filename.clone(),
            posts,
            meta,
            overall_title: overall_title.clone(),
        };

        for tag in thread.meta.tags.iter() {
            *tags.entry(tag.clone()).or_insert(0usize) += 1;
        }
        threads.push(thread.clone());
        if SETTINGS.thread_is_on_excluded_archived_list(&thread) {
            excluded_threads.push(thread.clone());
        } else if SETTINGS.thread_is_on_interesting_archived_list(&thread) {
            interesting_threads.push(thread.clone());
            marked_interesting_threads.push(thread.clone());
        } else {
            let mut was_interesting = false;
            for tag in thread.meta.tags.iter() {
                if SETTINGS.interesting_tags.contains(tag) {
                    interesting_threads.push(thread.clone());
                    threads_by_interesting_tag
                        .entry(tag.clone())
                        .or_insert(vec![])
                        .push(thread.clone());
                    was_interesting = true;
                    break;
                }
            }
            if !was_interesting {
                // if the thread had some input from us at publish time, that is, if the last post
                // was authored by us with content and/or tags...
                if thread.posts.last().is_some_and(|post| {
                    (!post.meta.is_transparent_share || !post.meta.tags.is_empty())
                        && post
                            .meta
                            .author
                            .as_ref()
                            .is_some_and(|author| SETTINGS.self_authors.contains(&author.href))
                }) {
                    skipped_own_threads.push(thread.clone());
                } else {
                    skipped_other_threads.push(thread.clone());
                }
            }
        }

        // reader step: generate post page.
        let template = ThreadsTemplate {
            threads: vec![thread.clone()],
            page_title: format!("{overall_title} — {}", SETTINGS.site_title),
            feed_href: None,
        };
        let path = output_path.join(filename);
        debug!("writing post page: {path:?}");
        writeln!(File::create(path)?, "{}", template.render()?)?;
    }

    threads.sort_by(Thread::reverse_chronological);
    interesting_threads.sort_by(Thread::reverse_chronological);
    marked_interesting_threads.sort_by(Thread::reverse_chronological);
    excluded_threads.sort_by(Thread::reverse_chronological);
    skipped_own_threads.sort_by(Thread::reverse_chronological);
    skipped_other_threads.sort_by(Thread::reverse_chronological);
    for (_, threads) in threads_by_interesting_tag.iter_mut() {
        threads.sort_by(Thread::reverse_chronological);
    }
    trace!("threads by tag: {threads_by_interesting_tag:#?}");
    let tagged_path = output_path.join("tagged");
    create_dir_all(&tagged_path)?;

    // author step: generate atom feeds.
    let template = AtomFeedTemplate {
        threads: interesting_threads.clone(),
        feed_title: SETTINGS.site_title.clone(),
        updated: now.clone(),
    };
    let atom_feed_path = output_path.join("index.feed.xml");
    writeln!(File::create(atom_feed_path)?, "{}", template.render()?)?;
    for (tag, threads) in threads_by_interesting_tag.clone().into_iter() {
        let template = AtomFeedTemplate {
            threads,
            feed_title: format!("{} — {tag}", SETTINGS.site_title),
            updated: now.clone(),
        };
        let atom_feed_path = tagged_path.join(format!("{tag}.feed.xml"));
        writeln!(File::create(atom_feed_path)?, "{}", template.render()?)?;
    }

    let mut tags = tags.into_iter().collect::<Vec<_>>();
    tags.sort_by(|p, q| p.1.cmp(&q.1).reverse().then(p.0.cmp(&q.0)));
    info!("all tags: {tags:?}");
    info!(
        "interesting tags: {:?}",
        tags.iter()
            .filter(|(tag, _)| SETTINGS.interesting_tags.contains(tag))
            .collect::<Vec<_>>()
    );
    info!("interesting threads: {}", interesting_threads.len());
    info!("own skipped threads: {}", skipped_own_threads.len());
    info!("others’ skipped threads: {}", skipped_other_threads.len());
    info!("all threads: {}", threads.len());

    let interesting_tags_filenames = SETTINGS.interesting_tags.iter().flat_map(|tag| {
        [
            format!("tagged/{tag}.feed.xml"),
            format!("tagged/{tag}.html"),
        ]
    });
    let interesting_tags_posts_filenames =
        interesting_threads.iter().map(|thread| thread.href.clone());
    let interesting_filenames = vec!["index.html".to_owned(), "index.feed.xml".to_owned()]
        .into_iter()
        .chain(interesting_tags_filenames)
        .chain(interesting_tags_posts_filenames)
        .map(|filename| format!("{}\n", filename))
        .collect::<Vec<_>>()
        .join("");
    if let Some(path) = &SETTINGS.interesting_output_filenames_list_path {
        File::create(path)?.write_all(interesting_filenames.as_bytes())?;
    }

    // reader step: generate internal posts pages.
    let template = ThreadsTemplate {
        threads,
        page_title: format!("all posts — {}", SETTINGS.site_title),
        feed_href: None,
    };
    let posts_page_path = output_path.join("all.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    let template = ThreadsTemplate {
        threads: excluded_threads,
        page_title: format!("excluded archived posts — {}", SETTINGS.site_title),
        feed_href: None,
    };
    let posts_page_path = output_path.join("excluded.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    let template = ThreadsTemplate {
        threads: marked_interesting_threads,
        page_title: format!(
            "archived posts that were marked interesting — {}",
            SETTINGS.site_title
        ),
        feed_href: None,
    };
    let posts_page_path = output_path.join("marked_interesting.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    let template = ThreadsTemplate {
        threads: skipped_own_threads,
        page_title: format!("own skipped archived posts — {}", SETTINGS.site_title),
        feed_href: None,
    };
    let posts_page_path = output_path.join("skipped_own.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    let template = ThreadsTemplate {
        threads: skipped_other_threads,
        page_title: format!("others’ skipped archived posts — {}", SETTINGS.site_title),
        feed_href: None,
    };
    let posts_page_path = output_path.join("skipped_other.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;

    // reader step: generate posts pages.
    let template = ThreadsTemplate {
        threads: interesting_threads,
        page_title: format!("posts — {}", SETTINGS.site_title),
        feed_href: Some("index.feed.xml".to_owned()),
    };
    let posts_page_path = output_path.join("index.html");
    writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    for (tag, threads) in threads_by_interesting_tag.into_iter() {
        let template = ThreadsTemplate {
            threads,
            page_title: format!("#{tag} — {}", SETTINGS.site_title),
            feed_href: Some(format!("tagged/{tag}.feed.xml")),
        };
        let posts_page_path = tagged_path.join(format!("{tag}.html"));
        writeln!(File::create(posts_page_path)?, "{}", template.render()?)?;
    }

    Ok(())
}
