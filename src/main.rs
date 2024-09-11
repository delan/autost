use std::{env::args, fs::File, io::Read};

use askama::Template;
use comrak::Options;

#[derive(askama::Template)]
#[template(path = "post.html")]
struct PostTemplate<'input> {
    content: &'input str,
}

fn main() -> jane_eyre::eyre::Result<()> {
    jane_eyre::install()?;

    let path = args().nth(1).unwrap();
    let mut file = File::open(path)?;
    let mut markdown = String::default();
    file.read_to_string(&mut markdown)?;

    // author step: render markdown to html.
    let mut options = Options::default();
    options.render.unsafe_ = true;
    let unsafe_html = comrak::markdown_to_html(&markdown, &options);

    // reader step: filter html.
    let safe_html = ammonia::Builder::default()
        .add_generic_attributes(["style"])
        .add_tag_attributes("details", ["open"])
        .id_prefix(Some("user-content-")) // cohost compatibility
        .clean(&unsafe_html)
        .to_string();

    // reader step: generate post page.
    let template = PostTemplate {
        content: &safe_html,
    };
    println!("{}", template.render()?);

    Ok(())
}
