pub mod binary;
pub mod script;
pub mod source;

use crate::manifest::Tool;
use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Script,
    Binary,
    Source,
}

impl Strategy {
    pub fn parse(s: &str) -> Result<Strategy> {
        match s {
            "script" => Ok(Strategy::Script),
            "binary" => Ok(Strategy::Binary),
            "source" => Ok(Strategy::Source),
            other => bail!("unknown --strategy `{other}` (expected script, binary, or source)"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Strategy::Script => "script",
            Strategy::Binary => "binary",
            Strategy::Source => "source",
        }
    }
}

pub struct Outcome {
    pub version: String,
    pub strategy: Strategy,
    /// Human-readable location of the result: an installed binary path, or
    /// (for `script` installs) the directory the vendor's installer used.
    pub location: String,
}

/// Install `tool`, honoring a forced `strategy` if given, otherwise picking
/// the best available one: a vendor `script` always wins (it's the tool's
/// own blessed installer); otherwise `binary` is tried first and, on
/// failure, porta falls back to `source` if the manifest offers it.
pub fn install(tool: &Tool, forced: Option<Strategy>) -> Result<Outcome> {
    match forced {
        Some(Strategy::Script) => {
            let spec = tool
                .script
                .as_ref()
                .ok_or_else(|| anyhow!("`{}` has no `script` install strategy", tool.name))?;
            script::install(tool, spec)
        }
        Some(Strategy::Binary) => {
            let spec = tool
                .binary
                .as_ref()
                .ok_or_else(|| anyhow!("`{}` has no `binary` install strategy", tool.name))?;
            binary::install(tool, spec)
        }
        Some(Strategy::Source) => {
            let spec = tool
                .source
                .as_ref()
                .ok_or_else(|| anyhow!("`{}` has no `source` install strategy", tool.name))?;
            source::install(tool, spec)
        }
        None => install_auto(tool),
    }
}

pub(crate) fn binary_file_name(tool_name: &str) -> String {
    if cfg!(windows) {
        format!("{tool_name}.exe")
    } else {
        tool_name.to_string()
    }
}

#[cfg(unix)]
pub(crate) fn make_executable(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
pub(crate) fn make_executable(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

fn install_auto(tool: &Tool) -> Result<Outcome> {
    if let Some(spec) = &tool.script {
        return script::install(tool, spec);
    }

    match (&tool.binary, &tool.source) {
        (Some(bspec), Some(sspec)) => match binary::install(tool, bspec) {
            Ok(outcome) => Ok(outcome),
            Err(binary_err) => {
                eprintln!(
                    "porta: prebuilt binary install of `{}` failed ({binary_err}); falling back to building from source",
                    tool.name
                );
                source::install(tool, sspec)
            }
        },
        (Some(bspec), None) => binary::install(tool, bspec),
        (None, Some(sspec)) => source::install(tool, sspec),
        (None, None) => bail!("tool `{}` has no install strategy", tool.name),
    }
}
