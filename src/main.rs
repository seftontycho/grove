mod cli;
mod cmd;
mod config;
mod db;
mod git;
mod zellij;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Cmd, ConfigCmd, RepoCmd, SessionCmd, TreeCmd};
use config::Config;
use db::Db;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load()?;
    let db = Db::open(&config::db_path()?)?;

    match &cli.command {
        Cmd::Init { shell } => cmd::init::run(shell),
        Cmd::Clone { url, dir } => cmd::clone::run(&db, &config, url, dir.as_deref()),
        Cmd::Open { query, branch } => {
            cmd::open::run(&db, &config, query.as_deref(), branch.as_deref())
        }
        Cmd::Repo(sub) => match sub {
            RepoCmd::Add { path } => cmd::repo::add(&db, path),
            RepoCmd::Rm { name } => cmd::repo::rm(&db, name),
            RepoCmd::List => cmd::repo::list(&db),
        },
        Cmd::Tree(sub) => match sub {
            TreeCmd::List { repo } => cmd::tree::list(&db, repo.as_deref()),
            TreeCmd::Close { query } => cmd::tree::close(&db, query.as_deref()),
            TreeCmd::Prune { repo } => cmd::tree::prune(&db, repo.as_deref()),
        },
        Cmd::Session(sub) => match sub {
            SessionCmd::List => cmd::session::list(),
            SessionCmd::Attach { name } => cmd::session::attach(name),
        },
        Cmd::Config(sub) => match sub {
            ConfigCmd::Show => cmd::config::show(),
            ConfigCmd::Edit => cmd::config::edit(),
        },
    }
}
