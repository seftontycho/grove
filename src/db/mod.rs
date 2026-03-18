mod repo;

pub use repo::{Repo, RepoStatus};

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::path::Path;

pub struct Db {
    conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open DB: {}", path.display()))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS repos (
                id          INTEGER PRIMARY KEY,
                name        TEXT NOT NULL UNIQUE,
                path        TEXT NOT NULL UNIQUE,
                url         TEXT,
                directory   TEXT,
                status      TEXT NOT NULL DEFAULT 'active',
                frecency    REAL NOT NULL DEFAULT 0.0,
                last_accessed_at TEXT,
                created_at  TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )?;
        Ok(())
    }

    pub fn add_repo(&self, repo: &NewRepo) -> Result<Repo> {
        repo::insert(&self.conn, repo)
    }

    pub fn remove_repo(&self, name: &str) -> Result<bool> {
        repo::remove(&self.conn, name)
    }

    pub fn list_repos(&self, filter: RepoFilter) -> Result<Vec<Repo>> {
        repo::list(&self.conn, filter)
    }

    pub fn find_repo(&self, query: &str) -> Result<Option<Repo>> {
        repo::find(&self.conn, query)
    }

    pub fn touch_repo(&self, id: i64) -> Result<()> {
        repo::touch(&self.conn, id)
    }
}

/// Fields required to create a new repo entry.
pub struct NewRepo<'a> {
    pub name: &'a str,
    pub path: &'a Path,
    pub url: Option<&'a str>,
    pub directory: Option<&'a str>,
}

/// Filter criteria for listing repos.
#[derive(Default)]
pub struct RepoFilter {
    pub status: Option<RepoStatus>,
    pub directory: Option<String>,
}
