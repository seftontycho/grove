use anyhow::{bail, Result};
use dialoguer::FuzzySelect;

use crate::config::Config;
use crate::db::{Db, Repo, RepoFilter, RepoStatus};
use crate::git;
use crate::multiplexer::{Multiplexer, SessionName};

/// What the user picked (interactively or via positional arg). Carries
/// enough information to dispatch to the right `git::WorktreeSource`.
#[derive(Debug, Clone)]
pub enum BranchSource {
    /// Existing local branch — open it as-is, no reset.
    Local(String),
    /// Remote-tracking ref. `local` is the branch to create; `upstream` is
    /// the full remote ref (e.g. "origin/feat").
    Remote { local: String, upstream: String },
    /// Brand-new branch from HEAD. The String is the name to create.
    New(String),
}

impl BranchSource {
    /// The local branch name this source produces, for session naming and
    /// worktree path resolution.
    pub fn branch_name(&self) -> &str {
        match self {
            BranchSource::Local(name) => name,
            BranchSource::Remote { local, .. } => local,
            BranchSource::New(name) => name,
        }
    }
}

/// Build the picker's display list from local + remote branch lists.
///
/// Output order: `[new]` first, then locals sorted alphabetically, then
/// remotes sorted alphabetically. Any remote whose branch part shadows a
/// local entry is omitted.
fn build_picker_entries(
    locals: &[String],
    remote_refs: &[String],
) -> Vec<(String, BranchSource)> {
    let mut entries: Vec<(String, BranchSource)> = Vec::new();

    entries.push((
        "[new]    + create new branch".to_string(),
        // Placeholder name; the caller prompts for the real one if this
        // entry is selected.
        BranchSource::New(String::new()),
    ));

    let mut sorted_locals: Vec<&String> = locals.iter().collect();
    sorted_locals.sort();
    for name in &sorted_locals {
        entries.push((
            format!("[local]  {}", name),
            BranchSource::Local((*name).clone()),
        ));
    }

    let local_set: std::collections::HashSet<&str> =
        locals.iter().map(String::as_str).collect();

    let mut sorted_remotes: Vec<&String> = remote_refs.iter().collect();
    sorted_remotes.sort();
    for remote_ref in &sorted_remotes {
        let Some((_remote, branch)) = remote_ref.split_once('/') else {
            // Defensive: list_remote_branches already filters these.
            continue;
        };
        if local_set.contains(branch) {
            continue;
        }
        entries.push((
            format!("[remote] {}", remote_ref),
            BranchSource::Remote {
                local: branch.to_string(),
                upstream: (*remote_ref).clone(),
            },
        ));
    }

    entries
}

pub fn run(
    db: &Db,
    config: &Config,
    mux: &dyn Multiplexer,
    query: Option<&str>,
    branch: Option<&str>,
) -> Result<()> {
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

    // If a session already exists, attach to it instead of creating a new one.
    // Each backend uses its own name format for the lookup.
    let sessions = mux.list_sessions()?;
    let exists = sessions
        .iter()
        .any(|s| s.name == session.as_zellij_name() || s.name == session.as_tmux_name());

    if exists {
        // Determine which name format the session was stored under.
        let name = if sessions.iter().any(|s| s.name == session.as_zellij_name()) {
            session.as_zellij_name()
        } else {
            session.as_tmux_name()
        };
        println!("Session '{session}' already exists, attaching...");
        return mux.attach_session(&name);
    }

    // Create worktree (or reuse if it already exists).
    let worktree_path = match find_existing_worktree(&repo, &branch_name)? {
        Some(path) => {
            println!("Reusing existing worktree at {}", path.display());
            path
        }
        None => {
            let path = git::worktree_add(&repo.path, &branch_name, git::WorktreeSource::NewFromHead)?;
            println!("Created worktree at {}", path.display());
            path
        }
    };

    println!("Starting session '{session}'...");
    mux.create_session(&session, &worktree_path, &config.shell.to_string())?;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(entries: &[(String, BranchSource)]) -> Vec<&str> {
        entries.iter().map(|(l, _)| l.as_str()).collect()
    }

    #[test]
    fn build_picker_entries_orders_new_then_locals_then_remotes() {
        let entries = build_picker_entries(
            &["master".to_string(), "feat/auth".to_string()],
            &["origin/main".to_string(), "origin/release/1.2".to_string()],
        );

        assert_eq!(
            labels(&entries),
            vec![
                "[new]    + create new branch",
                "[local]  feat/auth",
                "[local]  master",
                "[remote] origin/main",
                "[remote] origin/release/1.2",
            ]
        );
    }

    #[test]
    fn build_picker_entries_hides_remotes_shadowed_by_locals() {
        let entries = build_picker_entries(
            &["feat".to_string()],
            &["origin/feat".to_string(), "origin/main".to_string()],
        );

        let labels = labels(&entries);
        assert!(labels.contains(&"[local]  feat"));
        assert!(labels.contains(&"[remote] origin/main"));
        assert!(
            !labels.iter().any(|l| l.contains("origin/feat")),
            "origin/feat must be hidden by local feat"
        );
    }

    #[test]
    fn build_picker_entries_keeps_multiple_remotes_for_same_branch_when_no_local() {
        let entries = build_picker_entries(
            &[],
            &["origin/feat".to_string(), "upstream/feat".to_string()],
        );

        let labels = labels(&entries);
        assert!(labels.contains(&"[remote] origin/feat"));
        assert!(labels.contains(&"[remote] upstream/feat"));
    }

    #[test]
    fn build_picker_entries_splits_remote_on_first_slash_only() {
        let entries = build_picker_entries(&[], &["origin/release/1.2".to_string()]);

        let (label, source) = entries
            .iter()
            .find(|(l, _)| l.starts_with("[remote]"))
            .expect("remote entry");
        assert_eq!(label, "[remote] origin/release/1.2");
        match source {
            BranchSource::Remote { local, upstream } => {
                assert_eq!(local, "release/1.2");
                assert_eq!(upstream, "origin/release/1.2");
            }
            other => panic!("expected Remote, got {other:?}"),
        }
    }

    #[test]
    fn build_picker_entries_returns_new_only_when_no_branches() {
        let entries = build_picker_entries(&[], &[]);
        assert_eq!(labels(&entries), vec!["[new]    + create new branch"]);
        assert!(matches!(entries[0].1, BranchSource::New(_)));
    }
}
