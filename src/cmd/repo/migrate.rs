use anyhow::{bail, Result};
use dialoguer::FuzzySelect;

use crate::db::{Db, Repo, RepoFilter};
use crate::git::{self, RepoLayout};

pub fn migrate(db: &Db, name: Option<&str>) -> Result<()> {
    let repo = resolve_repo(db, name)?;

    // Already migrated?
    if let Ok(RepoLayout::Container { .. }) = RepoLayout::detect(&repo.path) {
        println!("'{}' is already in the container layout", repo.name);
        return Ok(());
    }

    // Refuse if the shell is currently inside the repo being migrated — its
    // path is renamed during migration. Canonicalize so the comparison is
    // reliable even when repo.path was stored non-canonically (e.g. via a
    // relative or symlinked config directory).
    let cwd = std::env::current_dir()?;
    let repo_path = repo
        .path
        .canonicalize()
        .unwrap_or_else(|_| repo.path.clone());
    if cwd.starts_with(&repo_path) {
        bail!(
            "Cannot migrate '{}' while inside it ({}). \
             Run this from outside the repo.",
            repo.name,
            repo.path.display()
        );
    }

    // Migration is non-destructive: worktrees (and their uncommitted changes)
    // are relocated into the new layout, not discarded.
    git::migrate_to_container(&repo.path)?;
    println!(
        "Migrated '{}' to the container layout at {}",
        repo.name,
        repo.path.display()
    );
    Ok(())
}

fn resolve_repo(db: &Db, name: Option<&str>) -> Result<Repo> {
    match name {
        Some(q) => db
            .find_repo(q)?
            .ok_or_else(|| anyhow::anyhow!("No repo found matching '{q}'")),
        None => {
            // No status filter: a legacy or inactive repo is still a valid
            // migration target, unlike `grove open` / `grove tree`.
            let repos = db.list_repos(RepoFilter::default())?;
            if repos.is_empty() {
                bail!("No repos tracked.");
            }
            let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
            let selection = FuzzySelect::new()
                .with_prompt("Select a repo to migrate")
                .items(&names)
                .interact()?;
            Ok(repos[selection].clone())
        }
    }
}
