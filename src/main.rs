use std::{env::args, fs::File, io::Read};

use comrak::Options;

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
        .clean(&unsafe_html)
        .to_string();
    println!("{}", safe_html);

    Ok(())
}
