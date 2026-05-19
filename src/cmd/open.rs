use anyhow::{bail, Context, Result};
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

/// Resolve a positional `<branch>` argument to a `BranchSource`.
///
/// Resolution order:
/// 1. exact local match → `Local`
/// 2. exact remote-ref match → `Remote { local, upstream }`
/// 3. `<known-remote>/<rest>` with no matching ref → reject (typo guard)
/// 4. otherwise → `New`
fn resolve_branch_arg(repo: &Repo, arg: &str) -> Result<BranchSource> {
    // Non-interactive path: propagate errors so a corrupt repo surfaces as a
    // clear failure instead of silently falling through to "create new branch
    // named <arg>".
    let locals = git::list_local_branches(&repo.path)
        .context("Failed to list local branches while resolving branch arg")?;
    if locals.iter().any(|b| b == arg) {
        return Ok(BranchSource::Local(arg.to_string()));
    }

    let remote_refs = git::list_remote_branches(&repo.path)
        .context("Failed to list remote branches while resolving branch arg")?;
    if remote_refs.iter().any(|r| r == arg) {
        let (_remote, branch) = arg
            .split_once('/')
            .expect("remote ref always contains '/'");
        return Ok(BranchSource::Remote {
            local: branch.to_string(),
            upstream: arg.to_string(),
        });
    }

    if let Some((maybe_remote, _rest)) = arg.split_once('/') {
        let remotes = git::list_remotes(&repo.path)
            .context("Failed to list remotes while resolving branch arg")?;
        if remotes.iter().any(|r| r == maybe_remote) {
            bail!(
                "unknown branch '{arg}': '{maybe_remote}' is a remote but the ref does not exist. \
                 Did you mean to create '{arg}' as a new branch? It would nest under a directory \
                 named '{maybe_remote}' — refusing."
            );
        }
    }

    Ok(BranchSource::New(arg.to_string()))
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

    let branch_source = match branch {
        Some(b) => resolve_branch_arg(&repo, b)?,
        None => select_branch_source(&repo)?,
    };
    let branch_name = branch_source.branch_name().to_string();

    let session = SessionName::new(&repo.name, &branch_name);

    // If a session already exists, attach to it instead of creating a new one.
    let sessions = mux.list_sessions()?;
    let exists = sessions
        .iter()
        .any(|s| s.name == session.as_zellij_name() || s.name == session.as_tmux_name());

    if exists {
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
            let wt_source = match &branch_source {
                BranchSource::Local(_) => git::WorktreeSource::ExistingLocal,
                BranchSource::Remote { upstream, .. } => {
                    git::WorktreeSource::TrackingRemote {
                        upstream: upstream.clone(),
                    }
                }
                BranchSource::New(_) => git::WorktreeSource::NewFromHead,
            };
            let path = git::worktree_add(&repo.path, &branch_name, wt_source)?;
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

/// Interactive branch picker. Fetches, builds the unified list, returns
/// the user's selection as a `BranchSource`.
fn select_branch_source(repo: &Repo) -> Result<BranchSource> {
    if let Err(e) = git::fetch_all(&repo.path) {
        eprintln!("Warning: fetch failed: {e:#}");
    }

    let locals = git::list_local_branches(&repo.path).unwrap_or_default();
    let remote_refs = git::list_remote_branches(&repo.path).unwrap_or_default();
    let entries = build_picker_entries(&locals, &remote_refs);

    let labels: Vec<&str> = entries.iter().map(|(l, _)| l.as_str()).collect();
    let selection = FuzzySelect::new()
        .with_prompt("Select or create a branch")
        .items(&labels)
        .interact()?;

    let (_label, source) = entries[selection].clone();

    // For the "[new]" entry, prompt for the real name now.
    if matches!(source, BranchSource::New(_)) {
        let name: String = dialoguer::Input::new()
            .with_prompt("New branch name")
            .interact_text()?;
        return Ok(BranchSource::New(name));
    }

    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;

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

    /// Build a container repo with exactly the requested local branches and
    /// remote-tracking refs under `origin`. `master` is always present locally
    /// (created by `init_repo`). Returns the container path.
    fn make_test_repo(
        tmpdir: &std::path::Path,
        local_branches: &[&str],
        remote_branches: &[&str],
    ) -> std::path::PathBuf {
        let container = tmpdir.join("repo");
        git::init_repo(&container, "master").unwrap();
        let bare = container.join(".bare");

        let master_commit =
            git::run_git_capture(&["rev-parse", "refs/heads/master"], &bare).unwrap();

        for b in local_branches {
            git::run_git(
                &["update-ref", &format!("refs/heads/{b}"), &master_commit],
                &bare,
            )
            .unwrap();
        }

        // Configure a stub `origin` remote (URL is never fetched).
        git::run_git(
            &[
                "remote",
                "add",
                "origin",
                "https://example.invalid/source.git",
            ],
            &bare,
        )
        .unwrap();

        // Seed remote-tracking refs directly. `list_remote_branches` reads
        // these as `origin/<branch>`.
        for b in remote_branches {
            git::run_git(
                &[
                    "update-ref",
                    &format!("refs/remotes/origin/{b}"),
                    &master_commit,
                ],
                &bare,
            )
            .unwrap();
        }

        container
    }

    fn fake_repo(path: std::path::PathBuf) -> crate::db::Repo {
        crate::db::Repo {
            id: 1,
            name: "test".to_string(),
            path,
            url: None,
            directory: None,
            status: crate::db::RepoStatus::Active,
            frecency: 0.0,
            last_accessed_at: None,
            created_at: "1970-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn resolve_branch_arg_picks_local_first() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_test_repo(tmp.path(), &["feat"], &["feat"]);
        let repo = fake_repo(path);

        let src = resolve_branch_arg(&repo, "feat").unwrap();
        assert!(matches!(src, BranchSource::Local(ref n) if n == "feat"));
    }

    #[test]
    fn resolve_branch_arg_finds_remote_when_no_local() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_test_repo(tmp.path(), &[], &["feat"]);
        let repo = fake_repo(path);

        let src = resolve_branch_arg(&repo, "origin/feat").unwrap();
        match src {
            BranchSource::Remote { local, upstream } => {
                assert_eq!(local, "feat");
                assert_eq!(upstream, "origin/feat");
            }
            other => panic!("expected Remote, got {other:?}"),
        }
    }

    #[test]
    fn resolve_branch_arg_creates_new_for_unknown_name() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_test_repo(tmp.path(), &[], &[]);
        let repo = fake_repo(path);

        let src = resolve_branch_arg(&repo, "shiny").unwrap();
        assert!(matches!(src, BranchSource::New(ref n) if n == "shiny"));
    }

    #[test]
    fn resolve_branch_arg_rejects_unknown_remote_qualified_name() {
        let tmp = tempfile::tempdir().unwrap();
        let path = make_test_repo(tmp.path(), &[], &["feat"]);
        let repo = fake_repo(path);

        let err = resolve_branch_arg(&repo, "origin/typo").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("unknown branch"),
            "expected typo guard, got: {msg}"
        );
    }
}
