use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The result of a successful clone operation.
pub struct CloneResult {
    pub path: PathBuf,
    pub name: String,
}

/// Clone a bare repository into `parent_dir`.
pub fn clone_bare(url: &str, parent_dir: &Path) -> Result<CloneResult> {
    let name = repo_name_from_url(url)?;
    let dest = parent_dir.join(&name);

    if dest.exists() {
        bail!("Destination already exists: {}", dest.display());
    }

    let status = Command::new("git")
        .args(["clone", "--bare", url])
        .arg(&dest)
        .status()
        .context("Failed to run git clone")?;

    if !status.success() {
        bail!("git clone --bare failed for {url}");
    }

    Ok(CloneResult { path: dest, name })
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
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
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
    let worktree_dir = repo_path.join("worktrees").join(branch);

    let status = Command::new("git")
        .args(["worktree", "add", "-B", branch])
        .arg(&worktree_dir)
        .current_dir(repo_path)
        .status()
        .context("Failed to run git worktree add")?;

    if !status.success() {
        bail!("git worktree add failed for branch '{branch}'");
    }

    Ok(worktree_dir)
}

/// Remove a worktree.
pub fn worktree_remove(repo_path: &Path, worktree_path: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["worktree", "remove"])
        .arg(worktree_path)
        .current_dir(repo_path)
        .status()
        .context("Failed to run git worktree remove")?;

    if !status.success() {
        bail!("git worktree remove failed for {}", worktree_path.display());
    }

    Ok(())
}

/// Prune stale worktree entries.
pub fn worktree_prune(repo_path: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(repo_path)
        .status()
        .context("Failed to run git worktree prune")?;

    if !status.success() {
        bail!("git worktree prune failed");
    }

    Ok(())
}

/// List remote branches for a repository.
pub fn list_remote_branches(repo_path: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["branch", "-r", "--format=%(refname:short)"])
        .current_dir(repo_path)
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
