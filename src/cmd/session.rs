use anyhow::Result;

use crate::zellij;

pub fn list() -> Result<()> {
    let sessions = zellij::list_sessions()?;

    if sessions.is_empty() {
        println!("No active zellij sessions.");
        return Ok(());
    }

    for session in &sessions {
        println!("  {}", session.name);
    }

    Ok(())
}

pub fn attach(name: &str) -> Result<()> {
    zellij::attach_session(name)
}
