use std::env::args;

use autost::{cli_init, command, SETTINGS};
use jane_eyre::eyre::{self, bail};

#[tokio::main]
async fn main() -> eyre::Result<()> {
    cli_init()?;

    // fail fast if there are any settings errors.
    let _ = &*SETTINGS;

    let mut args = args();
    let command_name = args.nth(1);

    match command_name.as_deref() {
        Some("cohost2autost") => command::cohost2autost::main(args),
        Some("cohost2json") => command::cohost2json::main(args),
        Some("render") => command::render::main(args),
        Some("server") => command::server::main(args).await,
        _ => bail!("usage: autost <cohost2autost|cohost2json|render|server>"),
    }
}
