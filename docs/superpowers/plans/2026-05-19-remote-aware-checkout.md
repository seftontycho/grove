# Remote-Aware Worktree Checkout Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix `grove open`'s branch picker so it auto-fetches, shows local and remote branches together, sets upstream tracking when checking out a remote branch, and stops resetting branches that already exist. Make the positional form (`grove open <repo> <branch>`) resolve sensibly whether the user typed a local name, a remote ref, or a brand-new name.

**Architecture:** A new `WorktreeSource` enum in `src/git.rs` makes `worktree_add`'s intent explicit, replacing the always-`-B` reset with three precise git invocations. A new `BranchSource` enum in `src/cmd/open.rs` carries the picker's decision through to the dispatch. Pure picker-construction logic is extracted into `build_picker_entries` so it can be unit-tested without dialoguer.

**Tech Stack:** Rust 2024, clap, dialoguer, anyhow, the `git` CLI (shelled out), `tempfile` (existing dev-dependency).

**Spec:** `docs/superpowers/specs/2026-05-19-remote-aware-checkout-design.md`

---

## File Structure

**Modified:**
- `src/git.rs` — add `WorktreeSource` enum; refactor `worktree_add`; add `fetch_all`, `list_local_branches`, `list_remotes`; filter symbolic refs from `list_remote_branches`; add tests for each.
- `src/cmd/open.rs` — add `BranchSource` enum and `build_picker_entries`; replace `select_or_create_branch` with `select_branch_source`; add `resolve_branch_arg`; rewire `run` to dispatch via `BranchSource`.

**Untouched:** `Cargo.toml`, `src/cli.rs`, `src/main.rs`, `src/db/*`, `src/multiplexer.rs`, `src/zellij.rs`, `src/tmux.rs`, `src/cmd/repo/*`, `src/cmd/{clone,tree,session,config,init,completions}.rs`, README.

---

## Task 1: Add `WorktreeSource` enum and refactor `worktree_add`

**Files:**
- Modify: `src/git.rs`
- Modify: `src/cmd/open.rs`

`worktree_add` currently runs `git worktree add -B <branch> <path>` for every caller. `-B` resets the branch if it exists, which is the bug we're fixing. Splitting the call by `WorktreeSource` makes the intent explicit at every call site.

- [ ] **Step 1: Write the failing test for `ExistingLocal` preserving the branch**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/git.rs`, after `worktree_add_places_worktree_at_container_root`:

```rust
    #[test]
    fn worktree_add_existing_local_does_not_reset() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();
        let bare = container.join(".bare");

        // Create a local branch `feat` advanced one commit past master.
        // Use plumbing so we don't need a working tree.
        let empty_tree =
            run_git_capture(&["hash-object", "-w", "-t", "tree", "--stdin"], &bare).unwrap();
        let master_commit =
            run_git_capture(&["rev-parse", "refs/heads/master"], &bare).unwrap();
        let feat_commit = run_git_capture(
            &[
                "-c",
                "user.name=grove",
                "-c",
                "user.email=grove@localhost",
                "commit-tree",
                empty_tree.as_str(),
                "-p",
                master_commit.as_str(),
                "-m",
                "feat work",
            ],
            &bare,
        )
        .unwrap();
        run_git(
            &["update-ref", "refs/heads/feat", feat_commit.as_str()],
            &bare,
        )
        .unwrap();

        // Open the existing local branch as a worktree.
        let wt = worktree_add(&container, "feat", WorktreeSource::ExistingLocal).unwrap();

        // The worktree's HEAD must point at the existing feat commit,
        // not at master.
        let head = run_git_capture(&["rev-parse", "HEAD"], &wt).unwrap();
        assert_eq!(head, feat_commit, "existing local branch must not be reset");
    }
```

- [ ] **Step 2: Run the test to confirm it fails to compile**

Run: `cargo test --lib worktree_add_existing_local_does_not_reset`
Expected: compilation error — `WorktreeSource` not defined and `worktree_add` has wrong arity.

- [ ] **Step 3: Add `WorktreeSource` enum and refactor `worktree_add`**

In `src/git.rs`, find the existing `worktree_add` (around line 477-494) and replace it with:

```rust
/// How to materialize a branch when adding a worktree.
#[derive(Debug, Clone)]
pub enum WorktreeSource {
    /// The branch already exists locally; just check it out into the worktree.
    ExistingLocal,
    /// Create the branch from a remote-tracking ref, with upstream set.
    TrackingRemote { upstream: String },
    /// Create a brand-new branch from HEAD.
    NewFromHead,
}

/// Create a new worktree for `branch`. Returns the path to the created worktree.
pub fn worktree_add(repo_path: &Path, branch: &str, source: WorktreeSource) -> Result<PathBuf> {
    let layout = RepoLayout::detect(repo_path)?;
    let worktree_dir = layout.worktree_path(branch);

    let mut cmd = Command::new("git");
    cmd.arg("worktree").arg("add");
    match &source {
        WorktreeSource::ExistingLocal => {
            cmd.arg(&worktree_dir).arg(branch);
        }
        WorktreeSource::TrackingRemote { upstream } => {
            cmd.arg("-b").arg(branch).arg(&worktree_dir).arg(upstream);
        }
        WorktreeSource::NewFromHead => {
            cmd.arg("-b").arg(branch).arg(&worktree_dir);
        }
    }
    let status = cmd
        .current_dir(layout.git_dir())
        .status()
        .context("Failed to run git worktree add")?;

    if !status.success() {
        bail!("git worktree add failed for branch '{branch}'");
    }

    Ok(worktree_dir)
}
```

- [ ] **Step 4: Update existing tests in `src/git.rs` to use the new signature**

Locate and update three existing tests:

In `worktree_list_works_for_container_layout` (around line 715), change:

```rust
        let wt = worktree_add(&container, "master").unwrap();
```

to:

```rust
        let wt = worktree_add(&container, "master", WorktreeSource::ExistingLocal).unwrap();
```

In `worktree_add_places_worktree_at_container_root` (around line 726), change the same line in the same way, and rename the test to reflect what it covers:

```rust
    #[test]
    fn worktree_add_existing_local_places_worktree_at_container_root() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();

        let wt = worktree_add(&container, "master", WorktreeSource::ExistingLocal).unwrap();

        // Worktree is a direct child of the container, NOT under worktrees/.
        assert_eq!(wt, container.join("master"));
        assert!(wt.join(".git").is_file());
        assert!(!container.join("worktrees").exists());
    }
```

In `migrate_converts_legacy_bare_to_container` the test uses `git worktree add -B feat ...` directly via `run_git`, not via `worktree_add` — leave it alone.

- [ ] **Step 5: Update the caller in `src/cmd/open.rs`**

No import changes (the existing `use crate::git;` already gives access to `git::WorktreeSource`).

In the `run` function, replace the existing `git::worktree_add` call (around line 58):

```rust
            let path = git::worktree_add(&repo.path, &branch_name)?;
```

with:

```rust
            let path = git::worktree_add(&repo.path, &branch_name, git::WorktreeSource::NewFromHead)?;
```

This preserves today's "create a new branch from HEAD" semantics for the picker's `[create new branch]` path. The other picker outcomes (local / tracking remote) are wired up in Task 7; until then this single call site stays as-is.

- [ ] **Step 6: Run all tests to confirm green**

Run: `cargo test --lib`
Expected: all tests pass, including the new `worktree_add_existing_local_does_not_reset`.

- [ ] **Step 7: Build the binary to confirm callers compile**

Run: `cargo build`
Expected: clean build, no warnings about unused imports.

- [ ] **Step 8: Add the `TrackingRemote` test**

Add to the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn worktree_add_tracking_remote_sets_upstream() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a bare "remote" with a `feat` branch.
        let source = tmp.path().join("source");
        init_bare_at(&source, "master").unwrap();
        let empty_tree =
            run_git_capture(&["hash-object", "-w", "-t", "tree", "--stdin"], &source).unwrap();
        let master_commit =
            run_git_capture(&["rev-parse", "refs/heads/master"], &source).unwrap();
        let feat_commit = run_git_capture(
            &[
                "-c",
                "user.name=grove",
                "-c",
                "user.email=grove@localhost",
                "commit-tree",
                empty_tree.as_str(),
                "-p",
                master_commit.as_str(),
                "-m",
                "feat on remote",
            ],
            &source,
        )
        .unwrap();
        run_git(
            &["update-ref", "refs/heads/feat", feat_commit.as_str()],
            &source,
        )
        .unwrap();

        // Clone it into a container layout, which sets up origin/* tracking refs.
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let cloned = clone_bare(source.to_str().unwrap(), &dest).unwrap();

        // Add a worktree that tracks origin/feat.
        let wt = worktree_add(
            &cloned.path,
            "feat",
            WorktreeSource::TrackingRemote {
                upstream: "origin/feat".to_string(),
            },
        )
        .unwrap();

        // The local `feat` branch must be configured to track origin/feat.
        let bare = cloned.path.join(".bare");
        let remote_cfg = run_git_capture(&["config", "branch.feat.remote"], &bare).unwrap();
        let merge_cfg = run_git_capture(&["config", "branch.feat.merge"], &bare).unwrap();
        assert_eq!(remote_cfg, "origin");
        assert_eq!(merge_cfg, "refs/heads/feat");

        // And the worktree's HEAD must be at the remote's feat commit.
        let head = run_git_capture(&["rev-parse", "HEAD"], &wt).unwrap();
        assert_eq!(head, feat_commit);
    }
```

- [ ] **Step 9: Run the new test**

Run: `cargo test --lib worktree_add_tracking_remote_sets_upstream`
Expected: PASS. The implementation already covers this case from Step 3.

- [ ] **Step 10: Add the `NewFromHead` test**

Add to the test module:

```rust
    #[test]
    fn worktree_add_new_from_head_creates_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();

        // master exists from init_repo; create a brand-new branch off HEAD.
        let wt = worktree_add(&container, "shiny", WorktreeSource::NewFromHead).unwrap();

        assert_eq!(wt, container.join("shiny"));

        let bare = container.join(".bare");
        let branches =
            run_git_capture(&["branch", "--format=%(refname:short)"], &bare).unwrap();
        assert!(branches.lines().any(|b| b == "shiny"));

        // No upstream config: brand-new branch should not track anything.
        let remote_cfg = Command::new("git")
            .args(["config", "branch.shiny.remote"])
            .current_dir(&bare)
            .output()
            .unwrap();
        assert!(
            !remote_cfg.status.success(),
            "new branch must not have an upstream"
        );
    }
```

- [ ] **Step 11: Run the new test**

Run: `cargo test --lib worktree_add_new_from_head_creates_branch`
Expected: PASS.

- [ ] **Step 12: Run the full test suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 13: Commit**

```bash
git add src/git.rs src/cmd/open.rs
git commit -m "refactor: introduce WorktreeSource and split worktree_add by intent"
```

---

## Task 2: Add `git::fetch_all`

**Files:**
- Modify: `src/git.rs`

The picker needs fresh remote state. `fetch_all` runs `git fetch --all --prune` against the repo's git directory.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn fetch_all_picks_up_new_remote_branches() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a bare "remote".
        let source = tmp.path().join("source");
        init_bare_at(&source, "master").unwrap();

        // Clone it into a container.
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let cloned = clone_bare(source.to_str().unwrap(), &dest).unwrap();
        let bare = cloned.path.join(".bare");

        // After the clone, origin/feat should NOT be present.
        let before =
            run_git_capture(&["branch", "-r", "--format=%(refname:short)"], &bare).unwrap();
        assert!(!before.lines().any(|b| b == "origin/feat"));

        // Push a new branch on the source.
        let empty_tree =
            run_git_capture(&["hash-object", "-w", "-t", "tree", "--stdin"], &source).unwrap();
        let master_commit =
            run_git_capture(&["rev-parse", "refs/heads/master"], &source).unwrap();
        let feat_commit = run_git_capture(
            &[
                "-c",
                "user.name=grove",
                "-c",
                "user.email=grove@localhost",
                "commit-tree",
                empty_tree.as_str(),
                "-p",
                master_commit.as_str(),
                "-m",
                "feat on remote",
            ],
            &source,
        )
        .unwrap();
        run_git(
            &["update-ref", "refs/heads/feat", feat_commit.as_str()],
            &source,
        )
        .unwrap();

        // fetch_all should bring the new branch into the clone.
        fetch_all(&cloned.path).unwrap();

        let after =
            run_git_capture(&["branch", "-r", "--format=%(refname:short)"], &bare).unwrap();
        assert!(
            after.lines().any(|b| b == "origin/feat"),
            "fetch_all should pull origin/feat\nactual: {after}"
        );
    }
```

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test --lib fetch_all_picks_up_new_remote_branches`
Expected: compilation error — `fetch_all` is undefined.

- [ ] **Step 3: Implement `fetch_all`**

Add to `src/git.rs`, after `worktree_add` (around line 510):

```rust
/// Fetch all remotes (with prune). Used to refresh remote-tracking refs
/// before the branch picker.
pub fn fetch_all(repo_path: &Path) -> Result<()> {
    let layout = RepoLayout::detect(repo_path)?;
    let status = Command::new("git")
        .args(["fetch", "--all", "--prune"])
        .current_dir(layout.git_dir())
        .status()
        .context("Failed to run git fetch --all --prune")?;

    if !status.success() {
        bail!("git fetch --all --prune failed");
    }
    Ok(())
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cargo test --lib fetch_all_picks_up_new_remote_branches`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/git.rs
git commit -m "feat: add git::fetch_all for picker refresh"
```

---

## Task 3: Add `git::list_local_branches`

**Files:**
- Modify: `src/git.rs`

The unified picker needs the list of local branches.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn list_local_branches_returns_seeded_branches() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();

        // Add two more local branches via plumbing.
        let bare = container.join(".bare");
        let master_commit =
            run_git_capture(&["rev-parse", "refs/heads/master"], &bare).unwrap();
        for name in ["feat", "fix/typo"] {
            let refname = format!("refs/heads/{name}");
            run_git(&["update-ref", &refname, master_commit.as_str()], &bare).unwrap();
        }

        let mut branches = list_local_branches(&container).unwrap();
        branches.sort();
        assert_eq!(branches, vec!["feat", "fix/typo", "master"]);
    }
```

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test --lib list_local_branches_returns_seeded_branches`
Expected: compilation error — `list_local_branches` is undefined.

- [ ] **Step 3: Implement `list_local_branches`**

Add to `src/git.rs`, alongside `list_remote_branches` (around line 550):

```rust
/// List local branches for a repository.
pub fn list_local_branches(repo_path: &Path) -> Result<Vec<String>> {
    let layout = RepoLayout::detect(repo_path)?;
    let output = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(layout.git_dir())
        .output()
        .context("Failed to run git branch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git branch failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let branches = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(branches)
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cargo test --lib list_local_branches_returns_seeded_branches`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/git.rs
git commit -m "feat: add git::list_local_branches"
```

---

## Task 4: Add `git::list_remotes`

**Files:**
- Modify: `src/git.rs`

The positional resolver uses the set of configured remotes to detect typos like `origin/fet`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn list_remotes_returns_configured_remotes() {
        let tmp = tempfile::tempdir().unwrap();

        let source = tmp.path().join("source");
        init_bare_at(&source, "master").unwrap();

        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let cloned = clone_bare(source.to_str().unwrap(), &dest).unwrap();
        let bare = cloned.path.join(".bare");

        // Add a second remote.
        run_git(
            &[
                "remote",
                "add",
                "upstream",
                source.to_str().unwrap(),
            ],
            &bare,
        )
        .unwrap();

        let mut remotes = list_remotes(&cloned.path).unwrap();
        remotes.sort();
        assert_eq!(remotes, vec!["origin", "upstream"]);
    }
```

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test --lib list_remotes_returns_configured_remotes`
Expected: compilation error — `list_remotes` is undefined.

- [ ] **Step 3: Implement `list_remotes`**

Add to `src/git.rs`, near `list_local_branches` / `list_remote_branches`:

```rust
/// List configured remote names for a repository.
pub fn list_remotes(repo_path: &Path) -> Result<Vec<String>> {
    let layout = RepoLayout::detect(repo_path)?;
    let output = Command::new("git")
        .arg("remote")
        .current_dir(layout.git_dir())
        .output()
        .context("Failed to run git remote")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git remote failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let remotes = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    Ok(remotes)
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cargo test --lib list_remotes_returns_configured_remotes`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/git.rs
git commit -m "feat: add git::list_remotes"
```

---

## Task 5: Filter symbolic refs from `list_remote_branches`

**Files:**
- Modify: `src/git.rs`

`git branch -r --format=%(refname:short)` outputs `refs/remotes/origin/HEAD` as the bare short name `origin` (because git strips the `/HEAD` tail). That entry has no `/` and would appear in the picker as a non-branch. Filter it.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn list_remote_branches_filters_symbolic_refs() {
        let tmp = tempfile::tempdir().unwrap();

        let source = tmp.path().join("source");
        init_bare_at(&source, "master").unwrap();

        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let cloned = clone_bare(source.to_str().unwrap(), &dest).unwrap();
        let bare = cloned.path.join(".bare");

        // Manually create a symbolic ref refs/remotes/origin/HEAD -> refs/remotes/origin/master,
        // which `git branch -r --format=%(refname:short)` shortens to the bare name "origin".
        run_git(
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/master",
            ],
            &bare,
        )
        .unwrap();

        let branches = list_remote_branches(&cloned.path).unwrap();

        // The bare "origin" entry (from the symbolic ref) must not appear.
        assert!(
            !branches.iter().any(|b| b == "origin"),
            "bare remote-name short form must be filtered\nactual: {branches:?}"
        );
        // Defensive: any */HEAD entry must also be filtered.
        assert!(
            !branches.iter().any(|b| b.ends_with("/HEAD")),
            "*/HEAD entries must be filtered\nactual: {branches:?}"
        );
        // Real branches still come through.
        assert!(branches.iter().any(|b| b == "origin/master"));
    }
```

- [ ] **Step 2: Run the test to confirm it fails**

Run: `cargo test --lib list_remote_branches_filters_symbolic_refs`
Expected: FAIL on the first assertion — current `list_remote_branches` returns `origin` in the list.

- [ ] **Step 3: Add the filter**

In `src/git.rs`, locate `list_remote_branches` (around line 530). Replace the existing function body's branch-collection step:

```rust
    let branches = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
```

with:

```rust
    let branches = stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        // Drop symbolic refs: `git branch -r --format=%(refname:short)` shortens
        // `refs/remotes/<remote>/HEAD` to the bare `<remote>` (no slash). Filter
        // that out, and defensively any `*/HEAD` entries from older git versions.
        .filter(|l| l.contains('/') && !l.ends_with("/HEAD"))
        .collect();
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cargo test --lib list_remote_branches_filters_symbolic_refs`
Expected: PASS.

- [ ] **Step 5: Run the full suite**

Run: `cargo test`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/git.rs
git commit -m "fix: filter symbolic refs from list_remote_branches"
```

---

## Task 6: Add `BranchSource` enum and `build_picker_entries`

**Files:**
- Modify: `src/cmd/open.rs`

`build_picker_entries` is the pure-logic core of the picker: it takes `(locals, remote_refs)` and returns the labelled, ordered, deduplicated list the user sees, along with the matching `BranchSource` per entry. Pulling it out keeps the picker unit-testable without dialoguer.

- [ ] **Step 1: Write the failing tests**

Add to the bottom of `src/cmd/open.rs` (creating the test module if it doesn't exist):

```rust
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
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test --lib --package grove tests::build_picker_entries`
Expected: compilation errors — `BranchSource` and `build_picker_entries` are undefined.

- [ ] **Step 3: Add the `BranchSource` enum**

At the top of `src/cmd/open.rs`, after the existing `use` statements, add:

```rust
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
```

- [ ] **Step 4: Add `build_picker_entries`**

Add to `src/cmd/open.rs`, somewhere below the `BranchSource` impl:

```rust
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
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --lib tests::build_picker_entries`
Expected: all five build_picker_entries tests PASS.

- [ ] **Step 6: Run the full suite**

Run: `cargo test`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add src/cmd/open.rs
git commit -m "feat: add BranchSource and build_picker_entries"
```

---

## Task 7: Replace `select_or_create_branch` and rewire `run` to dispatch via `BranchSource`

**Files:**
- Modify: `src/cmd/open.rs`

This is the wiring task. `select_or_create_branch` is replaced with `select_branch_source` (which fetches first, then uses `build_picker_entries`). A new `resolve_branch_arg` handles the positional path. `run` dispatches the resulting `BranchSource` to the correct `WorktreeSource` for `git::worktree_add`.

- [ ] **Step 1: Write the failing tests for `resolve_branch_arg`**

Add to the `tests` module in `src/cmd/open.rs`:

```rust
    use crate::git;

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

        // Get master's commit to point new refs at.
        let master_commit = std::process::Command::new("git")
            .args(["rev-parse", "refs/heads/master"])
            .current_dir(&bare)
            .output()
            .unwrap();
        let master_commit = String::from_utf8_lossy(&master_commit.stdout)
            .trim()
            .to_string();

        // Seed additional local branches.
        for b in local_branches {
            std::process::Command::new("git")
                .args(["update-ref", &format!("refs/heads/{b}"), &master_commit])
                .current_dir(&bare)
                .status()
                .unwrap();
        }

        // Configure a stub `origin` remote (URL is never fetched).
        std::process::Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://example.invalid/source.git",
            ])
            .current_dir(&bare)
            .status()
            .unwrap();

        // Seed remote-tracking refs directly. `list_remote_branches` reads
        // these as `origin/<branch>`.
        for b in remote_branches {
            std::process::Command::new("git")
                .args([
                    "update-ref",
                    &format!("refs/remotes/origin/{b}"),
                    &master_commit,
                ])
                .current_dir(&bare)
                .status()
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
```

- [ ] **Step 2: Run the tests to confirm they fail**

Run: `cargo test --lib tests::resolve_branch_arg`
Expected: compilation errors — `resolve_branch_arg` is undefined.

- [ ] **Step 3: Add `resolve_branch_arg`**

Add to `src/cmd/open.rs`, alongside `build_picker_entries`:

```rust
/// Resolve a positional `<branch>` argument to a `BranchSource`.
///
/// Resolution order:
/// 1. exact local match → `Local`
/// 2. exact remote-ref match → `Remote { local, upstream }`
/// 3. `<known-remote>/<rest>` with no matching ref → reject (typo guard)
/// 4. otherwise → `New`
fn resolve_branch_arg(repo: &Repo, arg: &str) -> Result<BranchSource> {
    let locals = git::list_local_branches(&repo.path).unwrap_or_default();
    if locals.iter().any(|b| b == arg) {
        return Ok(BranchSource::Local(arg.to_string()));
    }

    let remote_refs = git::list_remote_branches(&repo.path).unwrap_or_default();
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
        let remotes = git::list_remotes(&repo.path).unwrap_or_default();
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
```

- [ ] **Step 4: Replace `select_or_create_branch` with `select_branch_source`**

In `src/cmd/open.rs`, locate the existing `select_or_create_branch` (around lines 102-131) and replace it with:

```rust
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
```

- [ ] **Step 5: Rewire `run` to dispatch via `BranchSource`**

Replace the body of `run` in `src/cmd/open.rs`. The current body (lines 9-68) becomes:

```rust
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
```

- [ ] **Step 6: Run the resolver tests**

Run: `cargo test --lib tests::resolve_branch_arg`
Expected: all four resolve_branch_arg tests PASS.

- [ ] **Step 7: Run the full suite**

Run: `cargo test`
Expected: all green.

- [ ] **Step 8: Build the binary**

Run: `cargo build`
Expected: clean build.

- [ ] **Step 9: Commit**

```bash
git add src/cmd/open.rs
git commit -m "feat: unified picker with fetch + proper upstream tracking"
```

---

## Task 8: Smoke test the binary end-to-end

**Files:** none modified (manual verification only)

Verify the user-visible behaviour against the spec's goals.

- [ ] **Step 1: Install the dev build into PATH for `gv`**

Run: `cargo install --path . --force`
Expected: build succeeds, installs to `~/.cargo/bin/grove`.

- [ ] **Step 2: Test auto-fetch + unified picker**

In a repo that has been cloned via grove (so it has a `.bare` container), have a colleague push a new branch (or push one yourself from another clone). Then run:

```sh
gv <repo>
```

Expected:
- Brief delay while fetch runs.
- Picker shows `[new]    + create new branch` first.
- Then `[local]` entries for local branches (alphabetical).
- Then `[remote]` entries for remote branches not shadowed by locals.
- The newly-pushed branch appears in `[remote]`.

- [ ] **Step 3: Test that picking a remote branch sets upstream**

Pick a `[remote] origin/<branch>` entry. After the session opens, in the worktree's shell:

```sh
git branch -vv
```

Expected: the current branch shows `[origin/<branch>]` upstream marker. `git pull` and `git push` work without explicit remote/branch arguments.

- [ ] **Step 4: Test that picking a local branch doesn't reset**

In an existing worktree's shell, make an uncommitted change. Then close the session (`gv -c` if you have the alias, or `grove tree close`). Re-open the same repo+branch:

```sh
gv <repo> <branch>
```

Expected: the worktree is reused (reuse message printed); the uncommitted change is still there.

Now delete the worktree (`grove tree close` and confirm) and re-open:

```sh
gv <repo> <branch>
```

Expected: a fresh worktree is created at the branch's HEAD; no error about "branch already exists".

- [ ] **Step 5: Test the positional typo guard**

```sh
gv <repo> origin/this-branch-does-not-exist
```

Expected: error message containing "unknown branch" and an explanation that `origin` is a remote.

- [ ] **Step 6: Test fetch-failure fallback**

Temporarily point the repo's origin at an unreachable URL:

```sh
git -C <repo>/.bare remote set-url origin https://invalid.invalid/whatever.git
```

Then run the picker:

```sh
gv <repo>
```

Expected: a `Warning: fetch failed: ...` line on stderr, followed by the picker showing locals + cached remotes. The command does NOT hang (allow a few seconds for DNS/connect timeout) or bail.

Restore the URL when done:

```sh
git -C <repo>/.bare remote set-url origin <original-url>
```

- [ ] **Step 7: Commit (no code changes, but record successful manual verification)**

If anything in steps 2-6 didn't match expectations, file a follow-up and fix before declaring done.

No commit needed for this task; it's verification only. Move to the final summary.

---

## Done

All seven implementation tasks committed. The full feature:

1. Auto-fetches before the interactive picker (non-fatal on failure).
2. Shows local + remote branches in a single unified picker.
3. Hides remote entries shadowed by a same-named local.
4. Sets proper upstream tracking when checking out a remote branch.
5. Stops resetting existing local branches.
6. Resolves positional `<branch>` arguments sensibly (local → remote → typo-guard → new).

Per the spec's migration section: no data migration, no DB schema change. The only user-visible behaviour change for existing users is that the silent `-B` reset is gone — they'll see an explicit error if they try to overwrite a local branch with the same name, which is the desired outcome.
