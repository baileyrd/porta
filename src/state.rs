//! The registry of what porta has installed, persisted as JSON at
//! `$PORTA_HOME/state.json`. This is porta's own bookkeeping — it doesn't
//! affect how a tool actually runs, only what `porta list`/`doctor` report
//! and what `porta uninstall` removes.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub tools: HashMap<String, InstalledTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledTool {
    pub version: String,
    pub strategy: String,
    pub installed_at_unix: u64,
    /// Where the installed binary (or, for `script` tools, the vendor's
    /// install directory) lives.
    pub location: String,
}

/// Locations under `$PORTA_HOME` are stored with a `${PORTA_HOME}` prefix
/// instead of the absolute path, so `state.json` stays correct when the
/// whole directory is copied to a machine with a different home path.
const PORTA_HOME_VAR: &str = "${PORTA_HOME}";

fn portable_location(location: &str) -> String {
    let home = crate::paths::porta_home().display().to_string();
    match location.strip_prefix(&home) {
        Some(rest) => format!("{PORTA_HOME_VAR}{rest}"),
        None => location.to_string(),
    }
}

/// Expand a stored location against the *current* `$PORTA_HOME`.
pub fn resolve_location(location: &str) -> String {
    match location.strip_prefix(PORTA_HOME_VAR) {
        Some(rest) => format!("{}{rest}", crate::paths::porta_home().display()),
        None => location.to_string(),
    }
}

impl State {
    pub fn load() -> Result<State> {
        let path = crate::paths::state_file();
        if !path.exists() {
            return Ok(State::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        crate::paths::ensure_layout()?;
        let path = crate::paths::state_file();
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))
    }

    pub fn record(&mut self, name: &str, version: &str, strategy: &str, location: &str) {
        self.tools.insert(
            name.to_string(),
            InstalledTool {
                version: version.to_string(),
                strategy: strategy.to_string(),
                installed_at_unix: now_unix(),
                location: portable_location(location),
            },
        );
    }

    pub fn remove(&mut self, name: &str) -> Option<InstalledTool> {
        self.tools.remove(name)
    }
}

impl InstalledTool {
    /// The location as an absolute path under the current `$PORTA_HOME`.
    pub fn resolved_location(&self) -> String {
        resolve_location(&self.location)
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations_under_porta_home_round_trip_portably() {
        let home = crate::paths::porta_home().display().to_string();

        let inside = format!("{home}/bin/claude");
        let stored = portable_location(&inside);
        assert_eq!(stored, "${PORTA_HOME}/bin/claude");
        assert_eq!(resolve_location(&stored), inside);

        // Paths outside PORTA_HOME (script-strategy installs) are untouched.
        let outside = "/home/someone/.local/bin";
        assert_eq!(portable_location(outside), outside);
        assert_eq!(resolve_location(outside), outside);
    }
}
