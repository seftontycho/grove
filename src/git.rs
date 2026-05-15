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
    fn worktree_list_works_for_container_layout() {
        let tmp = tempfile::tempdir().unwrap();
        let container = tmp.path().join("myrepo");
        init_repo(&container, "master").unwrap();
        let wt = worktree_add(&container, "master").unwrap();

        let trees = worktree_list(&container).unwrap();
        assert!(trees.iter().any(|t| t.path == wt && !t.is_bare));
    }

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
