use std::process::Command;
use tracing::{error, info};

/// The terminal binary this daemon spawns.
pub const TERMINAL_COMMAND: &str = "cosmic-term";

/// The Wayland app_id that the spawned terminal will advertise.
pub const TERMINAL_APP_ID: &str = "com.system76.CosmicTerm";

pub struct SpawnResult {
    pub pid: u32,
}

pub fn spawn_terminal(args: &[String]) -> Option<SpawnResult> {
    let mut cmd = Command::new(TERMINAL_COMMAND);
    cmd.args(args);

    info!(
        "Spawning terminal: {} {:?} (tracking app_id={})",
        TERMINAL_COMMAND, args, TERMINAL_APP_ID
    );

    match cmd.spawn() {
        Ok(child) => {
            let pid = child.id();
            // Intentionally drop the Child handle — the terminal process is
            // independent and will be reaped via waitpid when it exits.
            drop(child);
            Some(SpawnResult { pid })
        }
        Err(e) => {
            error!("Failed to spawn terminal '{}': {}", TERMINAL_COMMAND, e);
            None
        }
    }
}
