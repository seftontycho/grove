use anyhow::{bail, Result};
use std::path::PathBuf;
use tabled::{Table, Tabled};

use crate::db::{Db, NewRepo, Repo, RepoFilter};
use crate::git;

#[derive(Tabled)]
struct RepoRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Dir")]
    dir: String,
    #[tabled(rename = "Score")]
    score: String,
    #[tabled(rename = "Path")]
    path: String,
}

impl From<&Repo> for RepoRow {
    fn from(repo: &Repo) -> Self {
        Self {
            name: repo.name.clone(),
            dir: repo.directory.as_deref().unwrap_or("-").to_string(),
            score: format!("{:.0}", repo.frecency),
            path: repo.path.display().to_string(),
        }
    }
}

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

    let rows: Vec<RepoRow> = repos.iter().map(RepoRow::from).collect();
    let table = Table::new(rows);
    println!("{table}");

    Ok(())
}
