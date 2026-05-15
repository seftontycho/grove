use anyhow::{bail, Result};

use crate::cmd::clone::select_directory;
use crate::config::Config;
use crate::db::{Db, NewRepo};
use crate::git;

/// Default branch for newly created repositories.
const DEFAULT_BRANCH: &str = "master";

pub fn new(db: &Db, config: &Config, name: &str, dir: Option<&str>) -> Result<()> {
    if config.directories.is_empty() {
        bail!(
            "No directories configured. Add directories to your config file:\n  {}",
            Config::path()?.display()
        );
    }

    let dir_name = match dir {
        Some(d) => d.to_string(),
        None => select_directory(config)?,
    };

    let parent = config
        .resolve_dir(&dir_name)
        .ok_or_else(|| anyhow::anyhow!("Directory '{dir_name}' not found in config"))?;

    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }

    let container = parent.join(name);
    git::init_repo(&container, DEFAULT_BRANCH)?;

    db.add_repo(&NewRepo {
        name,
        path: &container,
        url: None,
        directory: Some(&dir_name),
    })?;

    println!("Created repo '{name}' at {}", container.display());
    println!("Open it with: grove open {name} {DEFAULT_BRANCH}");
    Ok(())
}
