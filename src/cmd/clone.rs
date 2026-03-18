use anyhow::{bail, Result};
use dialoguer::FuzzySelect;

use crate::config::Config;
use crate::db::{Db, NewRepo};
use crate::git;

pub fn run(db: &Db, config: &Config, url: &str, dir: Option<&str>) -> Result<()> {
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

    println!(
        "Cloning {url} into {}/{}",
        parent.display(),
        git::repo_name_from_url(url)?
    );

    let result = git::clone_bare(url, parent)?;

    db.add_repo(&NewRepo {
        name: &result.name,
        path: &result.path,
        url: Some(url),
        directory: Some(&dir_name),
    })?;

    println!(
        "Tracked repo '{}' at {}",
        result.name,
        result.path.display()
    );

    Ok(())
}

fn select_directory(config: &Config) -> Result<String> {
    let names = config.dir_names();
    if names.len() == 1 {
        return Ok(names[0].to_string());
    }

    let selection = FuzzySelect::new()
        .with_prompt("Clone to which directory?")
        .items(&names)
        .interact()?;

    Ok(names[selection].to_string())
}
