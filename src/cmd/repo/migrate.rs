use anyhow::{bail, Result};
use dialoguer::{Confirm, FuzzySelect};

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
    // path is renamed during migration.
    let cwd = std::env::current_dir()?;
    if cwd.starts_with(&repo.path) {
        bail!(
            "Cannot migrate '{}' while inside it ({}). cd elsewhere and retry.",
            repo.name,
            repo.path.display()
        );
    }

    // Refuse if any worktree has uncommitted changes to tracked files.
    let worktrees = git::worktree_list(&repo.path)?;
    let non_bare: Vec<_> = worktrees.iter().filter(|w| !w.is_bare).collect();

    let mut dirty = Vec::new();
    for wt in &non_bare {
        if git::has_uncommitted_tracked_changes(&wt.path)? {
            dirty.push(wt.path.clone());
        }
    }
    if !dirty.is_empty() {
        eprintln!(
            "Cannot migrate '{}': worktrees have uncommitted changes:",
            repo.name
        );
        for p in &dirty {
            eprintln!("  {}", p.display());
        }
        bail!("Commit or stash these changes, then retry");
    }

    let proceed = Confirm::new()
        .with_prompt(format!(
            "Migrating '{}' discards {} worktree(s) (branches are kept; \
             recreate worktrees with `grove open`). Continue?",
            repo.name,
            non_bare.len()
        ))
        .default(false)
        .interact()?;
    if !proceed {
        println!("Migration cancelled");
        return Ok(());
    }

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
