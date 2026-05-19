use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// Convert a legacy bare repository at `repo_path` into the `.bare` container
/// layout, in place. Assumes `repo_path` is currently a legacy bare repo.
///
/// The conversion (move aside → create container → move bare data in → write
/// the `.git` pointer) either fully completes or is rolled back to the
/// original legacy layout.
///
/// Existing worktrees are preserved: each checkout is relocated to
/// `<container>/<branch>` and its git admin files are disentangled into a
/// clean admin directory. Worktree relocation is best-effort — a worktree that
/// cannot be relocated is left for `grove open` to recreate and does not fail
/// the migration.
pub fn migrate_to_container(repo_path: &Path) -> Result<()> {
    let repo_path = repo_path
        .canonicalize()
        .with_context(|| format!("Cannot access {}", repo_path.display()))?;
    let repo_path = repo_path.as_path();

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

    // Enumerate worktrees before moving anything — their registrations point
    // at the current on-disk paths. Each entry is (branch, path relative to
    // the bare repo) so it can be located again after the bare data moves.
    let linked: Vec<(String, PathBuf)> = worktree_list(repo_path)
        .unwrap_or_default()
        .into_iter()
        .filter(|wt| !wt.is_bare)
        .filter_map(|wt| {
            let branch = wt
                .branch
                .as_deref()?
                .strip_prefix("refs/heads/")?
                .to_string();
            let rel = wt.path.strip_prefix(repo_path).ok()?.to_path_buf();
            Some((branch, rel))
        })
        .collect();

    // Move the bare data aside so the original path can become the container.
    std::fs::rename(repo_path, &staging)
        .with_context(|| format!("Failed to move {} aside", repo_path.display()))?;

    // Convert the layout. Every step here is rolled back as a unit on failure.
    let bare = repo_path.join(".bare");
    let convert = (|| -> Result<()> {
        std::fs::create_dir(repo_path)
            .with_context(|| format!("Failed to create container {}", repo_path.display()))?;
        std::fs::rename(&staging, &bare)
            .with_context(|| format!("Failed to move bare data into {}", bare.display()))?;
        write_gitdir_file(repo_path)?;
        Ok(())
    })();

    if let Err(e) = convert {
        // Roll back to the original legacy layout, undoing whatever happened.
        if bare.exists() && !staging.exists() {
            let _ = std::fs::rename(&bare, &staging);
        }
        let _ = std::fs::remove_file(repo_path.join(".git"));
        let _ = std::fs::remove_dir(repo_path);
        let _ = std::fs::rename(&staging, repo_path);
        return Err(e.context(format!(
            "Migration rolled back; {} left unchanged",
            repo_path.display()
        )));
    }

    // The layout is converted. Relocate each worktree into it — best-effort,
    // so a single failure does not undo the migration.
    for (branch, rel) in &linked {
        if let Err(e) = relocate_worktree(repo_path, &bare, branch, rel) {
            eprintln!(
                "Warning: migrated, but could not relocate worktree '{branch}': {e:#}\n  \
                 recreate it later with `grove open {} {branch}`",
                name.to_string_lossy()
            );
        }
    }
    let _ = run_git(&["worktree", "prune"], &bare);

    Ok(())
}

/// Relocate one legacy worktree into the container layout: move its checkout
/// to `<container>/<branch>` and disentangle the git admin files (which the
/// legacy layout tangled into the checkout) into a clean admin directory.
fn relocate_worktree(container: &Path, bare: &Path, branch: &str, rel: &Path) -> Result<()> {
    // A `/` in the branch means git's admin directory was never collided into
    // the checkout the way it is for flat branch names; leave those for
    // `grove open` to recreate.
    if branch.contains('/') {
        bail!("branch names containing '/' are not auto-relocated");
    }

    // After the bare data moved, the tangled worktree directory sits at the
    // same path relative to the (now nested) bare repo.
    let tangled = bare.join(rel);
    if !tangled.join("HEAD").is_file() {
        bail!("{} is not a recognizable legacy worktree", tangled.display());
    }

    let new_checkout = container.join(branch);
    if new_checkout.exists() {
        bail!("{} already exists", new_checkout.display());
    }
    let new_admin = bare.join("worktrees").join(branch);

    // Move the whole tangled directory to the checkout location, then pull the
    // git admin files back out into a clean admin directory.
    std::fs::rename(&tangled, &new_checkout).with_context(|| {
        format!(
            "Failed to move {} to {}",
            tangled.display(),
            new_checkout.display()
        )
    })?;
    std::fs::create_dir_all(&new_admin)
        .with_context(|| format!("Failed to create admin dir {}", new_admin.display()))?;

    for entry in admin_entries(&new_checkout) {
        let file_name = entry
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("admin entry has no name"))?;
        std::fs::rename(&entry, new_admin.join(file_name))
            .with_context(|| format!("Failed to relocate admin entry {}", entry.display()))?;
    }

    // Replace the checkout's self-referential `.git` with a pointer to the
    // admin directory, and link the admin directory back to the checkout.
    let _ = std::fs::remove_file(new_checkout.join(".git"));
    std::fs::write(new_admin.join("commondir"), "../..\n")
        .with_context(|| format!("Failed to write {}/commondir", new_admin.display()))?;
    std::fs::write(
        new_admin.join("gitdir"),
        format!("{}\n", new_checkout.join(".git").display()),
    )
    .with_context(|| format!("Failed to write {}/gitdir", new_admin.display()))?;
    std::fs::write(
        new_checkout.join(".git"),
        format!("gitdir: {}\n", new_admin.display()),
    )
    .with_context(|| format!("Failed to write {}/.git", new_checkout.display()))?;

    Ok(())
}

/// Git per-worktree admin files. In the legacy layout these were tangled into
/// the worktree checkout alongside the project's own files.
fn admin_entries(worktree_dir: &Path) -> Vec<PathBuf> {
    const ADMIN_FILES: &[&str] = &[
        "HEAD",
        "ORIG_HEAD",
        "FETCH_HEAD",
        "MERGE_HEAD",
        "MERGE_MSG",
        "MERGE_MODE",
        "MERGE_RR",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "AUTO_MERGE",
        "BISECT_LOG",
        "BISECT_START",
        "COMMIT_EDITMSG",
        "index",
        "commondir",
        "gitdir",
        "packed-refs",
        "sparse-checkout",
        "shallow",
    ];

    let mut found: Vec<PathBuf> = ADMIN_FILES
        .iter()
        .map(|n| worktree_dir.join(n))
        .filter(|p| p.is_file())
        .collect();

    // `logs/` — only git's, identified by a `logs/HEAD` file, so a project
    // directory that happens to be named `logs` is left alone.
    let logs = worktree_dir.join("logs");
    if logs.is_dir() && logs.join("HEAD").is_file() {
        found.push(logs);
    }

    found
}

/// The result of a successful clone operation.
pub struct CloneResult {
    pub path: PathBuf,
    pub name: String,
}

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

    // `git clone --bare` populated refs/heads/* with the source's branches.
    // Those duplicate the refs/remotes/origin/* tracking refs we now have,
    // and worse, they prevent `git worktree add -b <branch> <path>
    // origin/<branch>` from setting up tracking later. Delete every local
    // head except the default branch, and configure its upstream so git
    // pull/push work in its worktree.
    //
    // If HEAD is detached (unusual but possible for sources without a
    // symbolic HEAD), there is no "default branch" to preserve; skip the
    // cleanup entirely rather than failing the clone.
    if let Ok(default_branch) =
        run_git_capture(&["symbolic-ref", "--short", "HEAD"], &bare)
    {
        let default_ref = format!("refs/heads/{default_branch}");
        let heads = run_git_capture(
            &["for-each-ref", "--format=%(refname)", "refs/heads/"],
            &bare,
        )?;
        for head in heads.lines() {
            if head == default_ref {
                continue;
            }
            run_git(&["update-ref", "-d", head], &bare)?;
        }
        run_git(
            &[
                "config",
                &format!("branch.{default_branch}.remote"),
                "origin",
            ],
            &bare,
        )?;
        run_git(
            &[
                "config",
                &format!("branch.{default_branch}.merge"),
                &default_ref,
            ],
            &bare,
        )?;
    }

    Ok(CloneResult {
        path: container,
        name,
    })
}

/// Extract a repository name from a URL.
///
/// Handles HTTPS and SSH URLs:
///   https://github.com/user/repo.git -> repo
///   git@github.com:user/repo.git    -> repo
pub fn repo_name_from_url(url: &str) -> Result<String> {
    let basename = url
        .rsplit('/')
        .next()
        .or_else(|| url.rsplit(':').next())
        .context("Could not extract repo name from URL")?;

    let name = basename.trim_end_matches(".git");

    if name.is_empty() {
        bail!("Could not extract repo name from URL: {url}");
    }

    Ok(name.to_string())
}

/// A worktree entry as reported by `git worktree list`.
#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub head: String,
    pub branch: Option<String>,
    pub is_bare: bool,
}

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

fn parse_worktree_list(output: &str) -> Result<Vec<Worktree>> {
    let mut worktrees = Vec::new();
    let mut path: Option<PathBuf> = None;
    let mut head = String::new();
    let mut branch: Option<String> = None;
    let mut is_bare = false;

    for line in output.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(p));
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            head = h.to_string();
        } else if let Some(b) = line.strip_prefix("branch ") {
            branch = Some(b.to_string());
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() {
            if let Some(p) = path.take() {
                worktrees.push(Worktree {
                    path: p,
                    head: std::mem::take(&mut head),
                    branch: branch.take(),
                    is_bare,
                });
                is_bare = false;
            }
        }
    }

    // Handle last entry (no trailing blank line)
    if let Some(p) = path.take() {
        worktrees.push(Worktree {
            path: p,
            head,
            branch,
            is_bare,
        });
    }

    Ok(worktrees)
}

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

/// Fetch all remotes (with prune). Used to refresh remote-tracking refs
/// before the branch picker. Surfaces git's stderr on failure so the
/// caller's warning ("Warning: fetch failed: ...") names the real cause
/// (auth, network, bad config) instead of just "fetch failed".
pub fn fetch_all(repo_path: &Path) -> Result<()> {
    let layout = RepoLayout::detect(repo_path)?;
    let output = Command::new("git")
        .args(["fetch", "--all", "--prune"])
        .current_dir(layout.git_dir())
        .output()
        .context("Failed to run git fetch --all --prune")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git fetch --all --prune failed: {stderr}");
    }
    Ok(())
}

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
        // Drop symbolic refs: `git branch -r --format=%(refname:short)` shortens
        // `refs/remotes/<remote>/HEAD` to the bare `<remote>` for the typical
        // single-segment remote name (no slash), and to `<remote>/HEAD` for
        // older git versions or trailing-HEAD cases. Both forms are filtered.
        // Edge case: a remote whose name itself contains a slash would produce
        // `<foo>/<bar>` here and survive; we accept that since `git remote add
        // foo/bar` is exceedingly rare in practice.
        .filter(|l| l.contains('/') && !l.ends_with("/HEAD"))
        .collect();

    Ok(branches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_name_from_https_url() {
        let name = repo_name_from_url("https://github.com/user/myrepo.git").unwrap();
        assert_eq!(name, "myrepo");
    }

    #[test]
    fn test_repo_name_from_ssh_url() {
        let name = repo_name_from_url("git@github.com:user/myrepo.git").unwrap();
        assert_eq!(name, "myrepo");
    }

    #[test]
    fn test_repo_name_no_git_suffix() {
        let name = repo_name_from_url("https://github.com/user/myrepo").unwrap();
        assert_eq!(name, "myrepo");
    }

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

    #[test]
    fn migrate_converts_legacy_bare_to_container() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("legacy");

        // Build a legacy bare repo with a legacy-style worktree (reproducing
        // the old collision: worktree placed under <bare>/worktrees/).
        init_bare_at(&repo, "master").unwrap();
        let legacy_wt = repo.join("worktrees").join("feat");
        run_git(
            &["worktree", "add", "-B", "feat", legacy_wt.to_str().unwrap()],
            &repo,
        )
        .unwrap();
        // Leave an uncommitted change in the worktree — it must survive.
        std::fs::write(legacy_wt.join("scratch.txt"), "work in progress\n").unwrap();

        migrate_to_container(&repo).unwrap();

        // Container layout in place at the same path.
        assert!(repo.join(".bare").join("HEAD").is_file());
        assert_eq!(
            std::fs::read_to_string(repo.join(".git")).unwrap(),
            "gitdir: ./.bare\n"
        );

        // The worktree was relocated to <container>/<branch>, not discarded,
        // and its uncommitted file came along.
        let feat = repo.join("feat");
        assert!(feat.join(".git").is_file());
        assert_eq!(
            std::fs::read_to_string(feat.join("scratch.txt")).unwrap(),
            "work in progress\n"
        );

        // git recognizes the relocated worktree and its HEAD is intact.
        let trees = worktree_list(&repo).unwrap();
        assert!(
            trees.iter().any(|t| !t.is_bare && t.path.ends_with("feat")),
            "relocated worktree 'feat' should be listed"
        );
        let head_branch =
            run_git_capture(&["rev-parse", "--abbrev-ref", "HEAD"], &feat).unwrap();
        assert_eq!(head_branch, "feat");

        // Branches are preserved.
        let bare = repo.join(".bare");
        let branches =
            run_git_capture(&["branch", "--format=%(refname:short)"], &bare).unwrap();
        assert!(branches.lines().any(|b| b == "master"));
        assert!(branches.lines().any(|b| b == "feat"));
    }

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

    #[test]
    fn clone_bare_leaves_only_default_branch_local() {
        let tmp = tempfile::tempdir().unwrap();

        let source = tmp.path().join("source");
        init_bare_at(&source, "master").unwrap();

        // Add feat + fix to source.
        let master_commit =
            run_git_capture(&["rev-parse", "refs/heads/master"], &source).unwrap();
        for branch in ["feat", "fix"] {
            run_git(
                &["update-ref", &format!("refs/heads/{branch}"), &master_commit],
                &source,
            )
            .unwrap();
        }

        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(&dest).unwrap();
        let cloned = clone_bare(source.to_str().unwrap(), &dest).unwrap();
        let bare = cloned.path.join(".bare");

        // Local refs/heads should only contain the default branch (master).
        let locals = run_git_capture(&["branch", "--format=%(refname:short)"], &bare).unwrap();
        let mut locals: Vec<&str> = locals.lines().collect();
        locals.sort();
        assert_eq!(locals, vec!["master"]);

        // Remote-tracking refs should contain ALL the source's branches.
        let remotes =
            run_git_capture(&["branch", "-r", "--format=%(refname:short)"], &bare).unwrap();
        assert!(remotes.lines().any(|r| r == "origin/feat"));
        assert!(remotes.lines().any(|r| r == "origin/fix"));
        assert!(remotes.lines().any(|r| r == "origin/master"));

        // Default branch should have upstream set so git pull/push work.
        let remote_cfg =
            run_git_capture(&["config", "branch.master.remote"], &bare).unwrap();
        let merge_cfg =
            run_git_capture(&["config", "branch.master.merge"], &bare).unwrap();
        assert_eq!(remote_cfg, "origin");
        assert_eq!(merge_cfg, "refs/heads/master");
    }

    #[test]
    fn worktree_list_works_for_container_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();
        let wt = worktree_add(&container, "master", WorktreeSource::ExistingLocal).unwrap();

        // git's worktree list output reports canonicalized paths. On macOS
        // `$TMPDIR` lives under `/var/folders/...` which is a symlink to
        // `/private/var/folders/...`, so compare canonical forms.
        let wt = wt.canonicalize().unwrap();
        let trees = worktree_list(&container).unwrap();
        assert!(trees
            .iter()
            .any(|t| t.path.canonicalize().ok() == Some(wt.clone()) && !t.is_bare));
    }

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

    /// Test helper: create `branch` in `bare`, advanced one empty commit past
    /// `parent_branch`. Returns the new commit SHA so callers can assert
    /// against it. Uses plumbing so no working tree is needed.
    fn seed_branch(bare: &Path, parent_branch: &str, branch: &str, msg: &str) -> String {
        let empty_tree =
            run_git_capture(&["hash-object", "-w", "-t", "tree", "--stdin"], bare).unwrap();
        let parent = run_git_capture(
            &["rev-parse", &format!("refs/heads/{parent_branch}")],
            bare,
        )
        .unwrap();
        let commit = run_git_capture(
            &[
                "-c",
                "user.name=grove",
                "-c",
                "user.email=grove@localhost",
                "commit-tree",
                empty_tree.as_str(),
                "-p",
                parent.as_str(),
                "-m",
                msg,
            ],
            bare,
        )
        .unwrap();
        run_git(
            &["update-ref", &format!("refs/heads/{branch}"), &commit],
            bare,
        )
        .unwrap();
        commit
    }

    #[test]
    fn worktree_add_existing_local_does_not_reset() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();
        let bare = container.join(".bare");

        let feat_commit = seed_branch(&bare, "master", "feat", "feat work");

        // Open the existing local branch as a worktree.
        let wt = worktree_add(&container, "feat", WorktreeSource::ExistingLocal).unwrap();

        // The worktree's HEAD must point at the existing feat commit,
        // not at master.
        let head = run_git_capture(&["rev-parse", "HEAD"], &wt).unwrap();
        assert_eq!(head, feat_commit, "existing local branch must not be reset");
    }

    #[test]
    fn worktree_add_tracking_remote_sets_upstream() {
        let tmp = tempfile::tempdir().unwrap();

        // Build a bare "remote" with a `feat` branch.
        let source = tmp.path().join("source");
        init_bare_at(&source, "master").unwrap();
        let feat_commit = seed_branch(&source, "master", "feat", "feat on remote");

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
        seed_branch(&source, "master", "feat", "feat on remote");

        // fetch_all should bring the new branch into the clone.
        fetch_all(&cloned.path).unwrap();

        let after =
            run_git_capture(&["branch", "-r", "--format=%(refname:short)"], &bare).unwrap();
        assert!(
            after.lines().any(|b| b == "origin/feat"),
            "fetch_all should pull origin/feat\nactual: {after}"
        );
    }

    #[test]
    fn test_parse_worktree_list() {
        let output = "\
worktree /home/user/repo
HEAD abc123
branch refs/heads/main
bare

worktree /home/user/repo/worktrees/feat
HEAD def456
branch refs/heads/feat

";
        let trees = parse_worktree_list(output).unwrap();
        assert_eq!(trees.len(), 2);
        assert!(trees[0].is_bare);
        assert!(!trees[1].is_bare);
        assert_eq!(trees[1].branch.as_deref(), Some("refs/heads/feat"));
    }
}
