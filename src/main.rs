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

    let db = if matches!(
        command,
        Command::Attach { .. }
            | Command::Cohost2autost { .. }
            // | Command::Db { .. }
            | Command::Import { .. }
            | Command::Reimport { .. }
            | Command::Render { .. }
            | Command::Server { .. }
    ) {
        // fail fast if there are any settings or migration errors.
        let _ = &*SETTINGS;
        Some(run_migrations().await?)
    } else {
        None
    };

    match command {
        Command::Attach(_) => command::attach::main().await,
        Command::Cohost2autost(args) => command::cohost2autost::main(args),
        Command::Cohost2json(_) => command::cohost2json::main().await,
        Command::CohostArchive(_) => command::cohost_archive::main().await,
        Command::Cache(args) => command::cache::main(args).await,
        Command::Db(args) => command::db::main(args).await,
        Command::Import(_) => command::import::main().await,
        Command::New(args) => command::new::main(args),
        Command::Reimport(_) => command::import::reimport::main().await,
        Command::Render(args) => {
            command::render::main(args, db.expect("guaranteed by definition")).await
        }
        Command::Server(_) => command::server::main(db.expect("guaranteed by definition")).await,
    }
}
