use jane_eyre::eyre;

#[derive(clap::Args, Debug)]
pub struct Db {}

pub async fn main(_args: Db) -> eyre::Result<()> {
    Ok(())
}
