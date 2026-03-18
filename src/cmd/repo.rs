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

    // Calculate column widths
    let name_width = repos.iter().map(|r| r.name.len()).max().unwrap_or(0);
    let dir_width = repos
        .iter()
        .map(|r| r.directory.as_deref().unwrap_or("-").len())
        .max()
        .unwrap_or(0);
    let score_width = repos
        .iter()
        .map(|r| format!("{:.0}", r.frecency).len())
        .max()
        .unwrap_or(0);

    // Header
    println!(
        "{:<name_width$}  {:<dir_width$}  {:>score_width$}  {}",
        "NAME", "DIR", "SCORE", "PATH",
    );
    println!(
        "{:<name_width$}  {:<dir_width$}  {:>score_width$}  {}",
        "-".repeat(name_width),
        "-".repeat(dir_width),
        "-".repeat(score_width),
        "----",
    );

    for repo in &repos {
        let dir = repo.directory.as_deref().unwrap_or("-");
        println!(
            "{:<name_width$}  {:<dir_width$}  {:>score_width$.0}  {}",
            repo.name,
            dir,
            repo.frecency,
            repo.path.display(),
        );
    }

    Ok(())
}
