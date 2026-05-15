use anyhow::{bail, Result};

use crate::db::Db;

pub fn rm(db: &Db, name: &str) -> Result<()> {
    if db.remove_repo(name)? {
        println!("Removed '{name}' from tracking");
    } else {
        bail!("No repo found with name '{name}'");
    }
    Ok(())
}
