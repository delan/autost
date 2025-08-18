use jane_eyre::eyre;

#[derive(clap::Subcommand, Debug)]
pub enum Cache {
    Test,
}

pub async fn main(_args: Cache) -> eyre::Result<()> {
    crate::cache::test().await
}
