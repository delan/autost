use atomic_write_file::{unix::OpenOptionsExt as _, AtomicWriteFile};
use jane_eyre::eyre;

use std::{io::Write as _, path::Path};

pub fn atomic_writer(path: impl AsRef<Path>) -> eyre::Result<AtomicWriteFile> {
    Ok(AtomicWriteFile::options()
        .preserve_mode(false)
        .preserve_owner(false)
        .try_preserve_owner(false)
        .open(path)?)
}

pub fn atomic_write(path: impl AsRef<Path>, content: impl AsRef<[u8]>) -> eyre::Result<()> {
    let mut file = atomic_writer(path)?;
    file.write_all(content.as_ref())?;
    file.commit()?;

    Ok(())
}
