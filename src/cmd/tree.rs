use anyhow::{bail, Context, Result};
use dialoguer::{Confirm, FuzzySelect};

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
        let stale_label = if is_stale(wt) { " [stale]" } else { "" };
        println!(
            "  {branch_label}{bare_label}{stale_label}  {}  {}",
            wt.head.get(..8).unwrap_or(&wt.head),
            wt.path.display()
        );
    }

    Ok(())
}

/// A worktree is stale when its working directory no longer exists on disk.
/// Bare entries point at the repo itself and are never considered stale.
fn is_stale(wt: &git::Worktree) -> bool {
    !wt.is_bare && !wt.path.exists()
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

pub fn prune(
    db: &Db,
    mux: &dyn Multiplexer,
    repo_query: Option<&str>,
    all: bool,
    orphans: bool,
    merged: bool,
) -> Result<()> {
    if all {
        let repos = db.list_repos(RepoFilter {
            status: Some(RepoStatus::Active),
            ..Default::default()
        })?;

        if repos.is_empty() {
            println!("No repos tracked.");
            return Ok(());
        }

        for repo in &repos {
            match prune_repo(mux, repo, orphans, merged) {
                Ok(outcome) => println!("{}", outcome.summary(&repo.name, orphans, merged)),
                Err(e) => eprintln!("Failed to prune '{}': {e}", repo.name),
            }
        }
        return Ok(());
    }

    let repo = resolve_repo(db, repo_query)?;
    let outcome = prune_repo(mux, &repo, orphans, merged)?;
    println!("{}", outcome.summary(&repo.name, orphans, merged));
    Ok(())
}

/// What a single repo's prune removed.
struct PruneOutcome {
    stale_worktrees: usize,
    orphan_dirs: usize,
    merged: MergedOutcome,
}

impl PruneOutcome {
    fn summary(&self, repo_name: &str, orphans: bool, merged: bool) -> String {
        let mut s = format!("Pruned {} stale worktree(s)", self.stale_worktrees);
        if orphans {
            s.push_str(&format!(", removed {} orphan dir(s)", self.orphan_dirs));
        }
        if merged {
            s.push_str(&format!(
                ", removed {} merged + {} gone worktree(s)",
                self.merged.merged_removed, self.merged.gone_removed
            ));
            if self.merged.kept_dirty > 0 {
                s.push_str(&format!(
                    ", kept {} (uncommitted changes)",
                    self.merged.kept_dirty
                ));
            }
        }
        s.push_str(&format!(" for '{repo_name}'"));
        s
    }
}

/// Prune stale worktrees for a single repo, killing the multiplexer session
/// for each one before removing git's records. When `orphans` is set, also
/// offer to delete leftover directories that have no registered worktree.
/// When `merged` is set, also offer to remove worktrees whose branch is done
/// (merged into the base branch or deleted upstream).
fn prune_repo(
    mux: &dyn Multiplexer,
    repo: &Repo,
    orphans: bool,
    merged: bool,
) -> Result<PruneOutcome> {
    let worktrees = git::worktree_list(&repo.path)?;
    let stale: Vec<_> = worktrees.iter().filter(|wt| is_stale(wt)).collect();

    for wt in &stale {
        let branch = extract_branch_name(wt.branch.as_deref());
        let sn = SessionName::new(&repo.name, &branch);
        // Best-effort: the session may not exist, and we try both name formats.
        let _ = mux.kill_session(&sn.as_zellij_name());
        let _ = mux.kill_session(&sn.as_tmux_name());
    }

    git::worktree_prune(&repo.path)?;

    let orphan_dirs = if orphans {
        remove_orphan_dirs(repo)?
    } else {
        0
    };

    let merged = if merged {
        prune_merged(mux, repo)?
    } else {
        MergedOutcome::default()
    };

    Ok(PruneOutcome {
        stale_worktrees: stale.len(),
        orphan_dirs,
        merged,
    })
}

/// Why a worktree's branch is considered "done".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoneReason {
    /// Branch is fully contained in the base branch.
    Merged,
    /// Branch tracked a remote branch that has since been deleted.
    Gone,
}

/// Counts from a merged-branch prune pass.
#[derive(Debug, Default)]
struct MergedOutcome {
    merged_removed: usize,
    gone_removed: usize,
    kept_dirty: usize,
}

/// Classify a worktree as a merged-branch prune candidate, given the
/// already-computed `merged`/`gone` facts for its branch. Bare entries,
/// detached worktrees, and the base branch's own worktree are never
/// candidates. `merged` (the higher-confidence signal) takes precedence
/// over `gone`.
fn classify_worktree(
    wt: &git::Worktree,
    base_branch: &str,
    merged: bool,
    gone: bool,
) -> Option<DoneReason> {
    if wt.is_bare {
        return None;
    }
    let branch = wt.branch.as_deref()?.strip_prefix("refs/heads/")?;
    if branch == base_branch {
        return None;
    }
    if merged {
        Some(DoneReason::Merged)
    } else if gone {
        Some(DoneReason::Gone)
    } else {
        None
    }
}

/// Find worktrees whose branch is merged into the base branch or whose
/// upstream was deleted, confirm with the user, then remove each one
/// (session + worktree + branch). Working-tree safety is delegated to
/// `git worktree remove` (without `--force`), which refuses any checkout
/// holding uncommitted or untracked files — so confirmed-but-dirty worktrees
/// are kept and reported rather than destroyed.
fn prune_merged(mux: &dyn Multiplexer, repo: &Repo) -> Result<MergedOutcome> {
    // `[gone]` is only accurate against fresh remote-tracking refs. Best-effort:
    // on failure (e.g. offline) the merged tier still works; only gone detection
    // may be stale.
    if let Err(e) = git::fetch_all(&repo.path) {
        eprintln!("Warning: fetch failed: {e:#}");
    }

    let base = git::resolve_base_branch(&repo.path)?;
    let worktrees = git::worktree_list(&repo.path)?;

    // Classify each worktree. Skip bare/detached and the base branch before
    // shelling out for the merge/gone facts.
    let mut candidates: Vec<(git::Worktree, String, DoneReason)> = Vec::new();
    for wt in worktrees {
        let Some(branch) = wt
            .branch
            .as_deref()
            .and_then(|b| b.strip_prefix("refs/heads/"))
            .map(str::to_string)
        else {
            continue;
        };
        if wt.is_bare || branch == base.name {
            continue;
        }

        let merged = git::is_branch_merged(&repo.path, &branch, &base.git_ref)?;
        // `merged` wins; only check for a gone upstream otherwise.
        let gone = !merged && git::branch_upstream_gone(&repo.path, &branch)?;

        if let Some(reason) = classify_worktree(&wt, &base.name, merged, gone) {
            candidates.push((wt, branch, reason));
        }
    }

    if candidates.is_empty() {
        return Ok(MergedOutcome::default());
    }

    // List grouped by tier, then confirm once for the whole set.
    let merged_branches: Vec<&str> = candidates
        .iter()
        .filter(|(_, _, r)| *r == DoneReason::Merged)
        .map(|(_, b, _)| b.as_str())
        .collect();
    let gone_branches: Vec<&str> = candidates
        .iter()
        .filter(|(_, _, r)| *r == DoneReason::Gone)
        .map(|(_, b, _)| b.as_str())
        .collect();

    if !merged_branches.is_empty() {
        println!("Merged into {} (safe to delete):", base.git_ref);
        for branch in &merged_branches {
            println!("  {branch}");
        }
    }
    if !gone_branches.is_empty() {
        println!("Upstream deleted — likely squash-merged, branch delete will be forced:");
        for branch in &gone_branches {
            println!("  {branch}");
        }
    }

    let confirmed = Confirm::new()
        .with_prompt(format!(
            "Delete the {} worktree(s) above (worktree + branch + session)?",
            candidates.len()
        ))
        .default(false)
        .interact()?;
    if !confirmed {
        println!("Skipped merged-branch removal.");
        return Ok(MergedOutcome::default());
    }

    let mut outcome = MergedOutcome::default();
    for (wt, branch, reason) in &candidates {
        // Best-effort: kill both session name forms.
        let sn = SessionName::new(&repo.name, branch);
        let _ = mux.kill_session(&sn.as_zellij_name());
        let _ = mux.kill_session(&sn.as_tmux_name());

        // Remove the worktree WITHOUT --force: git refuses dirty/untracked
        // checkouts, which is exactly the safety guarantee we want. Keep and
        // report those rather than forcing; do not touch their branch.
        if let Err(e) = git::worktree_remove(&repo.path, &wt.path) {
            eprintln!("  kept '{branch}': {e:#}");
            outcome.kept_dirty += 1;
            continue;
        }

        // Worktree gone → delete the branch. `-d` for merged (guaranteed to
        // succeed, since we proved it's an ancestor of base); `-D` for gone
        // (a squash-merged branch's commits aren't ancestors, so `-d` refuses).
        let force = matches!(reason, DoneReason::Gone);
        if let Err(e) = git::delete_branch(&repo.path, branch, force) {
            eprintln!("  removed worktree '{branch}' but branch delete failed: {e:#}");
        }

        match reason {
            DoneReason::Merged => outcome.merged_removed += 1,
            DoneReason::Gone => outcome.gone_removed += 1,
        }
    }

    // Tidy any records left dangling by the removals.
    git::worktree_prune(&repo.path)?;

    Ok(outcome)
}

/// Find leftover directories with no registered worktree, list them, and —
/// after explicit confirmation — delete them. Returns the number removed.
fn remove_orphan_dirs(repo: &Repo) -> Result<usize> {
    let dirs = git::orphan_worktree_dirs(&repo.path)?;
    if dirs.is_empty() {
        return Ok(0);
    }

    println!(
        "Orphan directories in '{}' (no registered worktree):",
        repo.name
    );
    for dir in &dirs {
        println!("  {}", dir.display());
    }

    let confirmed = Confirm::new()
        .with_prompt(format!("Delete the {} director(y/ies) above?", dirs.len()))
        .default(false)
        .interact()?;
    if !confirmed {
        println!("Skipped orphan directory removal.");
        return Ok(0);
    }

    let mut removed = 0;
    for dir in &dirs {
        std::fs::remove_dir_all(dir)
            .with_context(|| format!("Failed to remove {}", dir.display()))?;
        removed += 1;
    }
    Ok(removed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use git::Worktree;
    use std::path::PathBuf;

    fn worktree(path: PathBuf, is_bare: bool) -> Worktree {
        Worktree {
            path,
            head: "deadbeef".to_string(),
            branch: Some("refs/heads/main".to_string()),
            is_bare,
        }
    }

    #[test]
    fn existing_directory_is_not_stale() {
        let dir = tempfile::tempdir().unwrap();
        let wt = worktree(dir.path().to_path_buf(), false);
        assert!(!is_stale(&wt));
    }

    #[test]
    fn missing_directory_is_stale() {
        let wt = worktree(PathBuf::from("/no/such/grove/worktree"), false);
        assert!(is_stale(&wt));
    }

    #[test]
    fn bare_entry_is_never_stale() {
        // Bare points at the repo itself; even a missing path must not count.
        let wt = worktree(PathBuf::from("/no/such/grove/repo"), true);
        assert!(!is_stale(&wt));
    }

    /// A non-bare worktree on `branch`.
    fn branch_worktree(branch: &str) -> Worktree {
        Worktree {
            path: PathBuf::from(format!("/repo/{branch}")),
            head: "deadbeef".to_string(),
            branch: Some(format!("refs/heads/{branch}")),
            is_bare: false,
        }
    }

    #[test]
    fn classify_skips_bare() {
        let wt = worktree(PathBuf::from("/repo"), true);
        assert_eq!(classify_worktree(&wt, "main", true, true), None);
    }

    #[test]
    fn classify_skips_base_branch() {
        let wt = branch_worktree("main");
        assert_eq!(classify_worktree(&wt, "main", true, false), None);
    }

    #[test]
    fn classify_skips_detached() {
        let mut wt = branch_worktree("feat");
        wt.branch = None;
        assert_eq!(classify_worktree(&wt, "main", true, true), None);
    }

    #[test]
    fn classify_prefers_merged_over_gone() {
        let wt = branch_worktree("feat");
        assert_eq!(
            classify_worktree(&wt, "main", true, true),
            Some(DoneReason::Merged)
        );
    }

    #[test]
    fn classify_reports_gone_when_not_merged() {
        let wt = branch_worktree("feat");
        assert_eq!(
            classify_worktree(&wt, "main", false, true),
            Some(DoneReason::Gone)
        );
    }

    #[test]
    fn classify_skips_when_neither() {
        let wt = branch_worktree("feat");
        assert_eq!(classify_worktree(&wt, "main", false, false), None);
    }
}
