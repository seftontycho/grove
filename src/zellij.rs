use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Represents the components of a zellij session name.
#[derive(Debug, Clone)]
pub struct SessionName {
    pub repo: String,
    pub branch: String,
}

impl SessionName {
    pub fn new(repo: &str, branch: &str) -> Self {
        Self {
            repo: repo.to_string(),
            branch: branch.to_string(),
        }
    }

    pub fn as_string(&self) -> String {
        format!("{}/{}", self.repo, self.branch)
    }
}

impl std::fmt::Display for SessionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.repo, self.branch)
    }
}

/// Status of a zellij session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Exited,
}

/// A zellij session as reported by `zellij list-sessions`.
#[derive(Debug, Clone)]
pub struct Session {
    pub name: String,
    pub status: SessionStatus,
}

/// Generate a KDL layout string for a worktree session.
pub fn generate_layout(worktree_path: &Path, shell: &str) -> String {
    let path = worktree_path.display();
    format!(
        r#"layout {{
    cwd "{path}"
    pane command="{shell}"
    pane command="nvim" {{
        args "."
    }}
    pane command="opencode"
}}"#
    )
}

/// Create a new zellij session with the given layout.
pub fn create_session(name: &SessionName, worktree_path: &Path, shell: &str) -> Result<()> {
    let layout = generate_layout(worktree_path, shell);

    let layout_path =
        std::env::temp_dir().join(format!("grove-{}.kdl", name.as_string().replace('/', "-")));
    std::fs::write(&layout_path, &layout)
        .with_context(|| format!("Failed to write layout to {}", layout_path.display()))?;

    let status = Command::new("zellij")
        .args(["--session", &name.as_string(), "--layout"])
        .arg(&layout_path)
        .status()
        .context("Failed to run zellij")?;

    // Clean up temp layout
    let _ = std::fs::remove_file(&layout_path);

    if !status.success() {
        bail!("zellij session creation failed for '{name}'");
    }

    Ok(())
}

/// List active zellij sessions.
pub fn list_sessions() -> Result<Vec<Session>> {
    let output = Command::new("zellij")
        .args(["list-sessions", "--short"])
        .output()
        .context("Failed to run zellij list-sessions")?;

    if !output.status.success() {
        // zellij returns non-zero if no sessions exist
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let sessions = stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let name = l.trim().to_string();
            Session {
                name,
                status: SessionStatus::Active,
            }
        })
        .collect();

    Ok(sessions)
}

/// Attach to an existing zellij session.
pub fn attach_session(name: &str) -> Result<()> {
    let status = Command::new("zellij")
        .args(["attach", name])
        .status()
        .context("Failed to run zellij attach")?;

    if !status.success() {
        bail!("Failed to attach to zellij session '{name}'");
    }

    Ok(())
}

/// Kill a zellij session.
pub fn kill_session(name: &str) -> Result<()> {
    let status = Command::new("zellij")
        .args(["kill-session", name])
        .status()
        .context("Failed to run zellij kill-session")?;

    if !status.success() {
        bail!("Failed to kill zellij session '{name}'");
    }

    Ok(())
}
