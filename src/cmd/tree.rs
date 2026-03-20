use anyhow::{bail, Result};
use dialoguer::FuzzySelect;

use crate::db::{Db, Repo, RepoFilter, RepoStatus};
use crate::git;
use crate::multiplexer::{Multiplexer, Session, SessionName};

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

pub fn close(db: &Db, mux: &dyn Multiplexer, query: Option<&str>) -> Result<()> {
    let repo = resolve_repo(db, query)?;
    let worktrees = git::worktree_list(&repo.path)?;

    let non_bare: Vec<_> = worktrees.iter().filter(|wt| !wt.is_bare).collect();

    // Find orphaned sessions: a session exists for this repo but its worktree is gone.
    let all_sessions = mux.list_sessions()?;
    let orphaned_sessions: Vec<&Session> = all_sessions
        .iter()
        .filter(|s| {
            is_repo_session(&repo.name, &s.name)
                && !non_bare.iter().any(|wt| {
                    let branch = extract_branch_name(wt.branch.as_deref());
                    let sn = SessionName::new(&repo.name, &branch);
                    s.name == sn.as_zellij_name() || s.name == sn.as_tmux_name()
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

    // Build selection list combining worktrees and orphaned sessions.
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
            // Kill session (best-effort — may not exist).
            let sn = SessionName::new(&repo.name, branch);
            // Try both name formats; ignore errors since the session may not exist.
            let _ = mux.kill_session(&sn.as_zellij_name());
            let _ = mux.kill_session(&sn.as_tmux_name());

            // Remove worktree (force if directory is missing).
            match git::worktree_remove(&repo.path, worktree_path) {
                Ok(()) => {}
                Err(_) => {
                    eprintln!("Worktree directory missing, pruning stale entry...");
                    git::worktree_prune(&repo.path)?;
                }
            }

            println!("Closed worktree '{branch}' for '{}'", repo.name);
        }
        CloseAction::OrphanedSession { session_name } => {
            mux.kill_session(session_name)?;
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

/// Returns true if `session_name` belongs to `repo`, in either the zellij
/// (`repo/branch`) or tmux (`repo-branch`) naming convention.
fn is_repo_session(repo: &str, session_name: &str) -> bool {
    let zellij_prefix = format!("{}/", repo);
    let tmux_prefix = format!("{}-", repo);
    session_name.starts_with(&zellij_prefix) || session_name.starts_with(&tmux_prefix)
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
