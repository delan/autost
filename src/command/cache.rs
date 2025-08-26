use jane_eyre::eyre;

#[derive(clap::Subcommand, Debug)]
pub enum Cache {
    Test(Test),
}

#[derive(clap::Args, Debug)]
pub struct Test {
    #[arg(long)]
    pub use_packs: bool,
}

pub async fn main(args: Cache) -> eyre::Result<()> {
    match args {
        Cache::Test(test) => crate::cache::test(test).await,
    }
}
