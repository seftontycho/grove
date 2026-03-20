use anyhow::Result;

use crate::multiplexer::Multiplexer;

pub fn list(mux: &dyn Multiplexer) -> Result<()> {
    let sessions = mux.list_sessions()?;

    if sessions.is_empty() {
        println!("No active sessions.");
        return Ok(());
    }

    for session in &sessions {
        println!("  {}", session.name);
    }

    Ok(())
}

pub fn attach(mux: &dyn Multiplexer, name: &str) -> Result<()> {
    mux.attach_session(name)
}
