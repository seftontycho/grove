use anyhow::{Context, Result};
use rusqlite::{params, Connection, Row};
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use super::{NewRepo, RepoFilter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoStatus {
    Active,
    Archived,
}

impl RepoStatus {
    fn as_str(&self) -> &'static str {
        match self {
            RepoStatus::Active => "active",
            RepoStatus::Archived => "archived",
        }
    }
}

impl fmt::Display for RepoStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for RepoStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "active" => Ok(RepoStatus::Active),
            "archived" => Ok(RepoStatus::Archived),
            other => anyhow::bail!("Unknown repo status: {other}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Repo {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub url: Option<String>,
    pub directory: Option<String>,
    pub status: RepoStatus,
    pub frecency: f64,
    pub last_accessed_at: Option<String>,
    pub created_at: String,
}

fn row_to_repo(row: &Row<'_>) -> rusqlite::Result<Repo> {
    let status_str: String = row.get("status")?;
    let status = status_str
        .parse::<RepoStatus>()
        .unwrap_or(RepoStatus::Active);
    let path_str: String = row.get("path")?;
    Ok(Repo {
        id: row.get("id")?,
        name: row.get("name")?,
        path: PathBuf::from(path_str),
        url: row.get("url")?,
        directory: row.get("directory")?,
        status,
        frecency: row.get("frecency")?,
        last_accessed_at: row.get("last_accessed_at")?,
        created_at: row.get("created_at")?,
    })
}

pub fn insert(conn: &Connection, repo: &NewRepo<'_>) -> Result<Repo> {
    let path_str = repo.path.to_string_lossy();
    conn.execute(
        "INSERT INTO repos (name, path, url, directory) VALUES (?1, ?2, ?3, ?4)",
        params![repo.name, path_str, repo.url, repo.directory],
    )
    .with_context(|| format!("Failed to add repo '{}'", repo.name))?;

    let id = conn.last_insert_rowid();
    let mut stmt = conn.prepare("SELECT * FROM repos WHERE id = ?1")?;
    stmt.query_row(params![id], row_to_repo)
        .context("Failed to read back inserted repo")
}

pub fn remove(conn: &Connection, name: &str) -> Result<bool> {
    let affected = conn.execute("DELETE FROM repos WHERE name = ?1", params![name])?;
    Ok(affected > 0)
}

pub fn list(conn: &Connection, filter: RepoFilter) -> Result<Vec<Repo>> {
    let mut sql = String::from("SELECT * FROM repos WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref status) = filter.status {
        sql.push_str(" AND status = ?");
        param_values.push(Box::new(status.as_str().to_string()));
    }
    if let Some(ref dir) = filter.directory {
        sql.push_str(" AND directory = ?");
        param_values.push(Box::new(dir.clone()));
    }

    sql.push_str(" ORDER BY frecency DESC, name ASC");

    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params.as_slice(), row_to_repo)?;

    let mut repos = Vec::new();
    for row in rows {
        repos.push(row?);
    }
    Ok(repos)
}

pub fn find(conn: &Connection, query: &str) -> Result<Option<Repo>> {
    // Exact name match first
    let mut stmt = conn.prepare("SELECT * FROM repos WHERE name = ?1")?;
    if let Ok(repo) = stmt.query_row(params![query], row_to_repo) {
        return Ok(Some(repo));
    }

    // Fuzzy match: name contains query, ordered by frecency
    let pattern = format!("%{query}%");
    let mut stmt =
        conn.prepare("SELECT * FROM repos WHERE name LIKE ?1 ORDER BY frecency DESC LIMIT 1")?;
    match stmt.query_row(params![pattern], row_to_repo) {
        Ok(repo) => Ok(Some(repo)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Maximum sum of all frecency scores before a global decay is triggered.
const FRECENCY_MAX_TOTAL: f64 = 1000.0;

/// Decay factor applied to all scores when the total exceeds the threshold.
const FRECENCY_DECAY: f64 = 0.9;

/// Minimum score below which an entry is considered negligible after decay.
const FRECENCY_FLOOR: f64 = 0.01;

pub fn touch(conn: &Connection, id: i64) -> Result<()> {
    // Increment the accessed repo's score and update timestamp
    conn.execute(
        "UPDATE repos SET frecency = frecency + 1.0, last_accessed_at = datetime('now') WHERE id = ?1",
        params![id],
    )?;

    // Check if a global decay pass is needed
    let total: f64 = conn.query_row("SELECT COALESCE(SUM(frecency), 0.0) FROM repos", [], |r| {
        r.get(0)
    })?;

    if total > FRECENCY_MAX_TOTAL {
        decay_all(conn)?;
    }

    Ok(())
}

/// Apply multiplicative decay to all frecency scores and floor tiny values.
fn decay_all(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE repos SET frecency = frecency * ?1",
        params![FRECENCY_DECAY],
    )?;
    // Zero out scores that have decayed below the floor
    conn.execute(
        "UPDATE repos SET frecency = 0.0 WHERE frecency < ?1",
        params![FRECENCY_FLOOR],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE repos (
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
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_touch_increments_frecency() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO repos (name, path) VALUES ('testrepo', '/tmp/testrepo')",
            [],
        )
        .unwrap();

        touch(&conn, 1).unwrap();
        touch(&conn, 1).unwrap();
        touch(&conn, 1).unwrap();

        let score: f64 = conn
            .query_row("SELECT frecency FROM repos WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert!((score - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_decay_triggers_at_threshold() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO repos (name, path, frecency) VALUES ('big', '/tmp/big', ?1)",
            params![FRECENCY_MAX_TOTAL],
        )
        .unwrap();

        // This touch pushes total above the threshold, triggering decay
        touch(&conn, 1).unwrap();

        let score: f64 = conn
            .query_row("SELECT frecency FROM repos WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        // After +1 then *0.9: (1001) * 0.9 = 900.9
        let expected = (FRECENCY_MAX_TOTAL + 1.0) * FRECENCY_DECAY;
        assert!((score - expected).abs() < 0.1);
    }

    #[test]
    fn test_decay_floors_tiny_scores() {
        let conn = setup_db();
        conn.execute(
            "INSERT INTO repos (name, path, frecency) VALUES ('big', '/tmp/big', ?1)",
            params![FRECENCY_MAX_TOTAL + 1.0],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO repos (name, path, frecency) VALUES ('tiny', '/tmp/tiny', 0.005)",
            [],
        )
        .unwrap();

        // Touch big to trigger decay
        touch(&conn, 1).unwrap();

        let tiny_score: f64 = conn
            .query_row("SELECT frecency FROM repos WHERE id = 2", [], |r| r.get(0))
            .unwrap();
        // 0.005 * 0.9 = 0.0045, which is below FRECENCY_FLOOR
        assert!(tiny_score == 0.0);
    }
}
