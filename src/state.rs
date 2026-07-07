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
                location: location.to_string(),
            },
        );
    }

    pub fn remove(&mut self, name: &str) -> Option<InstalledTool> {
        self.tools.remove(name)
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
