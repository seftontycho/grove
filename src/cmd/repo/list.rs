use anyhow::Result;
use tabled::settings::object::Rows;
use tabled::settings::style::Style;
use tabled::settings::Alignment;
use tabled::settings::Modify;
use tabled::{Table, Tabled};

use crate::db::{Db, Repo, RepoFilter};

#[derive(Tabled)]
struct RepoRow {
    #[tabled(rename = "Name")]
    name: String,
    #[tabled(rename = "Dir")]
    dir: String,
    #[tabled(rename = "Score")]
    score: String,
    #[tabled(rename = "Path")]
    path: String,
}

impl From<&Repo> for RepoRow {
    fn from(repo: &Repo) -> Self {
        Self {
            name: repo.name.clone(),
            dir: repo.directory.as_deref().unwrap_or("-").to_string(),
            score: format!("{:.0}", repo.frecency),
            path: repo.path.display().to_string(),
        }
    }
}

pub fn list(db: &Db) -> Result<()> {
    let repos = db.list_repos(RepoFilter::default())?;

    if repos.is_empty() {
        println!("No repos tracked. Use 'grove clone' or 'grove repo add' to get started.");
        return Ok(());
    }

    let rows: Vec<RepoRow> = repos.iter().map(RepoRow::from).collect();
    let mut table = Table::new(rows);
    table
        .with(Style::markdown())
        .with(Modify::new(Rows::new(1..)).with(Alignment::left()));
    println!("{table}");

    Ok(())
}
