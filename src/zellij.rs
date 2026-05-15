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
        ensure_not_inside_zellij()?;

        let zellij_name = name.as_zellij_name();

        // Delete any dead/exited session with the same name so we can create a
        // fresh one. `delete-session` is required for exited sessions —
        // `kill-session` only works on live sessions. A non-existent name
        // returns non-zero, so we intentionally ignore the exit status.
        let _ = Command::new("zellij")
            .args(["delete-session", &zellij_name])
            .output();

        let template = load_template("zellij.kdl", DEFAULT_LAYOUT)?;
        let ctx = TemplateContext {
            worktree_path: &worktree_path.to_string_lossy(),
            shell,
            session_name: &zellij_name,
            repo: &name.repo,
            branch: &name.branch,
        };
        let layout =
            render_template(&template, &ctx).context("Failed to render zellij layout template")?;

        let path = layout_path(name);
        std::fs::write(&path, &layout)
            .with_context(|| format!("Failed to write layout to {}", path.display()))?;

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

    fn list_sessions(&self) -> Result<Vec<Session>> {
        let output = Command::new("zellij")
            .args(["list-sessions", "--no-formatting"])
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
            // Skip dead/exited sessions — they cannot be switched to via the
            // plugin pipe and will be cleaned up when a new session is created.
            .filter(|l| !l.contains("EXITED"))
            .filter_map(|l| {
                // Format: "name [Created ...ago] (current)" or "name [Created ...ago]"
                let name = l.split_whitespace().next()?;
                Some(Session {
                    name: name.to_string(),
                })
            })
            .collect();

        Ok(sessions)
    }

    fn attach_session(&self, name: &str) -> Result<()> {
        ensure_not_inside_zellij()?;

        let status = Command::new("zellij")
            .args(["attach", name])
            .status()
            .context("Failed to run zellij attach")?;
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

/// Returns an error if grove is running inside an existing zellij session.
/// grove cannot create or switch sessions from within one — the user must
/// detach first.
fn ensure_not_inside_zellij() -> Result<()> {
    if std::env::var_os("ZELLIJ").is_some() {
        bail!(
            "You're inside a zellij session. Detach first (Ctrl-o d), \
             then run grove again."
        );
    }
    Ok(())
}

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
