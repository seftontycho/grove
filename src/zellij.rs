use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::multiplexer::{
    load_template, render_template, Multiplexer, Session, SessionName, TemplateContext,
};

/// Built-in default KDL layout template (3 tabs: shell, editor, opencode).
const DEFAULT_LAYOUT: &str = include_str!("../templates/zellij.kdl");

/// Zellij multiplexer backend.
pub struct ZellijBackend;

impl ZellijBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Multiplexer for ZellijBackend {
    fn create_session(&self, name: &SessionName, worktree_path: &Path, shell: &str) -> Result<()> {
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
        let status = if std::env::var_os("ZELLIJ").is_some() {
            // Already inside a zellij session — create a detached background
            // session with the layout, then switch to it via the session-manager
            // plugin pipe message.
            let bg_status = Command::new("zellij")
                .args(["--layout"])
                .arg(&path)
                .args(["attach", "-b", &zellij_name])
                .status()
                .context("Failed to create background zellij session")?;

            if !bg_status.success() {
                let _ = std::fs::remove_file(&path);
                bail!("zellij session creation failed for '{name}'");
            }

            Command::new("zellij")
                .args([
                    "pipe",
                    "--plugin",
                    "zellij:session-manager",
                    "--name",
                    "switch_session",
                    "--",
                    &zellij_name,
                ])
                .status()
                .context("Failed to switch to zellij session")?
        } else {
            Command::new("zellij")
                .args(["-s", &zellij_name, "--layout"])
                .arg(&path)
                .status()
                .context("Failed to run zellij")?
        };

        // Clean up temp layout file regardless of outcome.
        let _ = std::fs::remove_file(&path);

        if !status.success() {
            bail!("zellij session creation failed for '{name}'");
        }

        Ok(())
    }

    fn list_sessions(&self) -> Result<Vec<Session>> {
        let output = Command::new("zellij")
            .args(["list-sessions", "--short"])
            .output()
            .context("Failed to run zellij list-sessions")?;

        if !output.status.success() {
            // zellij returns non-zero when no sessions exist.
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
        let status = if std::env::var_os("ZELLIJ").is_some() {
            Command::new("zellij")
                .args(["action", "switch-session", name])
                .status()
                .context("Failed to run zellij action switch-session")?
        } else {
            Command::new("zellij")
                .args(["attach", name])
                .status()
                .context("Failed to run zellij attach")?
        };

        if !status.success() {
            bail!("Failed to attach to zellij session '{name}'");
        }

        Ok(())
    }

    fn kill_session(&self, name: &str) -> Result<()> {
        let status = Command::new("zellij")
            .args(["kill-session", name])
            .status()
            .context("Failed to run zellij kill-session")?;

        if !status.success() {
            bail!("Failed to kill zellij session '{name}'");
        }

        Ok(())
    }
}

fn layout_path(name: &SessionName) -> PathBuf {
    std::env::temp_dir().join(format!(
        "grove-{}.kdl",
        name.as_zellij_name().replace('/', "-")
    ))
}
