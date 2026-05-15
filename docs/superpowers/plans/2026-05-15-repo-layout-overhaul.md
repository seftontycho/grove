# Repo Layout Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move grove from bare repos to a `.bare` container layout (fixing the worktree/admin-dir collision), add `grove repo new` and `grove repo migrate`, and remove the broken `zj-session-bar` zellij plugin.

**Architecture:** A new internal `RepoLayout` type in `src/git.rs` detects, per repo, whether it is the new container layout (`<repo>/.bare`) or a legacy bare repo, and resolves the git directory and worktree base for each. The `git.rs` worktree functions detect the layout internally, so their public signatures and callers are unchanged. New git-layer functions (`init_repo`, reworked `clone_bare`, `migrate_to_container`) do the heavy lifting; thin `cmd/repo/*` orchestration wires them to the CLI. The zellij plugin is removed entirely; running grove inside a zellij session now bails with a "detach first" message.

**Tech Stack:** Rust 2024, clap, rusqlite, anyhow, the `git` CLI (shelled out), `tempfile` (new dev-dependency for tests).

**Spec:** `docs/superpowers/specs/2026-05-15-repo-layout-overhaul-design.md`

---

## File Structure

**Created:**
- `src/cmd/repo/mod.rs` — module root for repo subcommands; re-exports.
- `src/cmd/repo/add.rs` — `add` (moved from `cmd/repo.rs`).
- `src/cmd/repo/rm.rs` — `rm` (moved from `cmd/repo.rs`).
- `src/cmd/repo/list.rs` — `list` + `RepoRow` table type (moved from `cmd/repo.rs`).
- `src/cmd/repo/new.rs` — `new` subcommand.
- `src/cmd/repo/migrate.rs` — `migrate` subcommand.

**Deleted:**
- `src/cmd/repo.rs` — replaced by the `src/cmd/repo/` module.

**Modified:**
- `Cargo.toml` — add `[dev-dependencies] tempfile`.
- `src/git.rs` — `RepoLayout`; git helpers; `init_repo`; reworked `clone_bare` and worktree functions; `migrate_to_container`; `has_uncommitted_tracked_changes`.
- `src/cli.rs` — add `RepoCmd::New` and `RepoCmd::Migrate`.
- `src/main.rs` — wire the two new subcommands.
- `src/cmd/clone.rs` — make `select_directory` `pub(crate)` for reuse.
- `src/zellij.rs` — remove the plugin code; add the `$ZELLIJ` guard.
- `templates/zellij.kdl` — remove the `zj-session-bar` plugin pane.
- `README.md`, `docs/src/**` — documentation updates.

---

## Task 1: Add `tempfile` dev-dependency

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the dev-dependencies section**

Append to the end of `Cargo.toml`:

```toml

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 2: Verify it resolves**

Run: `cargo build --tests`
Expected: compiles successfully (tempfile downloaded; no test uses it yet).

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "build: add tempfile dev-dependency for layout tests"
```

---

## Task 2: `RepoLayout` type and detection

**Files:**
- Modify: `src/git.rs`

`RepoLayout` describes how a tracked repo is laid out on disk and resolves the
git directory and worktree base. Detection is filesystem-only (no `git`
subprocess) so it is fast and unit-testable.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `src/git.rs`:

```rust
    #[test]
    fn detect_container_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let bare = tmp.path().join(".bare");
        std::fs::create_dir_all(bare.join("objects")).unwrap();
        std::fs::write(bare.join("HEAD"), "ref: refs/heads/master\n").unwrap();

        let layout = RepoLayout::detect(tmp.path()).unwrap();
        assert!(matches!(layout, RepoLayout::Container { .. }));
        assert_eq!(layout.git_dir(), bare);
        assert_eq!(layout.worktree_path("feat"), tmp.path().join("feat"));
    }

    #[test]
    fn detect_legacy_bare_layout() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("objects")).unwrap();
        std::fs::write(tmp.path().join("HEAD"), "ref: refs/heads/master\n").unwrap();

        let layout = RepoLayout::detect(tmp.path()).unwrap();
        assert!(matches!(layout, RepoLayout::LegacyBare { .. }));
        assert_eq!(layout.git_dir(), tmp.path());
        assert_eq!(
            layout.worktree_path("feat"),
            tmp.path().join("worktrees").join("feat")
        );
    }

    #[test]
    fn detect_rejects_non_repo() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(RepoLayout::detect(tmp.path()).is_err());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib git::tests::detect`
Expected: FAIL — `cannot find type RepoLayout in this scope`.

- [ ] **Step 3: Implement `RepoLayout`**

Add to `src/git.rs`, immediately after the `use` statements at the top:

```rust
/// On-disk layout of a grove-managed repository.
#[derive(Debug, Clone)]
pub enum RepoLayout {
    /// New container layout: `<container>/.bare` holds the git data and each
    /// worktree is a direct child of `<container>`.
    Container { container: PathBuf },
    /// Legacy layout: `<bare>` is itself a bare repository and worktrees live
    /// under `<bare>/worktrees`.
    LegacyBare { bare: PathBuf },
}

impl RepoLayout {
    /// Detect the layout of a tracked repository from its recorded path.
    pub fn detect(repo_path: &Path) -> Result<RepoLayout> {
        if repo_path.join(".bare").join("HEAD").is_file() {
            return Ok(RepoLayout::Container {
                container: repo_path.to_path_buf(),
            });
        }
        if repo_path.join("HEAD").is_file() && repo_path.join("objects").is_dir() {
            return Ok(RepoLayout::LegacyBare {
                bare: repo_path.to_path_buf(),
            });
        }
        bail!(
            "{} is not a grove-managed repository \
             (no .bare container or bare repo found)",
            repo_path.display()
        )
    }

    /// The directory to run git commands against.
    pub fn git_dir(&self) -> PathBuf {
        match self {
            RepoLayout::Container { container } => container.join(".bare"),
            RepoLayout::LegacyBare { bare } => bare.clone(),
        }
    }

    /// The directory under which worktree checkouts are placed.
    pub fn worktree_base(&self) -> PathBuf {
        match self {
            RepoLayout::Container { container } => container.clone(),
            RepoLayout::LegacyBare { bare } => bare.join("worktrees"),
        }
    }

    /// The checkout path for a worktree of the given branch.
    pub fn worktree_path(&self, branch: &str) -> PathBuf {
        self.worktree_base().join(branch)
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib git::tests::detect`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat: add RepoLayout for container/legacy repo detection"
```

---

## Task 3: Git helpers and `init_repo`

**Files:**
- Modify: `src/git.rs`

Adds private git-invocation helpers, a private `init_bare_at` that initializes
a bare repo with one empty seed commit, and the public `init_repo` that builds
a full `.bare` container.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn init_repo_creates_container_with_initial_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");

        init_repo(&container, "master").unwrap();

        // Container layout on disk.
        assert!(container.join(".bare").join("HEAD").is_file());
        assert_eq!(
            std::fs::read_to_string(container.join(".git")).unwrap(),
            "gitdir: ./.bare\n"
        );

        // Exactly one commit on master.
        let bare = container.join(".bare");
        let log = run_git_capture(&["log", "--oneline", "master"], &bare).unwrap();
        assert_eq!(log.lines().count(), 1);

        // Detected as a container.
        assert!(matches!(
            RepoLayout::detect(&container).unwrap(),
            RepoLayout::Container { .. }
        ));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib git::tests::init_repo`
Expected: FAIL — `cannot find function init_repo` / `run_git_capture`.

- [ ] **Step 3: Implement the helpers and `init_repo`**

Add `Stdio` to the imports at the top of `src/git.rs`:

```rust
use std::process::{Command, Stdio};
```

(Replace the existing `use std::process::Command;` line.)

Add the following functions to `src/git.rs` (place them after the `RepoLayout`
`impl` block):

```rust
/// Run `git <args>` with the working directory set to `git_dir`, expecting
/// success.
fn run_git(args: &[&str], git_dir: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(args)
        .current_dir(git_dir)
        .status()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    if !status.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

/// Run `git <args>` with the working directory set to `git_dir` and return
/// trimmed stdout. Stdin is empty, which `git hash-object --stdin` reads as
/// empty content.
fn run_git_capture(args: &[&str], git_dir: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(git_dir)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {stderr}", args.join(" "));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Write the `.git` pointer file that makes `<container>` resolve to `.bare`.
fn write_gitdir_file(container: &Path) -> Result<()> {
    let git_file = container.join(".git");
    std::fs::write(&git_file, "gitdir: ./.bare\n")
        .with_context(|| format!("Failed to write {}", git_file.display()))?;
    Ok(())
}

/// Initialize a bare repository at `bare` with a single empty commit on
/// `default_branch`, and point `HEAD` at that branch.
fn init_bare_at(bare: &Path, default_branch: &str) -> Result<()> {
    std::fs::create_dir_all(bare)
        .with_context(|| format!("Failed to create {}", bare.display()))?;
    run_git(&["init", "--bare"], bare)?;

    // Seed an empty commit via plumbing so the branch exists without needing
    // a working tree. `commit-tree` is given an explicit identity so it does
    // not depend on the caller's git config.
    let empty_tree = run_git_capture(&["hash-object", "-w", "-t", "tree", "--stdin"], bare)?;
    let commit = run_git_capture(
        &[
            "-c",
            "user.name=grove",
            "-c",
            "user.email=grove@localhost",
            "commit-tree",
            empty_tree.as_str(),
            "-m",
            "Initial commit",
        ],
        bare,
    )?;

    let branch_ref = format!("refs/heads/{default_branch}");
    run_git(&["update-ref", branch_ref.as_str(), commit.as_str()], bare)?;
    run_git(&["symbolic-ref", "HEAD", branch_ref.as_str()], bare)?;
    Ok(())
}

/// Create a new repository in the `.bare` container layout at `container`,
/// with a single empty commit on `default_branch`.
pub fn init_repo(container: &Path, default_branch: &str) -> Result<()> {
    if container.exists() {
        bail!("Destination already exists: {}", container.display());
    }
    std::fs::create_dir_all(container)
        .with_context(|| format!("Failed to create {}", container.display()))?;
    init_bare_at(&container.join(".bare"), default_branch)?;
    write_gitdir_file(container)?;
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib git::tests::init_repo`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat: add init_repo to create .bare container repos"
```

---

## Task 4: `worktree_add` uses `RepoLayout`

**Files:**
- Modify: `src/git.rs`

Place worktrees at the layout-resolved path (`<container>/<branch>` for
container repos) and run `git` against the resolved git directory.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn worktree_add_places_worktree_at_container_root() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();

        let wt = worktree_add(&container, "master").unwrap();

        // Worktree is a direct child of the container, NOT under worktrees/.
        assert_eq!(wt, container.join("master"));
        assert!(wt.join(".git").is_file());
        assert!(!container.join("worktrees").exists());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib git::tests::worktree_add`
Expected: FAIL — assertion fails: worktree is created at `myrepo/worktrees/master` by the current implementation.

- [ ] **Step 3: Rework `worktree_add`**

Replace the existing `worktree_add` function in `src/git.rs` with:

```rust
/// Create a new worktree. Returns the path to the created worktree.
pub fn worktree_add(repo_path: &Path, branch: &str) -> Result<PathBuf> {
    let layout = RepoLayout::detect(repo_path)?;
    let worktree_dir = layout.worktree_path(branch);

    let status = Command::new("git")
        .args(["worktree", "add", "-B", branch])
        .arg(&worktree_dir)
        .current_dir(layout.git_dir())
        .status()
        .context("Failed to run git worktree add")?;

    if !status.success() {
        bail!("git worktree add failed for branch '{branch}'");
    }

    Ok(worktree_dir)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib git::tests::worktree_add`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat: place worktrees via RepoLayout in worktree_add"
```

---

## Task 5: Layout-aware `worktree_list`, `worktree_remove`, `worktree_prune`, `list_remote_branches`

**Files:**
- Modify: `src/git.rs`

These functions currently run `git` with the working directory set to
`repo_path`. For a container repo `repo_path` is the container, not a git
directory, so they must run against `RepoLayout::git_dir()`.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn worktree_list_works_for_container_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();
        let wt = worktree_add(&container, "master").unwrap();

        let trees = worktree_list(&container).unwrap();
        assert!(trees.iter().any(|t| t.path == wt && !t.is_bare));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib git::tests::worktree_list_works_for_container`
Expected: FAIL — `git worktree list` run from the container directory does not resolve the bare repo.

- [ ] **Step 3: Rework the four functions**

In `src/git.rs`, change `worktree_list` so it resolves the layout. Replace the
body up to the `Command` invocation:

```rust
/// List worktrees for a repository.
pub fn worktree_list(repo_path: &Path) -> Result<Vec<Worktree>> {
    let layout = RepoLayout::detect(repo_path)?;
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(layout.git_dir())
        .output()
        .context("Failed to run git worktree list")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git worktree list failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_worktree_list(&stdout)
}
```

Replace `worktree_remove`:

```rust
/// Remove a worktree.
pub fn worktree_remove(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let layout = RepoLayout::detect(repo_path)?;
    let status = Command::new("git")
        .args(["worktree", "remove"])
        .arg(worktree_path)
        .current_dir(layout.git_dir())
        .status()
        .context("Failed to run git worktree remove")?;

    if !status.success() {
        bail!("git worktree remove failed for {}", worktree_path.display());
    }

    Ok(())
}
```

Replace `worktree_prune`:

```rust
/// Prune stale worktree entries.
pub fn worktree_prune(repo_path: &Path) -> Result<()> {
    let layout = RepoLayout::detect(repo_path)?;
    let status = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(layout.git_dir())
        .status()
        .context("Failed to run git worktree prune")?;

    if !status.success() {
        bail!("git worktree prune failed");
    }

    Ok(())
}
```

Replace `list_remote_branches`:

```rust
/// List remote branches for a repository.
pub fn list_remote_branches(repo_path: &Path) -> Result<Vec<String>> {
    let layout = RepoLayout::detect(repo_path)?;
    let output = Command::new("git")
        .args(["branch", "-r", "--format=%(refname:short)"])
        .current_dir(layout.git_dir())
        .output()
        .context("Failed to run git branch -r")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git branch -r failed: {stderr}");
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

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib git::`
Expected: PASS — all `git` tests, including `worktree_list_works_for_container_layout`.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat: make worktree list/remove/prune layout-aware"
```

---

## Task 6: `clone_bare` produces the container layout

**Files:**
- Modify: `src/git.rs`

`clone_bare` now clones into `<container>/.bare`, writes the `.git` pointer
file, and fixes the fetch refspec so remote branches appear as
`refs/remotes/origin/*` (otherwise `git branch -r` lists nothing and the
`grove open` branch picker is empty).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn clone_bare_produces_container_layout() {
        let tmp = tempfile::tempdir().unwrap();

        // A bare repo to act as the clone source.
        let source = tmp.path().join("source");
        init_bare_at(&source, "master").unwrap();

        let parent = tmp.path().join("dest");
        std::fs::create_dir_all(&parent).unwrap();

        let result = clone_bare(source.to_str().unwrap(), &parent).unwrap();

        assert_eq!(result.name, "source");
        assert_eq!(result.path, parent.join("source"));
        assert!(result.path.join(".bare").join("HEAD").is_file());
        assert_eq!(
            std::fs::read_to_string(result.path.join(".git")).unwrap(),
            "gitdir: ./.bare\n"
        );
        assert!(matches!(
            RepoLayout::detect(&result.path).unwrap(),
            RepoLayout::Container { .. }
        ));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib git::tests::clone_bare`
Expected: FAIL — the current `clone_bare` produces a bare repo directly at `parent/source`, so `parent/source/.bare/HEAD` does not exist.

- [ ] **Step 3: Rework `clone_bare`**

Replace the existing `clone_bare` function in `src/git.rs` with:

```rust
/// Clone a repository into the `.bare` container layout under `parent_dir`.
pub fn clone_bare(url: &str, parent_dir: &Path) -> Result<CloneResult> {
    let name = repo_name_from_url(url)?;
    let container = parent_dir.join(&name);

    if container.exists() {
        bail!("Destination already exists: {}", container.display());
    }

    let bare = container.join(".bare");
    let status = Command::new("git")
        .args(["clone", "--bare", url])
        .arg(&bare)
        .status()
        .context("Failed to run git clone")?;

    if !status.success() {
        // Clean up a partially-created container.
        let _ = std::fs::remove_dir_all(&container);
        bail!("git clone --bare failed for {url}");
    }

    write_gitdir_file(&container)?;

    // A plain `--bare` clone maps remote branches into local refs/heads/*.
    // Rewrite the refspec and re-fetch so they appear as origin/* tracking
    // branches, which `grove open`'s branch picker reads.
    run_git(
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
        &bare,
    )?;
    run_git(&["fetch", "origin"], &bare)?;

    Ok(CloneResult {
        path: container,
        name,
    })
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib git::tests::clone_bare`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat: clone into .bare container layout"
```

---

## Task 7: `migrate_to_container` and `has_uncommitted_tracked_changes`

**Files:**
- Modify: `src/git.rs`

`migrate_to_container` converts a legacy bare repo in place to the container
layout. `has_uncommitted_tracked_changes` checks a worktree for staged or
unstaged changes to tracked files — untracked files are deliberately ignored,
because in the legacy layout git's own admin files show up as untracked noise.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src/git.rs`:

```rust
    #[test]
    fn migrate_converts_legacy_bare_to_container() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("legacy");

        // Build a legacy bare repo with a legacy-style worktree (reproducing
        // the old collision: worktree placed under <bare>/worktrees/).
        init_bare_at(&repo, "master").unwrap();
        let legacy_wt = repo.join("worktrees").join("feat");
        run_git(
            &[
                "worktree",
                "add",
                "-B",
                "feat",
                legacy_wt.to_str().unwrap(),
            ],
            &repo,
        )
        .unwrap();

        migrate_to_container(&repo).unwrap();

        // Container layout in place at the same path.
        assert!(repo.join(".bare").join("HEAD").is_file());
        assert_eq!(
            std::fs::read_to_string(repo.join(".git")).unwrap(),
            "gitdir: ./.bare\n"
        );
        // Legacy worktree directories are gone.
        assert!(!repo.join(".bare").join("worktrees").exists());

        // Branches are preserved.
        let bare = repo.join(".bare");
        let branches =
            run_git_capture(&["branch", "--format=%(refname:short)"], &bare).unwrap();
        assert!(branches.lines().any(|b| b == "master"));
        assert!(branches.lines().any(|b| b == "feat"));
    }

    #[test]
    fn has_uncommitted_tracked_changes_detects_modifications() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();
        let wt = worktree_add(&container, "master").unwrap();

        // Clean worktree: no tracked changes.
        assert!(!has_uncommitted_tracked_changes(&wt).unwrap());

        // Add and commit a file, then modify it.
        std::fs::write(wt.join("file.txt"), "one\n").unwrap();
        run_git(&["add", "file.txt"], &wt).unwrap();
        run_git(
            &[
                "-c",
                "user.name=grove",
                "-c",
                "user.email=grove@localhost",
                "commit",
                "-m",
                "add file",
            ],
            &wt,
        )
        .unwrap();
        assert!(!has_uncommitted_tracked_changes(&wt).unwrap());

        std::fs::write(wt.join("file.txt"), "two\n").unwrap();
        assert!(has_uncommitted_tracked_changes(&wt).unwrap());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib git::tests::migrate git::tests::has_uncommitted`
Expected: FAIL — `cannot find function migrate_to_container` / `has_uncommitted_tracked_changes`.

- [ ] **Step 3: Implement the two functions**

Add to `src/git.rs` (after `init_repo`):

```rust
/// Returns true if the worktree has staged or unstaged changes to tracked
/// files. Untracked files are ignored: in the legacy layout git's own
/// per-worktree admin files appear as untracked noise.
pub fn has_uncommitted_tracked_changes(worktree_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(worktree_path)
        .output()
        .context("Failed to run git status")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git status failed: {stderr}");
    }

    Ok(!output.stdout.is_empty())
}

/// Convert a legacy bare repository at `repo_path` into the `.bare` container
/// layout, in place. Assumes `repo_path` is currently a legacy bare repo.
///
/// Legacy worktree directories are discarded — branches are preserved in the
/// bare data; recreate worktrees with `grove open`.
pub fn migrate_to_container(repo_path: &Path) -> Result<()> {
    let parent = repo_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot migrate a filesystem root"))?;
    let name = repo_path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine repo directory name"))?;

    let staging = parent.join(format!("{}.grove-migrate-tmp", name.to_string_lossy()));
    if staging.exists() {
        bail!("Migration staging path already exists: {}", staging.display());
    }

    // Move the bare data aside so the original path can become the container.
    std::fs::rename(repo_path, &staging)
        .with_context(|| format!("Failed to move {} aside", repo_path.display()))?;

    let result = (|| -> Result<()> {
        std::fs::create_dir(repo_path)
            .with_context(|| format!("Failed to create container {}", repo_path.display()))?;
        let bare = repo_path.join(".bare");
        std::fs::rename(&staging, &bare)
            .with_context(|| format!("Failed to move bare data into {}", bare.display()))?;
        write_gitdir_file(repo_path)?;

        // Drop the tangled legacy worktree directories and their admin
        // entries, then prune any dangling registrations.
        let worktrees = bare.join("worktrees");
        if worktrees.exists() {
            std::fs::remove_dir_all(&worktrees)
                .with_context(|| format!("Failed to remove {}", worktrees.display()))?;
        }
        run_git(&["worktree", "prune"], &bare)?;
        Ok(())
    })();

    // Best-effort rollback if migration failed before the bare data landed.
    if result.is_err() && staging.exists() && !repo_path.join(".bare").exists() {
        let _ = std::fs::remove_dir(repo_path);
        let _ = std::fs::rename(&staging, repo_path);
    }

    result
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib git::`
Expected: PASS — all `git` tests.

- [ ] **Step 5: Commit**

```bash
git add src/git.rs
git commit -m "feat: add migrate_to_container and dirty-worktree check"
```

---

## Task 8: Split `cmd/repo.rs` into a `cmd/repo/` module

**Files:**
- Create: `src/cmd/repo/mod.rs`
- Create: `src/cmd/repo/add.rs`
- Create: `src/cmd/repo/rm.rs`
- Create: `src/cmd/repo/list.rs`
- Delete: `src/cmd/repo.rs`

Pure refactor — no behavior change. `cmd/mod.rs` already declares `pub mod
repo;`, which works for a directory module. `main.rs` calls `cmd::repo::add`,
`cmd::repo::rm`, `cmd::repo::list`; `mod.rs` re-exports them so `main.rs` is
unchanged.

- [ ] **Step 1: Create `src/cmd/repo/mod.rs`**

```rust
mod add;
mod list;
mod rm;

pub use add::add;
pub use list::list;
pub use rm::rm;
```

- [ ] **Step 2: Create `src/cmd/repo/add.rs`**

```rust
use anyhow::{bail, Result};
use std::path::PathBuf;

use crate::db::{Db, NewRepo};
use crate::git;

pub fn add(db: &Db, path: &str) -> Result<()> {
    let path = PathBuf::from(path)
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("Path does not exist: {path}"))?;

    // Verify it's a git repo (bare or normal).
    let worktrees = git::worktree_list(&path);
    if worktrees.is_err() {
        bail!("{} does not appear to be a git repository", path.display());
    }

    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Could not determine repo name from path"))?;

    let repo = db.add_repo(&NewRepo {
        name,
        path: &path,
        url: None,
        directory: None,
    })?;

    println!("Tracking '{}' at {}", repo.name, repo.path.display());
    Ok(())
}
```

- [ ] **Step 3: Create `src/cmd/repo/rm.rs`**

```rust
use anyhow::{bail, Result};

use crate::db::Db;

pub fn rm(db: &Db, name: &str) -> Result<()> {
    if db.remove_repo(name)? {
        println!("Removed '{name}' from tracking");
    } else {
        bail!("No repo found with name '{name}'");
    }
    Ok(())
}
```

- [ ] **Step 4: Create `src/cmd/repo/list.rs`**

```rust
use anyhow::Result;
use tabled::settings::object::Rows;
use tabled::settings::style::Style;
use tabled::settings::Alignment;
use tabled::settings::Modify;
use tabled::{Table, Tabled};

use crate::db::{Db, Repo, RepoFilter};

#[derive(Tabled)]
struct RepoRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Dir")]
    dir: String,
    #[tabled(rename = "Score")]
    score: String,
    #[tabled(rename = "Path")]
    path: String,
}

impl From<&Repo> for RepoRow {
    fn from(repo: &Repo) -> Self {
        Self {
            name: repo.name.clone(),
            dir: repo.directory.as_deref().unwrap_or("-").to_string(),
            score: format!("{:.0}", repo.frecency),
            path: repo.path.display().to_string(),
        }
    }
}

pub fn list(db: &Db) -> Result<()> {
    let repos = db.list_repos(RepoFilter::default())?;

    if repos.is_empty() {
        println!("No repos tracked. Use 'grove clone' or 'grove repo add' to get started.");
        return Ok(());
    }

    let rows: Vec<RepoRow> = repos.iter().map(RepoRow::from).collect();
    let mut table = Table::new(rows);
    table
        .with(Style::markdown())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()));
    println!("{table}");

    Ok(())
}
```

- [ ] **Step 5: Delete the old file**

```bash
git rm src/cmd/repo.rs
```

- [ ] **Step 6: Verify the build and tests pass**

Run: `cargo build && cargo test`
Expected: compiles and all existing tests pass — no behavior change.

- [ ] **Step 7: Commit**

```bash
git add src/cmd/repo/
git commit -m "refactor: split cmd/repo.rs into a cmd/repo/ module"
```

---

## Task 9: `grove repo new` subcommand

**Files:**
- Modify: `src/cli.rs:71-82` (the `RepoCmd` enum)
- Modify: `src/cmd/clone.rs` (make `select_directory` reusable)
- Create: `src/cmd/repo/new.rs`
- Modify: `src/cmd/repo/mod.rs`
- Modify: `src/main.rs:34-38` (the `RepoCmd` match arm)

- [ ] **Step 1: Add the `New` variant to `RepoCmd`**

In `src/cli.rs`, inside `pub enum RepoCmd`, add this variant before `Rm`:

```rust
    /// Create a new repository in a configured directory
    New {
        /// Repository name
        name: String,
        /// Directory name from config (interactive if omitted)
        dir: Option<String>,
    },
```

- [ ] **Step 2: Make `select_directory` reusable**

In `src/cmd/clone.rs`, change the visibility of `select_directory` from
private to crate-visible. Change its signature line:

```rust
fn select_directory(config: &Config) -> Result<String> {
```

to:

```rust
pub(crate) fn select_directory(config: &Config) -> Result<String> {
```

- [ ] **Step 3: Create `src/cmd/repo/new.rs`**

```rust
use anyhow::{bail, Result};

use crate::cmd::clone::select_directory;
use crate::config::Config;
use crate::db::{Db, NewRepo};
use crate::git;

/// Default branch for newly created repositories.
const DEFAULT_BRANCH: &str = "master";

pub fn new(db: &Db, config: &Config, name: &str, dir: Option<&str>) -> Result<()> {
    if config.directories.is_empty() {
        bail!(
            "No directories configured. Add directories to your config file:\n  {}",
            Config::path()?.display()
        );
    }

    let dir_name = match dir {
        Some(d) => d.to_string(),
        None => select_directory(config)?,
    };

    let parent = config
        .resolve_dir(&dir_name)
        .ok_or_else(|| anyhow::anyhow!("Directory '{dir_name}' not found in config"))?;

    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
    }

    let container = parent.join(name);
    git::init_repo(&container, DEFAULT_BRANCH)?;

    db.add_repo(&NewRepo {
        name,
        path: &container,
        url: None,
        directory: Some(&dir_name),
    })?;

    println!("Created repo '{name}' at {}", container.display());
    println!("Open it with: grove open {name} {DEFAULT_BRANCH}");
    Ok(())
}
```

- [ ] **Step 4: Re-export `new` from the module**

In `src/cmd/repo/mod.rs`, add the module and re-export:

```rust
mod add;
mod list;
mod new;
mod rm;

pub use add::add;
pub use list::list;
pub use new::new;
pub use rm::rm;
```

- [ ] **Step 5: Wire the subcommand in `main.rs`**

In `src/main.rs`, in the `Cmd::Repo(sub)` match block, add a `New` arm:

```rust
        Cmd::Repo(sub) => match sub {
            RepoCmd::New { name, dir } => {
                return cmd::repo::new(&db, &config, name, dir.as_deref())
            }
            RepoCmd::Add { path } => return cmd::repo::add(&db, path),
            RepoCmd::Rm { name } => return cmd::repo::rm(&db, name),
            RepoCmd::List => return cmd::repo::list(&db),
        },
```

- [ ] **Step 6: Verify the build and run a smoke test**

Run: `cargo build`
Expected: compiles cleanly.

Smoke test (assumes at least one configured directory named `work`; adjust to
a real directory from `grove config show`):

```bash
cargo run -- repo new smoketest work
cargo run -- repo list
```

Expected: prints `Created repo 'smoketest' at <dir>/smoketest`, and
`repo list` shows `smoketest`. Verify on disk:

```bash
ls -a <dir>/smoketest          # → .bare  .git
git --git-dir=<dir>/smoketest/.bare log --oneline master   # → one "Initial commit"
```

Clean up: `cargo run -- repo rm smoketest && rm -rf <dir>/smoketest`.

- [ ] **Step 7: Commit**

```bash
git add src/cli.rs src/cmd/clone.rs src/cmd/repo/ src/main.rs
git commit -m "feat: add 'grove repo new' subcommand"
```

---

## Task 10: `grove repo migrate` subcommand

**Files:**
- Modify: `src/cli.rs` (the `RepoCmd` enum)
- Create: `src/cmd/repo/migrate.rs`
- Modify: `src/cmd/repo/mod.rs`
- Modify: `src/main.rs` (the `RepoCmd` match arm)

- [ ] **Step 1: Add the `Migrate` variant to `RepoCmd`**

In `src/cli.rs`, inside `pub enum RepoCmd`, add this variant after `List`:

```rust
    /// Migrate a legacy bare repository to the .bare container layout
    Migrate {
        /// Repository name (interactive if omitted)
        name: Option<String>,
    },
```

- [ ] **Step 2: Create `src/cmd/repo/migrate.rs`**

```rust
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
```

- [ ] **Step 3: Re-export `migrate` from the module**

In `src/cmd/repo/mod.rs`, add the module and re-export:

```rust
mod add;
mod list;
mod migrate;
mod new;
mod rm;

pub use add::add;
pub use list::list;
pub use migrate::migrate;
pub use new::new;
pub use rm::rm;
```

- [ ] **Step 4: Wire the subcommand in `main.rs`**

In `src/main.rs`, in the `Cmd::Repo(sub)` match block, add a `Migrate` arm:

```rust
        Cmd::Repo(sub) => match sub {
            RepoCmd::New { name, dir } => {
                return cmd::repo::new(&db, &config, name, dir.as_deref())
            }
            RepoCmd::Migrate { name } => {
                return cmd::repo::migrate(&db, name.as_deref())
            }
            RepoCmd::Add { path } => return cmd::repo::add(&db, path),
            RepoCmd::Rm { name } => return cmd::repo::rm(&db, name),
            RepoCmd::List => return cmd::repo::list(&db),
        },
```

- [ ] **Step 5: Verify the build and run a smoke test**

Run: `cargo build`
Expected: compiles cleanly.

Smoke test — build a legacy bare repo, track it, migrate it:

```bash
# Create a legacy-style bare repo with a worktree.
git clone --bare https://github.com/seftontycho/grove /tmp/legacy-grove
git -C /tmp/legacy-grove worktree add -B master /tmp/legacy-grove/worktrees/master
cargo run -- repo add /tmp/legacy-grove
cargo run -- repo migrate legacy-grove
```

Expected: a confirmation prompt, then `Migrated 'legacy-grove' ...`. Verify:

```bash
ls -a /tmp/legacy-grove        # → .bare  .git
test ! -e /tmp/legacy-grove/.bare/worktrees && echo "worktrees pruned"
```

Clean up: `cargo run -- repo rm legacy-grove && rm -rf /tmp/legacy-grove`.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs src/cmd/repo/ src/main.rs
git commit -m "feat: add 'grove repo migrate' subcommand"
```

---

## Task 11: Remove the `zj-session-bar` zellij plugin

**Files:**
- Modify: `templates/zellij.kdl`
- Modify: `src/zellij.rs`

Removes the plugin pane from the layout template and all pipe-based in-session
switching. When grove runs inside a zellij session it now bails with a
"detach first" message. tmux is unaffected — it already switches sessions
in-place.

- [ ] **Step 1: Write the failing test**

Add a `#[cfg(test)]` module at the bottom of `src/zellij.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::DEFAULT_LAYOUT;

    #[test]
    fn default_layout_has_no_session_bar_plugin() {
        assert!(
            !DEFAULT_LAYOUT.contains("zj-session-bar"),
            "default zellij layout must not reference the zj-session-bar plugin"
        );
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib zellij::tests`
Expected: FAIL — `DEFAULT_LAYOUT` still contains `zj-session-bar`.

- [ ] **Step 3: Remove the plugin pane from the template**

Replace the `default_tab_template` block in `templates/zellij.kdl` so it no
longer contains the `zj-session-bar` plugin pane. The full file becomes:

```kdl
layout {
    cwd "{{ worktree_path }}"
    default_tab_template {
        pane size=1 borderless=true {
            plugin location="zellij:tab-bar"
        }
        children
        pane size=2 borderless=true {
            plugin location="zellij:status-bar"
        }
    }
    tab name="shell" focus=true {
        pane command="{{ shell }}" {}
    }
    tab name="editor" {
        pane command="nvim" {
            args "."
        }
    }
    tab name="opencode" {
        pane command="opencode" {}
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --lib zellij::tests`
Expected: PASS.

- [ ] **Step 5: Remove the plugin code from `src/zellij.rs`**

Delete the functions `plugin_url`, `pipe_switch_session`, and
`pipe_create_session` entirely.

Replace the `create_session` method body so it no longer branches on `$ZELLIJ`
and instead bails when inside a zellij session:

```rust
    fn create_session(&self, name: &SessionName, worktree_path: &Path, shell: &str) -> Result<()> {
        if std::env::var_os("ZELLIJ").is_some() {
            bail!(
                "You're inside a zellij session. Detach first (Ctrl-o d), \
                 then run grove again."
            );
        }

        let template = load_template("zellij.kdl", DEFAULT_LAYOUT)?;
        let ctx = TemplateContext {
            worktree_path: &worktree_path.to_string_lossy(),
            shell,
            session_name: &name.as_zellij_name(),
            repo: &name.repo,
            branch: &name.branch,
        };
        let layout =
            render_template(&template, &ctx).context("Failed to render zellij layout template")?;

        let path = layout_path(name);
        std::fs::write(&path, &layout)
            .with_context(|| format!("Failed to write layout to {}", path.display()))?;

        let zellij_name = name.as_zellij_name();
        let status = Command::new("zellij")
            .args(["-s", &zellij_name, "-n"])
            .arg(&path)
            .status()
            .context("Failed to run zellij")?;

        let _ = std::fs::remove_file(&path);
        if !status.success() {
            bail!("zellij session creation failed for '{name}'");
        }

        Ok(())
    }
```

Replace the `attach_session` method body so it also bails when inside zellij:

```rust
    fn attach_session(&self, name: &str) -> Result<()> {
        if std::env::var_os("ZELLIJ").is_some() {
            bail!(
                "You're inside a zellij session. Detach first (Ctrl-o d), \
                 then run grove again."
            );
        }

        let status = Command::new("zellij")
            .args(["attach", name])
            .status()
            .context("Failed to run zellij attach")?;
        if !status.success() {
            bail!("Failed to attach to zellij session '{name}'");
        }
        Ok(())
    }
```

- [ ] **Step 6: Fix imports and verify the build**

The `PathBuf` import in `src/zellij.rs` is now only needed by `layout_path`,
which still uses it — keep `use std::path::{Path, PathBuf};` as-is. Remove any
import that the compiler now flags as unused.

Run: `cargo build && cargo test --lib zellij`
Expected: compiles with no warnings; zellij tests pass.

- [ ] **Step 7: Commit**

```bash
git add templates/zellij.kdl src/zellij.rs
git commit -m "feat: remove broken zj-session-bar zellij plugin"
```

---

## Task 12: Update documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/src/reference/commands.md`
- Modify: `docs/src/reference/layout.md`
- Modify: `docs/src/usage/clone.md`
- Modify: `docs/src/usage/repos.md`
- Modify: `docs/src/usage/worktrees.md`
- Modify: `docs/src/how-it-works.md`

- [ ] **Step 1: Update the README "Getting started" section**

In `README.md`, replace the `## Getting started` code block so it mentions the
container layout and the new `repo new` command:

````markdown
## Getting started

```sh
# 1. Set up the shell alias
eval "$(grove init zsh)"   # or bash / fish

# 2. Configure your project directories
grove config edit

# 3. Bring in a repo — clone an existing one…
grove clone git@github.com:your-org/your-repo.git work

#    …or create a brand-new one
grove repo new your-repo work

# 4. Open a worktree + session (interactive)
gv
```

Each repo is stored as a **container directory**: a hidden `.bare/` holding the
git data, and one clean subdirectory per worktree. No bare-repo plumbing in
your way.
````

- [ ] **Step 2: Update `docs/src/reference/commands.md`**

Change the `grove clone` description line from:

```
Clone a remote repository as a bare repo into a configured directory.
```

to:

```
Clone a remote repository into a configured directory, using the `.bare` container layout.
```

Add two new subsections under `## grove repo`, immediately after the
`### grove repo list` block:

````markdown
### `grove repo new`

Create a new, empty repository in the `.bare` container layout. The repo is
seeded with a single empty commit on `master` so it is immediately usable.

```
grove repo new <name> [dir]
```

| Argument | Description |
|----------|-------------|
| `name` | Name of the repository (becomes the container directory name) |
| `dir` | Named directory from config. Interactive if omitted. |

### `grove repo migrate`

Convert a legacy bare repository to the `.bare` container layout, in place.
Refuses if any worktree has uncommitted changes; existing worktrees are
discarded (branches are preserved — recreate worktrees with `grove open`).

```
grove repo migrate [name]
```

| Argument | Description |
|----------|-------------|
| `name` | Repo name. Interactive if omitted. |
````

- [ ] **Step 3: Update the layout example path in `docs/src/reference/layout.md`**

In the "Template variables" table, change the `worktree_path` example from:

```
/home/you/work/myrepo/worktrees/main
```

to:

```
/home/you/work/myrepo/main
```

- [ ] **Step 4: Update the usage and how-it-works pages**

In `docs/src/usage/clone.md`, `docs/src/usage/repos.md`,
`docs/src/usage/worktrees.md`, and `docs/src/how-it-works.md`, update the prose
to describe the container layout. Apply these consistent facts everywhere the
old model is described:

- A tracked repo is a **container directory** containing a hidden `.bare/`
  (the git data) and one subdirectory per worktree — e.g.
  `~/work/myrepo/.bare`, `~/work/myrepo/master`, `~/work/myrepo/feat-auth`.
- Worktrees are direct children of the container (`<repo>/<branch>`), not under
  a `worktrees/` subdirectory.
- `grove repo new` creates a new repo; `grove clone` clones an existing one;
  `grove repo migrate` upgrades a legacy bare repo.
- Remove any description of a visible zellij session-switcher bar / in-session
  switching: inside a zellij session grove now asks you to detach first.

Replace any literal `<repo>/worktrees/<branch>` paths with `<repo>/<branch>`.

- [ ] **Step 5: Verify the docs build**

Run: `cd docs && mdbook build && cd ..`
Expected: builds with no errors. (If `mdbook` is not installed, instead grep
for stale references — `grep -rn "worktrees/" docs/src` and
`grep -rn "session.bar\|zj-session" docs/src README.md` should return nothing
unintended.)

- [ ] **Step 6: Commit**

```bash
git add README.md docs/
git commit -m "docs: describe .bare container layout and new repo commands"
```

---

## Self-Review

**Spec coverage:**
- Part 1 (`.bare` container layout, `RepoLayout`) → Tasks 2, 4, 5.
- Part 2 (`grove clone` updated) → Task 6.
- Part 3 (`grove repo new`) → Tasks 3, 9.
- Part 4 (`grove repo migrate`) → Tasks 7, 10.
- Part 5 (remove `zj-session-bar`) → Task 11.
- Code organization (`cmd/repo/` module) → Task 8.
- Testing (git-layer unit/integration tests) → Tasks 2–7, 11.
- Docs → Task 12.
- No SQLite schema change — confirmed: no task touches `src/db/mod.rs`'s
  `migrate`.

**Note on the spec vs. plan:** the spec sketched `open.rs` / `tree.rs` calling
`RepoLayout::detect` directly. The plan instead keeps detection internal to the
`git.rs` functions, so caller signatures and `open.rs` / `tree.rs` are
unchanged. This is a strictly smaller, safer diff that meets the same goal;
`RepoLayout` is still `pub` because `cmd/repo/migrate.rs` uses it.

**Type consistency:** `RepoLayout` (`Container`/`LegacyBare`), `git_dir()`,
`worktree_base()`, `worktree_path()`, `init_repo`, `clone_bare`,
`migrate_to_container`, `has_uncommitted_tracked_changes`, `run_git`,
`run_git_capture`, `write_gitdir_file`, `init_bare_at` — names are used
consistently across tasks. `CloneResult { path, name }` is unchanged from the
existing struct.

**Placeholder scan:** no TBD/TODO; every code step contains complete code;
every command step states expected output.
