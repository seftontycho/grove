use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

use crate::multiplexer::{
    load_template, render_template, Multiplexer, Session, SessionName, TemplateContext,
};

/// Built-in default tmux layout template (3 windows: shell, editor, opencode).
const DEFAULT_LAYOUT: &str = include_str!("../templates/tmux.sh");

/// Tmux multiplexer backend.
pub struct TmuxBackend;

impl TmuxBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Multiplexer for TmuxBackend {
    fn create_session(&self, name: &SessionName, worktree_path: &Path, shell: &str) -> Result<()> {
        let session_name = name.as_tmux_name();

        // Create the session detached so we can set it up before attaching.
        let status = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-c",
                &worktree_path.to_string_lossy(),
            ])
            .status()
            .context("Failed to run tmux new-session")?;

        if !status.success() {
            bail!("tmux session creation failed for '{name}'");
        }

        // Render and execute the layout script.
        let template = load_template("tmux.sh", DEFAULT_LAYOUT)?;
        let ctx = TemplateContext {
            worktree_path: &worktree_path.to_string_lossy(),
            shell,
            session_name: &session_name,
            repo: &name.repo,
            branch: &name.branch,
        };
        let script =
            render_template(&template, &ctx).context("Failed to render tmux layout template")?;

        let status = Command::new("sh")
            .arg("-c")
            .arg(&script)
            .status()
            .context("Failed to execute tmux layout script")?;

        if !status.success() {
            bail!("tmux layout script failed for '{name}'");
        }

        // Attach (or switch if already inside tmux).
        self.attach_session(&session_name)
    }

    fn list_sessions(&self) -> Result<Vec<Session>> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()
            .context("Failed to run tmux list-sessions")?;

        if !output.status.success() {
            // tmux returns non-zero when no sessions exist.
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let sessions = stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| Session {
                name: l.trim().to_string(),
            })
            .collect();

        Ok(sessions)
    }

    fn attach_session(&self, name: &str) -> Result<()> {
        // If we are already inside a tmux session, switch rather than nest.
        let status = if std::env::var("TMUX").is_ok() {
            Command::new("tmux")
                .args(["switch-client", "-t", name])
                .status()
                .context("Failed to run tmux switch-client")?
        } else {
            Command::new("tmux")
                .args(["attach-session", "-t", name])
                .status()
                .context("Failed to run tmux attach-session")?
        };

        if !status.success() {
            bail!("Failed to attach to tmux session '{name}'");
        }

        Ok(())
    }

    fn kill_session(&self, name: &str) -> Result<()> {
        let status = Command::new("tmux")
            .args(["kill-session", "-t", name])
            .status()
            .context("Failed to run tmux kill-session")?;

        if !status.success() {
            bail!("Failed to kill tmux session '{name}'");
        }

        Ok(())
    }
}
