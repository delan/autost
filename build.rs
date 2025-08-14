use std::error::Error;

use vergen_gix::{Emitter, GixBuilder};

fn main() -> Result<(), Box<dyn Error>> {
    let mut gix = GixBuilder::default();
    gix.describe(false, true, None);
    Emitter::default().add_instructions(&gix.build()?)?.emit()?;

    // sqlx migrations (see src/migrations.rs)
    println!("cargo:rerun-if-changed=migrations");

    Ok(())
}
