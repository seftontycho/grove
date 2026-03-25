use anyhow::Result;
use minijinja::{context, Environment, Value};
use std::path::Path;

use crate::config;

/// Variables available to every layout template.
///
/// # Template schema
///
/// | Variable        | Type   | Description                                      |
/// |-----------------|--------|--------------------------------------------------|
/// | `worktree_path` | string | Absolute path to the worktree directory          |
/// | `shell`         | string | User's shell binary name, e.g. `zsh`             |
/// | `session_name`  | string | Full session identifier, e.g. `repo/branch`      |
/// | `repo`          | string | Repository name                                  |
/// | `branch`        | string | Branch name                                      |
pub struct TemplateContext<'a> {
    pub worktree_path: &'a str,
    pub shell: &'a str,
    pub session_name: &'a str,
    pub repo: &'a str,
    pub branch: &'a str,
}

impl<'a> TemplateContext<'a> {
    /// Convert to a minijinja [`Value`] map for rendering.
    pub fn to_value(&self) -> Value {
        context! {
            worktree_path => self.worktree_path,
            shell         => self.shell,
            session_name  => self.session_name,
            repo          => self.repo,
            branch        => self.branch,
        }
    }
}

/// Render a template string with the given context using minijinja.
pub fn render_template(template_str: &str, ctx: &TemplateContext<'_>) -> Result<String> {
    let mut env = Environment::new();
    env.add_template("t", template_str)?;
    let tmpl = env.get_template("t")?;
    Ok(tmpl.render(ctx.to_value())?)
}

/// Load a user-provided template from the grove config directory, or fall back
/// to `builtin_default` if no override exists.
///
/// User templates live at:
///   `~/.config/grove/templates/<filename>`
pub fn load_template(filename: &str, builtin_default: &str) -> Result<String> {
    let config_dir = config::project_dirs()?.config_dir().to_path_buf();
    let user_path = config_dir.join("templates").join(filename);

    if user_path.exists() {
        let contents = std::fs::read_to_string(&user_path)?;
        Ok(contents)
    } else {
        Ok(builtin_default.to_string())
    }
}

/// A session as reported by a multiplexer backend.
#[derive(Debug, Clone)]
pub struct Session {
    pub name: String,
}

/// Common interface for terminal multiplexer backends.
pub trait Multiplexer {
    /// Create a new session for the given worktree.
    fn create_session(&self, name: &SessionName, worktree_path: &Path, shell: &str) -> Result<()>;

    /// Return all currently active sessions.
    fn list_sessions(&self) -> Result<Vec<Session>>;

    /// Attach the terminal to an existing session.
    fn attach_session(&self, name: &str) -> Result<()>;

    /// Destroy a session.
    fn kill_session(&self, name: &str) -> Result<()>;
}

/// Represents the components of a session name.
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

    /// Canonical form used by zellij: `repo:branch`.
    /// Zellij does not allow `/` in session names, so `/` is replaced with `:`.
    pub fn as_zellij_name(&self) -> String {
        let repo = self.repo.replace('/', ":");
        let branch = self.branch.replace('/', ":");
        format!("{}:{}", repo, branch)
    }

    /// Sanitized form used by tmux (no `/` allowed): `repo-branch`.
    pub fn as_tmux_name(&self) -> String {
        let repo = self.repo.replace('/', "-");
        let branch = self.branch.replace('/', "-");
        format!("{}-{}", repo, branch)
    }
}

impl std::fmt::Display for SessionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.repo, self.branch)
    }
}
