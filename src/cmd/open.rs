use anyhow::{bail, Result};
use dialoguer::FuzzySelect;

use crate::config::Config;
use crate::db::{Db, Repo, RepoFilter, RepoStatus};
use crate::git;
use crate::zellij::{self, SessionName};

pub fn run(db: &Db, config: &Config, query: Option<&str>, branch: Option<&str>) -> Result<()> {
    let repo = match query {
        Some(q) => match db.find_repo(q)? {
            Some(r) => r,
            None => bail!("No repo found matching '{q}'"),
        },
        None => select_repo(db)?,
    };

    db.touch_repo(repo.id)?;

    let branch_name = match branch {
        Some(b) => b.to_string(),
        None => select_or_create_branch(&repo)?,
    };

    let session = SessionName::new(&repo.name, &branch_name);

    // If a zellij session already exists, attach to it instead of creating
    if zellij::session_exists(&session.as_string())? {
        println!("Session '{session}' already exists, attaching...");
        return zellij::attach_session(&session.as_string());
    }

    // Create worktree (or reuse if it already exists)
    let worktree_path = match find_existing_worktree(&repo, &branch_name)? {
        Some(path) => {
            println!("Reusing existing worktree at {}", path.display());
            path
        }
        None => {
            let path = git::worktree_add(&repo.path, &branch_name)?;
            println!("Created worktree at {}", path.display());
            path
        }
    };

    println!("Starting zellij session '{session}'...");
    zellij::create_session(&session, &worktree_path, &config.shell.to_string())?;

    Ok(())
}

/// Check if a worktree for this branch already exists.
fn find_existing_worktree(repo: &Repo, branch: &str) -> Result<Option<std::path::PathBuf>> {
    let worktrees = git::worktree_list(&repo.path)?;
    let needle = format!("refs/heads/{branch}");

    let found = worktrees
        .iter()
        .find(|wt| wt.branch.as_deref() == Some(&needle) && !wt.is_bare);

    Ok(found.map(|wt| wt.path.clone()))
}

fn select_repo(db: &Db) -> Result<Repo> {
    let repos = db.list_repos(RepoFilter {
        status: Some(RepoStatus::Active),
        ..Default::default()
    })?;

    if repos.is_empty() {
        bail!("No repos tracked. Use 'grove clone' or 'grove repo add' first.");
    }

    let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();

    let selection = FuzzySelect::new()
        .with_prompt("Select a repo")
        .items(&names)
        .interact()?;

    Ok(repos[selection].clone())
}

fn select_or_create_branch(repo: &Repo) -> Result<String> {
    let mut branches = git::list_remote_branches(&repo.path).unwrap_or_default();

    let create_new = "[create new branch]".to_string();
    branches.insert(0, create_new.clone());

    let selection = FuzzySelect::new()
        .with_prompt("Select or create a branch")
        .items(&branches)
        .interact()?;

    if branches[selection] == create_new {
        let name: String = dialoguer::Input::new()
            .with_prompt("New branch name")
            .interact_text()?;
        Ok(name)
    } else {
        // Strip remote prefix (e.g., "origin/feat" -> "feat")
        let branch = branches[selection]
            .split('/')
            .skip(1)
            .collect::<Vec<_>>()
            .join("/");
        if branch.is_empty() {
            Ok(branches[selection].clone())
        } else {
            Ok(branch)
        }
    }
}
