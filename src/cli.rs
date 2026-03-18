use crate::config::Shell;
use clap::{Parser, Subcommand};
use clap_complete::Shell as CompletionShell;

#[derive(Parser)]
#[command(
    name = "grove",
    version,
    about = "Manage git worktrees and zellij sessions"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// Print the shell function for `gv` alias
    Init {
        /// Shell to generate the function for
        shell: Shell,
    },

    /// Clone a bare repository into a configured directory
    Clone {
        /// Git remote URL to clone
        url: String,
        /// Directory name from config (interactive if omitted)
        dir: Option<String>,
    },

    /// Open a worktree and zellij session
    Open {
        /// Repo name or fuzzy query
        query: Option<String>,
        /// Branch name (interactive if omitted)
        branch: Option<String>,
    },

    /// Manage tracked repositories
    #[command(subcommand)]
    Repo(RepoCmd),

    /// Manage worktrees
    #[command(subcommand)]
    Tree(TreeCmd),

    /// Manage zellij sessions
    #[command(subcommand)]
    Session(SessionCmd),

    /// Manage configuration
    #[command(subcommand)]
    Config(ConfigCmd),

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: CompletionShell,
    },
}

#[derive(Subcommand)]
pub enum RepoCmd {
    /// Track an existing local repository
    Add {
        /// Path to the bare repository
        path: String,
    },
    /// Stop tracking a repository
    Rm {
        /// Repository name
        name: String,
    },
    /// List tracked repositories
    List,
}

#[derive(Subcommand)]
pub enum TreeCmd {
    /// List worktrees for a repository
    List {
        /// Repo name or fuzzy query (interactive if omitted)
        repo: Option<String>,
    },
    /// Close a worktree and its zellij session
    Close {
        /// Repo name or fuzzy query (interactive if omitted)
        query: Option<String>,
    },
    /// Prune stale worktree entries
    Prune {
        /// Repo name or fuzzy query (interactive if omitted)
        repo: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum SessionCmd {
    /// List active zellij sessions
    List,
    /// Attach to an existing session
    Attach {
        /// Session name
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ConfigCmd {
    /// Show current configuration
    Show,
    /// Open configuration in $EDITOR
    Edit,
}
