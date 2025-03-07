use autost::{
    cli_init,
    command::{self},
    Command, RunDetails, SETTINGS,
};
use clap::Parser;
use jane_eyre::eyre;
use tracing::info;

fn main() -> eyre::Result<()> {
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
        // fail fast if there are any settings errors.
        let _ = &*SETTINGS;
    }

    match command {
        Command::Attach(_) => command::attach::main(),
        Command::Cohost2autost(args) => command::cohost2autost::main(args),
        Command::Cohost2json(_) => command::cohost2json::main(),
        Command::CohostArchive(_) => command::cohost_archive::main(),
        Command::Import(_) => command::import::main(),
        Command::New(args) => command::new::main(args),
        Command::Reimport(_) => command::import::reimport::main(),
        Command::Render(args) => command::render::main(args),
        Command::Server(_) => command::server::main(),
    }
}
