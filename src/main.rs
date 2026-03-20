mod cli;
mod cmd;
mod config;
mod db;
mod git;
mod multiplexer;
mod tmux;
mod zellij;

use anyhow::{bail, Result};
use clap::Parser;

use cli::{Cli, Cmd, ConfigCmd, RepoCmd, SessionCmd, TreeCmd};
use config::{Config, ResolvedBackend};
use db::Db;
use multiplexer::Multiplexer;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;
    config.warn_if_invalid();
    let db = Db::open(&config::db_path()?)?;

    match &cli.command {
        Cmd::Init { shell } => return cmd::init::run(shell),
        Cmd::Clone { url, dir } => return cmd::clone::run(&db, &config, url, dir.as_deref()),
        Cmd::Completions { shell } => return cmd::completions::run(*shell),
        Cmd::Config(sub) => match sub {
            ConfigCmd::Show => return cmd::config::show(),
            ConfigCmd::Edit => return cmd::config::edit(),
        },
        Cmd::Repo(sub) => match sub {
            RepoCmd::Add { path } => return cmd::repo::add(&db, path),
            RepoCmd::Rm { name } => return cmd::repo::rm(&db, name),
            RepoCmd::List => return cmd::repo::list(&db),
        },
        _ => {}
    }

    // Commands below require a multiplexer backend.
    let mux: Box<dyn Multiplexer> = match config.multiplexer.resolve() {
        Some(ResolvedBackend::Zellij) => Box::new(zellij::ZellijBackend::new()),
        Some(ResolvedBackend::Tmux) => Box::new(tmux::TmuxBackend::new()),
        None => bail!(
            "No terminal multiplexer found. Install zellij or tmux, \
             or set `multiplexer = \"zellij\"` / `multiplexer = \"tmux\"` in your grove config."
        ),
    };

    match &cli.command {
        Cmd::Open { query, branch } => cmd::open::run(
            &db,
            &config,
            mux.as_ref(),
            query.as_deref(),
            branch.as_deref(),
        ),
        Cmd::Tree(sub) => match sub {
            TreeCmd::List { repo } => cmd::tree::list(&db, repo.as_deref()),
            TreeCmd::Close { query } => cmd::tree::close(&db, mux.as_ref(), query.as_deref()),
            TreeCmd::Prune { repo } => cmd::tree::prune(&db, repo.as_deref()),
        },
        Cmd::Session(sub) => match sub {
            SessionCmd::List => cmd::session::list(mux.as_ref()),
            SessionCmd::Attach { name } => cmd::session::attach(mux.as_ref(), name),
        },
        // Already handled above — unreachable.
        _ => unreachable!(),
    }
}
