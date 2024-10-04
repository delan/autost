use std::env::args;

use autost::{cli_init, command, SETTINGS};
use jane_eyre::eyre::{self, bail};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    cli_init()?;

    let mut args = args();
    let command_name = args.nth(1);

    if command_name.as_deref().map_or(false, |name| {
        ["attach", "cohost2autost", "import", "render", "server"].contains(&name)
    }) {
        // fail fast if there are any settings errors.
        let _ = &*SETTINGS;
    }

    match command_name.as_deref() {
        Some("attach") => command::attach::main(args).await,
        Some("cohost2autost") => command::cohost2autost::main(args),
        Some("cohost2json") => command::cohost2json::main(args),
        Some("import") => command::import::main(args).await,
        Some("new") => command::new::main(args),
        Some("render") => command::render::main(args),
        Some("server") => command::server::main(args).await,
        _ => bail!("usage: autost <attach|cohost2autost|cohost2json|import|new|render|server>"),
    }
}
