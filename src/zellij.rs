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
        if std::env::var_os("ZELLIJ").is_some() {
            // Already inside a zellij session — pipe to zj-session-bar plugin
            // which calls switch_session_with_layout via the zellij plugin API.
            // The layout file must remain on disk until zellij reads it.
            pipe_create_session(&zellij_name, &path)?;
        } else {
            let status = Command::new("zellij")
                .args(["-s", &zellij_name, "-n"])
                .arg(&path)
                .status()
                .context("Failed to run zellij")?;

            let _ = std::fs::remove_file(&path);
            if !status.success() {
                bail!("zellij session creation failed for '{name}'");
            }
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
        if std::env::var_os("ZELLIJ").is_some() {
            pipe_switch_session(name)
        } else {
            let status = Command::new("zellij")
                .args(["attach", name])
                .status()
                .context("Failed to run zellij attach")?;
            if !status.success() {
                bail!("Failed to attach to zellij session '{name}'");
            }
            Ok(())
        }
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

/// Resolve the zj-session-bar plugin URL for pipe commands.
fn plugin_url() -> String {
    // Check common plugin locations.
    let candidates = [
        directories::BaseDirs::new().map(|d| d.data_dir().join("zellij/plugins/zj-session-bar.wasm")),
        Some(PathBuf::from("/usr/share/zellij/plugins/zj-session-bar.wasm")),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return format!("file:{}", candidate.display());
        }
    }
    // Fall back to name-only (works in layouts but may not work in pipe).
    "zj-session-bar".to_string()
}

/// Send a `switch_session` pipe message to the zj-session-bar plugin.
fn pipe_switch_session(session_name: &str) -> Result<()> {
    let url = plugin_url();
    let output = Command::new("zellij")
        .args(["pipe", "--plugin", &url, "--name", "switch_session", "--", session_name])
        .output()
        .context("Failed to switch zellij session via pipe")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to switch zellij session '{session_name}': {stderr}");
    }
    Ok(())
}

/// Send a `switch_session` pipe message with a layout file to create and switch.
fn pipe_create_session(session_name: &str, layout_path: &Path) -> Result<()> {
    let url = plugin_url();
    let output = Command::new("zellij")
        .args([
            "pipe",
            "--plugin",
            &url,
            "--name",
            "switch_session",
            "--args",
            &format!("layout={}", layout_path.display()),
            "--",
            session_name,
        ])
        .output()
        .context("Failed to create zellij session via pipe")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Failed to create zellij session '{session_name}': {stderr}");
    }
    Ok(())
}

fn layout_path(name: &SessionName) -> PathBuf {
    std::env::temp_dir().join(format!(
        "grove-{}.kdl",
        name.as_zellij_name().replace('/', "-")
    ))
}
