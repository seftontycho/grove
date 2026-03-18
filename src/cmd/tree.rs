use anyhow::{bail, Result};
use dialoguer::FuzzySelect;

use crate::db::{Db, Repo, RepoFilter, RepoStatus};
use crate::git;
use crate::zellij::{self, SessionName};

pub fn list(db: &Db, repo_query: Option<&str>) -> Result<()> {
    let repo = resolve_repo(db, repo_query)?;
    let worktrees = git::worktree_list(&repo.path)?;

    if worktrees.is_empty() {
        println!("No worktrees for '{}'", repo.name);
        return Ok(());
    }

    for wt in &worktrees {
        let branch_label = wt.branch.as_deref().unwrap_or("(detached)");
        let bare_label = if wt.is_bare { " [bare]" } else { "" };
        println!(
            "  {branch_label}{bare_label}  {}  {}",
            wt.head.get(..8).unwrap_or(&wt.head),
            wt.path.display()
        );
    }

    Ok(())
}

pub fn close(db: &Db, query: Option<&str>) -> Result<()> {
    let repo = resolve_repo(db, query)?;
    let worktrees = git::worktree_list(&repo.path)?;

    let non_bare: Vec<_> = worktrees.iter().filter(|wt| !wt.is_bare).collect();

    if non_bare.is_empty() {
        println!("No worktrees to close for '{}'", repo.name);
        return Ok(());
    }

    let labels: Vec<String> = non_bare
        .iter()
        .map(|wt| wt.branch.as_deref().unwrap_or("(detached)").to_string())
        .collect();

    let selection = FuzzySelect::new()
        .with_prompt("Select worktree to close")
        .items(&labels)
        .interact()?;

    let wt = non_bare[selection];
    let branch = labels[selection].clone();

    // Kill zellij session if it exists
    let session_name = SessionName::new(&repo.name, &branch);
    let _ = zellij::kill_session(&session_name.as_string());

    git::worktree_remove(&repo.path, &wt.path)?;
    println!("Closed worktree '{branch}' for '{}'", repo.name);

    Ok(())
}

pub fn prune(db: &Db, repo_query: Option<&str>) -> Result<()> {
    let repo = resolve_repo(db, repo_query)?;
    git::worktree_prune(&repo.path)?;
    println!("Pruned stale worktrees for '{}'", repo.name);
    Ok(())
}

fn resolve_repo(db: &Db, query: Option<&str>) -> Result<Repo> {
    match query {
        Some(q) => db
            .find_repo(q)?
            .ok_or_else(|| anyhow::anyhow!("No repo found matching '{q}'")),
        None => {
            let repos = db.list_repos(RepoFilter {
                status: Some(RepoStatus::Active),
                ..Default::default()
            })?;

            if repos.is_empty() {
                bail!("No repos tracked.");
            }

            let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
            let selection = FuzzySelect::new()
                .with_prompt("Select a repo")
                .items(&names)
                .interact()?;

            Ok(repos[selection].clone())
        }
    }
}
