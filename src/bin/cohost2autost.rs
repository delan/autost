use std::{
    env::args,
    fs::{read_dir, File},
    io::Write,
    path::Path,
};

use ammonia::clean_text;
use autost::cohost::Post;
use jane_eyre::eyre::{self, OptionExt};
use tracing::{info, warn};
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

    for entry in read_dir(input_path)? {
        let entry = entry?;
        let input_path = entry.path();
        let output_name = entry.file_name();
        let output_name = output_name.to_str().ok_or_eyre("Unsupported file name")?;
        let Some(output_name) = output_name.strip_suffix(".json") else {
            continue;
        };
        let output_path = output_path.join(format!("{output_name}.md"));

        let post: Post = serde_json::from_reader(File::open(&input_path)?)?;

        // TODO: handle shares.
        if post.transparentShareOfPostId.is_some() || post.shareOfPostId.is_some() {
            warn!("TODO: skipping share post {}", post.postId);
            continue;
        }

        // TODO: handle attachments (`.blocks[] | select(.type == "attachment")`).
        info!("Converting {input_path:?} -> {output_path:?}");
        let mut output = File::create(output_path)?;
        let title = clean_text(&post.headline);
        output.write_all(format!(r#"<meta name="title" content="{title}">"#).as_bytes())?;
        output.write_all(b"\n\n")?;
        output.write_all(post.plainTextBody.as_bytes())?;
    }

    Ok(())
}
