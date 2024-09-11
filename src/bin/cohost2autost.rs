use std::{
    env::args,
    fs::{read_dir, DirEntry, File},
    io::{Read, Write},
    path::Path,
};

use ammonia::clean_text;
use autost::cohost::{Attachment, Block, Post};
use jane_eyre::eyre::{self, eyre, Context, OptionExt};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use tracing::{info, trace, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

fn main() -> eyre::Result<()> {
    jane_eyre::install()?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let input_path = args().nth(1).unwrap();
    let input_path = Path::new(&input_path);
    let output_path = args().nth(2).unwrap();
    let output_path = Path::new(&output_path);
    let attachments_path = args().nth(3).unwrap();
    let attachments_path = Path::new(&attachments_path);
    let dir_entries = read_dir(input_path)?.collect::<Vec<_>>();

    let results = dir_entries
        .into_par_iter()
        .map(|entry| -> eyre::Result<()> {
            let entry = entry?;
            convert_chost(&entry, output_path, attachments_path)
                .wrap_err_with(|| eyre!("{:?}: failed to convert", entry.path()))?;
            Ok(())
        })
        .collect::<Vec<_>>();
    for result in results {
        result?;
    }

    Ok(())
}

fn convert_chost(
    entry: &DirEntry,
    output_path: &Path,
    attachments_path: &Path,
) -> eyre::Result<()> {
    let input_path = entry.path();
    let output_name = entry.file_name();
    let output_name = output_name.to_str().ok_or_eyre("unsupported file name")?;
    let Some(output_name) = output_name.strip_suffix(".json") else {
        return Ok(());
    };
    let output_path = output_path.join(format!("{output_name}.md"));

    trace!("{input_path:?}: parsing");
    let post: Post = serde_json::from_reader(File::open(&input_path)?)?;

    // TODO: handle shares.
    if post.transparentShareOfPostId.is_some() || post.shareOfPostId.is_some() {
        warn!("{input_path:?}: TODO: skipping share post {}", post.postId);
        return Ok(());
    }

    info!("{input_path:?}: converting -> {output_path:?}");
    let mut output = File::create(output_path)?;
    let title = clean_text(&post.headline);
    let published = clean_text(&post.publishedAt);
    let n = "\n";
    output.write_all(format!(r#"<meta name="title" content="{title}">{n}"#).as_bytes())?;
    output
        .write_all(format!(r#"<meta name="published" content="{published}">{n}{n}"#).as_bytes())?;
    for block in post.blocks {
        match block {
            Block::Markdown { markdown } => {
                output.write_all(format!("{}\n\n", markdown.content).as_bytes())?;
            }
            Block::Attachment { attachment } => match attachment {
                Attachment::Image {
                    attachmentId,
                    altText,
                    width,
                    height,
                } => {
                    let url = format!("https://cohost.org/rc/attachment-redirect/{attachmentId}");
                    let path = attachments_path.join(&attachmentId);
                    cached_get(&url, &path)?;
                    let src = clean_text(&format!("attachments/{attachmentId}"));
                    output.write_all(format!(r#"<img loading="lazy" src="{src}" alt="{altText}" width="{width}" height="{height}">{n}{n}"#).as_bytes())?;
                }
                Attachment::Unknown { fields } => {
                    warn!("{input_path:?}: unknown attachment kind: {fields:?}");
                }
            },
            Block::Unknown { fields } => {
                warn!("{input_path:?}: unknown block type: {fields:?}");
            }
        }
    }

    Ok(())
}

fn cached_get(url: &str, path: &Path) -> eyre::Result<Vec<u8>> {
    if let Ok(mut file) = File::open(path) {
        trace!("cache hit: {url}");
        let mut result = Vec::default();
        file.read_to_end(&mut result)?;
        return Ok(result);
    }

    trace!("cache miss: {url}");
    let result = reqwest::blocking::get(url)?.bytes()?.to_vec();
    File::create(path)?.write_all(&result)?;

    Ok(result)
}
