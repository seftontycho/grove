use anyhow::Result;
#[cfg(unix)]
use directories::ProjectDirs;
use minijinja::{Environment, Value, context};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

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
        self.as_zellij_name_with_max_bytes(zellij_session_name_max_bytes())
    }

    fn as_zellij_name_with_max_bytes(&self, max_bytes: usize) -> String {
        let repo = self.repo.replace('/', ":");
        let branch = self.branch.replace('/', ":");
        shorten_with_hash(&format!("{}:{}", repo, branch), max_bytes)
    }

    /// Sanitized form used by tmux (no `/` allowed): `repo-branch`.
    pub fn as_tmux_name(&self) -> String {
        let repo = self.repo.replace('/', "-");
        let branch = self.branch.replace('/', "-");
        format!("{}-{}", repo, branch)
    }
}

fn shorten_with_hash(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }

    // FNV-1a keeps shortened names stable across Grove versions and processes.
    let hash = value.bytes().fold(0x811c9dc5_u32, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(0x01000193)
    });
    let suffix = format!("-{hash:08x}");

    if max_bytes <= suffix.len() {
        return suffix[suffix.len() - max_bytes..].to_string();
    }

    let prefix_bytes = max_bytes - suffix.len();
    let prefix_end = value
        .char_indices()
        .take_while(|(index, character)| index + character.len_utf8() <= prefix_bytes)
        .map(|(index, character)| index + character.len_utf8())
        .last()
        .unwrap_or(0);
    format!("{}{}", &value[..prefix_end], suffix)
}

#[cfg(unix)]
fn zellij_session_name_max_bytes() -> usize {
    use std::os::unix::ffi::OsStrExt;

    let socket_root = std::env::var_os("ZELLIJ_SOCKET_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            ProjectDirs::from("org", "Zellij Contributors", "Zellij")
                .and_then(|dirs| dirs.runtime_dir().map(Path::to_path_buf))
        })
        .unwrap_or_else(|| {
            // SAFETY: geteuid has no preconditions and does not dereference pointers.
            let uid = unsafe { libc::geteuid() };
            std::env::temp_dir().join(format!("zellij-{uid}"))
        });
    let socket_dir = socket_root.join("contract_version_1");
    let socket_max_bytes: usize = if cfg!(target_os = "macos") { 104 } else { 108 };

    socket_max_bytes
        .saturating_sub(socket_dir.as_os_str().as_bytes().len())
        .saturating_sub(2)
}

#[cfg(not(unix))]
fn zellij_session_name_max_bytes() -> usize {
    usize::MAX
}

impl std::fmt::Display for SessionName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.repo, self.branch)
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionName, shorten_with_hash};

    #[test]
    fn zellij_name_fits_available_socket_path_budget() {
        let name = SessionName::new("spork", "mcasonsnow/REL-4655");

        assert!(name.as_zellij_name_with_max_bytes(24).len() <= 24);
    }

    #[test]
    fn shortened_zellij_names_remain_distinct() {
        let first = shorten_with_hash("spork:mcasonsnow:REL-4655", 24);
        let second = shorten_with_hash("spork:mcasonsnow:REL-4656", 24);

        assert_ne!(first, second);
    }

    #[test]
    fn short_zellij_names_are_unchanged() {
        assert_eq!(shorten_with_hash("grove:main", 24), "grove:main");
    }
}
