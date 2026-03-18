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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryEntry {
    pub path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "Shell::detect")]
    pub shell: Shell,
    #[serde(default)]
    pub directories: BTreeMap<String, DirectoryEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shell: Shell::detect(),
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

    pub fn dir_names(&self) -> Vec<&str> {
        self.directories.keys().map(|s| s.as_str()).collect()
    }

    pub fn resolve_dir(&self, name: &str) -> Option<&Path> {
        self.directories.get(name).map(|e| e.path.as_path())
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
