use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
}

impl Shell {
    pub fn detect() -> Self {
        match std::env::var("SHELL") {
            Ok(s) if s.contains("fish") => Shell::Fish,
            Ok(s) if s.contains("bash") => Shell::Bash,
            _ => Shell::Zsh,
        }
    }
}

impl fmt::Display for Shell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Shell::Zsh => write!(f, "zsh"),
            Shell::Bash => write!(f, "bash"),
            Shell::Fish => write!(f, "fish"),
        }
    }
}

impl std::str::FromStr for Shell {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "zsh" => Ok(Shell::Zsh),
            "bash" => Ok(Shell::Bash),
            "fish" => Ok(Shell::Fish),
            other => anyhow::bail!("Unsupported shell: {other}. Use zsh, bash, or fish."),
        }
    }
}

/// Which terminal multiplexer backend grove should use.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MultiplexerBackend {
    /// Use zellij.
    Zellij,
    /// Use tmux.
    Tmux,
    /// Detect automatically: prefer the currently-running multiplexer
    /// ($ZELLIJ / $TMUX env vars), then fall back to whichever binary is
    /// found on $PATH first (zellij wins on a tie).
    #[default]
    Auto,
}

impl MultiplexerBackend {
    /// Resolve `Auto` to a concrete backend by inspecting the environment.
    /// Returns `None` if `Auto` is set but neither multiplexer is available.
    pub fn resolve(&self) -> Option<ResolvedBackend> {
        match self {
            MultiplexerBackend::Zellij => Some(ResolvedBackend::Zellij),
            MultiplexerBackend::Tmux => Some(ResolvedBackend::Tmux),
            MultiplexerBackend::Auto => {
                // Prefer the currently-running multiplexer.
                if std::env::var("ZELLIJ").is_ok() {
                    return Some(ResolvedBackend::Zellij);
                }
                if std::env::var("TMUX").is_ok() {
                    return Some(ResolvedBackend::Tmux);
                }
                // Fall back to whichever is on PATH.
                if is_on_path("zellij") {
                    return Some(ResolvedBackend::Zellij);
                }
                if is_on_path("tmux") {
                    return Some(ResolvedBackend::Tmux);
                }
                None
            }
        }
    }
}

impl fmt::Display for MultiplexerBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MultiplexerBackend::Zellij => write!(f, "zellij"),
            MultiplexerBackend::Tmux => write!(f, "tmux"),
            MultiplexerBackend::Auto => write!(f, "auto"),
        }
    }
}

/// A concrete, resolved multiplexer (no `Auto` variant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBackend {
    Zellij,
    Tmux,
}

fn is_on_path(binary: &str) -> bool {
    std::process::Command::new("which")
        .arg(binary)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "Shell::detect")]
    pub shell: Shell,
    #[serde(default)]
    pub multiplexer: MultiplexerBackend,
    #[serde(default)]
    pub directories: BTreeMap<String, DirectoryEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shell: Shell::detect(),
            multiplexer: MultiplexerBackend::Auto,
            directories: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            let config = Config::default();
            config.save()?;
            return Ok(config);
        }
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        let config: Config =
            toml::from_str(&contents).with_context(|| "Failed to parse config file")?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string_pretty(self)?;
        fs::write(&path, contents)?;
        Ok(())
    }

    pub fn path() -> Result<PathBuf> {
        let dirs = project_dirs()?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Validate config and return any warnings.
    /// Does not fail - issues are returned as warning strings.
    pub fn validate(&self) -> Vec<ConfigWarning> {
        let mut warnings = Vec::new();

        for (name, entry) in &self.directories {
            if !entry.path.exists() {
                warnings.push(ConfigWarning::DirectoryMissing {
                    name: name.clone(),
                    path: entry.path.clone(),
                });
            } else if !entry.path.is_dir() {
                warnings.push(ConfigWarning::NotADirectory {
                    name: name.clone(),
                    path: entry.path.clone(),
                });
            }
        }

        warnings
    }

    /// Print any config warnings to stderr.
    pub fn warn_if_invalid(&self) {
        for warning in self.validate() {
            eprintln!("warning: {warning}");
        }
    }

    pub fn dir_names(&self) -> Vec<&str> {
        self.directories.keys().map(|s| s.as_str()).collect()
    }

    pub fn resolve_dir(&self, name: &str) -> Option<&Path> {
        self.directories.get(name).map(|e| e.path.as_path())
    }
}

/// A non-fatal issue found during config validation.
#[derive(Debug, Clone)]
pub enum ConfigWarning {
    DirectoryMissing { name: String, path: PathBuf },
    NotADirectory { name: String, path: PathBuf },
}

impl fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigWarning::DirectoryMissing { name, path } => {
                write!(
                    f,
                    "configured directory '{name}' does not exist: {}",
                    path.display()
                )
            }
            ConfigWarning::NotADirectory { name, path } => {
                write!(
                    f,
                    "configured directory '{name}' is not a directory: {}",
                    path.display()
                )
            }
        }
    }
}

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "grove").context("Could not determine project directories")
}

pub fn data_dir() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    let data = dirs.data_dir().to_path_buf();
    fs::create_dir_all(&data)?;
    Ok(data)
}

pub fn db_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("grove.db"))
}
