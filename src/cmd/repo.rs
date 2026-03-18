use anyhow::{bail, Result};
use std::path::PathBuf;

use crate::db::{Db, NewRepo, RepoFilter};
use crate::git;

pub fn add(db: &Db, path: &str) -> Result<()> {
    let path = PathBuf::from(path)
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("Path does not exist: {path}"))?;

    // Verify it's a git repo (bare or normal)
    let worktrees = git::worktree_list(&path);
    if worktrees.is_err() {
        bail!("{} does not appear to be a git repository", path.display());
    }

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Could not determine repo name from path"))?;

    let repo = db.add_repo(&NewRepo {
        name,
        path: &path,
        url: None,
        directory: None,
    })?;

    println!("Tracking '{}' at {}", repo.name, repo.path.display());
    Ok(())
}

pub fn rm(db: &Db, name: &str) -> Result<()> {
    if db.remove_repo(name)? {
        println!("Removed '{name}' from tracking");
    } else {
        bail!("No repo found with name '{name}'");
    }
    Ok(())
}

pub fn list(db: &Db) -> Result<()> {
    let repos = db.list_repos(RepoFilter::default())?;

    if repos.is_empty() {
        println!("No repos tracked. Use 'grove clone' or 'grove repo add' to get started.");
        return Ok(());
    }

    for repo in &repos {
        let dir_label = repo
            .directory
            .as_deref()
            .map(|d| format!(" [{d}]"))
            .unwrap_or_default();
        println!(
            "{}{dir_label}  {}  (score: {:.0})",
            repo.name,
            repo.path.display(),
            repo.frecency
        );
    }

    Ok(())
}
