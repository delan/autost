use autost::{
    cli_init,
    command::{
        self,
        attach::Attach,
        cohost2autost::Cohost2autost,
        cohost2json::Cohost2json,
        cohost_archive::CohostArchive,
        import::{Import, Reimport},
        new::New,
        render::Render,
        server::Server,
    },
    RunDetails, SETTINGS,
};
use clap::Parser;
use jane_eyre::eyre;
use tracing::info;

#[derive(clap::Parser, Debug)]
enum Command {
    Attach(Attach),
    Cohost2autost(Cohost2autost),
    Cohost2json(Cohost2json),
    CohostArchive(CohostArchive),
    Import(Import),
    New(New),
    Reimport(Reimport),
    Render(Render),
    Server(Server),
    Server2(Server),
}

#[rocket::main]
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
        // fail fast if there are any settings errors.
        let _ = &*SETTINGS;
    }

    match command {
        Command::Attach(args) => command::attach::main(args).await,
        Command::Cohost2autost(args) => command::cohost2autost::main(args),
        Command::Cohost2json(args) => command::cohost2json::main(args).await,
        Command::CohostArchive(args) => command::cohost_archive::main(args).await,
        Command::Import(args) => command::import::main(args).await,
        Command::New(args) => command::new::main(args),
        Command::Reimport(args) => command::import::reimport(args).await,
        Command::Render(args) => command::render::main(args),
        Command::Server(args) => command::server::main(args).await,
        Command::Server2(args) => command::server2::main(args).await,
    }
}
