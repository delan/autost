use autost::{
    cli_init,
    command::{self},
    migrations::run_migrations,
    Command, RunDetails, SETTINGS,
};
use clap::Parser;
use jane_eyre::eyre;
use tracing::info;

#[tokio::main]
async fn main() -> eyre::Result<()> {
    cli_init()?;

    let command = Command::parse();
    info!(run_details = ?RunDetails::default());

    if matches!(
        command,
        Command::Attach { .. }
            | Command::Cohost2autost { .. }
            | Command::Import { .. }
            | Command::Reimport { .. }
            | Command::Render { .. }
            | Command::Server { .. }
    ) {
        // fail fast if there are any settings or migration errors.
        let _ = &*SETTINGS;
        run_migrations().await?;
    }

    match command {
        Command::Attach(_) => command::attach::main().await,
        Command::Cohost2autost(args) => command::cohost2autost::main(args),
        Command::Cohost2json(_) => command::cohost2json::main().await,
        Command::CohostArchive(_) => command::cohost_archive::main().await,
        Command::Import(_) => command::import::main().await,
        Command::New(args) => command::new::main(args),
        Command::Reimport(_) => command::import::reimport::main().await,
        Command::Render(args) => command::render::main(args),
        Command::Server(_) => command::server::main().await,
    }
}
