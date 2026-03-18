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

    // Also find orphaned zellij sessions (session exists but worktree is gone)
    let sessions = zellij::list_sessions()?;
    let session_prefix = format!("{}/", repo.name);
    let orphaned_sessions: Vec<&zellij::Session> = sessions
        .iter()
        .filter(|s| {
            s.name.starts_with(&session_prefix)
                && !non_bare.iter().any(|wt| {
                    let branch = extract_branch_name(wt.branch.as_deref());
                    let expected = SessionName::new(&repo.name, &branch);
                    s.name == expected.as_string()
                })
        })
        .collect();

    if non_bare.is_empty() && orphaned_sessions.is_empty() {
        println!(
            "No worktrees or orphaned sessions to close for '{}'",
            repo.name
        );
        return Ok(());
    }

    // Build selection list combining worktrees and orphaned sessions
    let mut labels: Vec<String> = Vec::new();
    let mut actions: Vec<CloseAction> = Vec::new();

    for wt in &non_bare {
        let branch = extract_branch_name(wt.branch.as_deref());
        labels.push(branch.clone());
        actions.push(CloseAction::WorktreeAndSession {
            branch,
            worktree_path: wt.path.clone(),
        });
    }

    for session in &orphaned_sessions {
        let label = format!("{} [orphaned session]", session.name);
        labels.push(label);
        actions.push(CloseAction::OrphanedSession {
            session_name: session.name.clone(),
        });
    }

    let selection = FuzzySelect::new()
        .with_prompt("Select worktree to close")
        .items(&labels)
        .interact()?;

    match &actions[selection] {
        CloseAction::WorktreeAndSession {
            branch,
            worktree_path,
        } => {
            // Kill zellij session (best-effort, may not exist)
            let session_name = SessionName::new(&repo.name, branch);
            let _ = zellij::kill_session(&session_name.as_string());

            // Remove worktree (force if directory is missing)
            match git::worktree_remove(&repo.path, worktree_path) {
                Ok(()) => {}
                Err(_) => {
                    // Worktree directory may already be gone, prune instead
                    eprintln!("Worktree directory missing, pruning stale entry...");
                    git::worktree_prune(&repo.path)?;
                }
            }

            println!("Closed worktree '{branch}' for '{}'", repo.name);
        }
        CloseAction::OrphanedSession { session_name } => {
            zellij::kill_session(session_name)?;
            println!("Killed orphaned session '{session_name}'");
        }
    }

    Ok(())
}

pub fn prune(db: &Db, repo_query: Option<&str>) -> Result<()> {
    let repo = resolve_repo(db, repo_query)?;
    git::worktree_prune(&repo.path)?;
    println!("Pruned stale worktrees for '{}'", repo.name);
    Ok(())
}

/// Actions that `close` can perform.
enum CloseAction {
    /// Normal case: both worktree and (possibly) session exist.
    WorktreeAndSession {
        branch: String,
        worktree_path: std::path::PathBuf,
    },
    /// Session exists but worktree is gone.
    OrphanedSession { session_name: String },
}

/// Extract a short branch name from a full ref or return a fallback.
fn extract_branch_name(branch_ref: Option<&str>) -> String {
    match branch_ref {
        Some(r) => r.strip_prefix("refs/heads/").unwrap_or(r).to_string(),
        None => "(detached)".to_string(),
    }
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
