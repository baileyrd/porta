//! The tool manifest: a declarative TOML list of what `porta install <name>`
//! knows how to fetch, and how. Three independent install strategies can be
//! declared per tool:
//!
//! - `script`  — run the tool's own official no-admin installer (e.g. Claude
//!   Code's `install.sh`/`install.ps1`). Used when the vendor already ships a
//!   trustworthy, portable installer; porta just makes sure the result ends
//!   up on `PATH`.
//! - `binary`  — download a prebuilt archive for the current OS/arch and copy
//!   the binary it contains into porta's `bin/`.
//! - `source`  — `git clone` the tool's source and build it locally.
//!
//! A tool may declare `binary` and `source` together; `porta install` then
//! tries the binary first and falls back to building from source, or the
//! caller can force one with `--strategy`.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    #[serde(rename = "tool", default)]
    pub tools: Vec<Tool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Name of the installed command when it differs from `name` (the `ai`
    /// tool installs a binary called `claude`). Defaults to `name`.
    #[serde(default)]
    pub bin_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub script: Option<ScriptSpec>,
    #[serde(default)]
    pub binary: Option<BinarySpec>,
    #[serde(default)]
    pub source: Option<SourceSpec>,
}

impl Tool {
    pub fn label(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.name)
    }

    pub fn bin_name(&self) -> &str {
        self.bin_name.as_deref().unwrap_or(&self.name)
    }
}

/// Runs a vendor-provided installer script. `unix` is used on Linux/macOS,
/// `windows` on Windows; either may be absent if the vendor doesn't support
/// that platform.
#[derive(Debug, Clone, Deserialize)]
pub struct ScriptSpec {
    pub unix: Option<ScriptTarget>,
    pub windows: Option<ScriptTarget>,
    /// Directory (may contain `~`) the vendor's own installer places its
    /// binary into. porta adds this to `PATH` alongside its own `bin/` — it
    /// does *not* copy the binary itself, since the vendor's installer
    /// usually manages its own updates.
    pub installs_to: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptTarget {
    pub url: String,
    pub interpreter: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BinarySpec {
    /// A concrete version ("14.1.1"), or "latest" together with
    /// `version_url` to resolve the current version at install time.
    pub version: String,
    /// URL whose response body is the current version string. Fetched only
    /// when `version = "latest"`; a pinned version never touches it, so a
    /// user manifest can pin for reproducibility.
    #[serde(default)]
    pub version_url: Option<String>,
    pub targets: HashMap<String, BinaryTarget>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BinaryTarget {
    /// Download URL; `{version}` is replaced with the (resolved) version.
    pub url: String,
    pub archive: ArchiveKind,
    /// Path to the binary inside the extracted archive. Ignored for
    /// `archive = "raw"` (the download *is* the binary).
    #[serde(default)]
    pub binary_path: Option<String>,
    /// Optional SHA-256 verification of the downloaded file.
    #[serde(default)]
    pub checksum: Option<ChecksumSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChecksumSpec {
    /// URL of the checksum document; `{version}` is replaced like in `url`.
    pub url: String,
    /// If set, the document is JSON and this dotted path (e.g.
    /// `platforms.linux-x64.checksum`) locates the hex digest. If unset,
    /// the document's first whitespace-separated token is the digest
    /// (the common `<hex>  <filename>` format).
    #[serde(default)]
    pub json_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveKind {
    #[serde(rename = "tar.gz")]
    TarGz,
    Zip,
    /// The downloaded file *is* the binary — no extraction.
    Raw,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SourceSpec {
    pub repo: String,
    #[serde(default)]
    pub git_ref: Option<String>,
    pub build_cmd: Vec<String>,
    /// Path to the built binary, relative to the repo root.
    pub binary_path: String,
}

/// The manifest baked into the `porta` binary at compile time.
const BUILTIN_MANIFEST_TOML: &str = include_str!("../manifests/tools.toml");

pub fn load_builtin() -> Result<Manifest> {
    toml::from_str(BUILTIN_MANIFEST_TOML)
        .context("built-in manifest failed to parse (this is a porta bug)")
}

/// Loads the built-in manifest and merges in the user's override file at
/// `$PORTA_HOME/tools.toml`, if present. User entries with the same `name`
/// replace built-in ones; new names are appended.
pub fn load_merged() -> Result<Manifest> {
    let mut manifest = load_builtin()?;

    let user_path = crate::paths::user_manifest_file();
    if user_path.exists() {
        let text = std::fs::read_to_string(&user_path)
            .with_context(|| format!("reading {}", user_path.display()))?;
        let user: Manifest =
            toml::from_str(&text).with_context(|| format!("parsing {}", user_path.display()))?;

        for tool in user.tools {
            if let Some(existing) = manifest.tools.iter_mut().find(|t| t.name == tool.name) {
                *existing = tool;
            } else {
                manifest.tools.push(tool);
            }
        }
    }

    validate(&manifest)?;
    Ok(manifest)
}

impl Manifest {
    pub fn find(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }
}

/// The manifest's target key for the machine porta is running on, e.g.
/// `linux-x86_64`, `macos-aarch64`, `windows-x86_64`.
pub fn current_target() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// Resolve `~` at the start of a path (the manifest's `installs_to` fields
/// use it for readability) against the current user's home directory.
pub fn expand_tilde(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        crate::paths::home_dir().join(rest)
    } else if path == "~" {
        crate::paths::home_dir()
    } else {
        std::path::PathBuf::from(path)
    }
}

/// Substitute the `{version}` placeholder in a manifest URL.
pub fn template_url(url: &str, version: &str) -> String {
    url.replace("{version}", version)
}

/// A fetched "latest" version string must look like a version — reject an
/// HTML error page or empty body before it gets templated into URLs.
pub fn validate_version_string(s: &str) -> Result<String> {
    let v = s.trim();
    let plausible = !v.is_empty()
        && v.len() < 64
        && v.chars().next().is_some_and(|c| c.is_ascii_digit())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'));
    if !plausible {
        bail!("version endpoint returned something that doesn't look like a version: {v:.80?}");
    }
    Ok(v.to_string())
}

/// Walk a dotted path (`platforms.linux-x64.checksum`) through a JSON
/// document and return the string at the end of it.
pub fn json_string_at_path<'a>(doc: &'a serde_json::Value, path: &str) -> Result<&'a str> {
    let mut node = doc;
    for key in path.split('.') {
        node = node
            .get(key)
            .with_context(|| format!("JSON key `{key}` not found (path `{path}`)"))?;
    }
    node.as_str()
        .with_context(|| format!("value at JSON path `{path}` is not a string"))
}

pub fn require_binary_target<'a>(spec: &'a BinarySpec, target: &str) -> Result<&'a BinaryTarget> {
    spec.targets.get(target).with_context(|| {
        format!(
            "no prebuilt binary published for target `{target}` (have: {})",
            spec.targets.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    })
}

pub fn validate(manifest: &Manifest) -> Result<()> {
    for tool in &manifest.tools {
        if tool.script.is_none() && tool.binary.is_none() && tool.source.is_none() {
            bail!(
                "tool `{}` declares no install strategy (script/binary/source)",
                tool.name
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_manifest_parses_and_is_valid() {
        let manifest = load_builtin().expect("builtin manifest parses");
        validate(&manifest).expect("builtin manifest is valid");
        assert!(manifest.find("ai").is_some(), "expected an `ai` tool entry");
    }

    #[test]
    fn current_target_has_expected_shape() {
        let target = current_target();
        assert!(target.contains('-'), "target should be `os-arch`: {target}");
    }

    #[test]
    fn version_strings_are_validated() {
        assert_eq!(validate_version_string("2.1.202\n").unwrap(), "2.1.202");
        assert_eq!(validate_version_string("14.1.1").unwrap(), "14.1.1");
        assert!(validate_version_string("").is_err());
        assert!(validate_version_string("<html>error</html>").is_err());
        assert!(validate_version_string("v with spaces").is_err());
    }

    #[test]
    fn url_templating_and_json_path() {
        assert_eq!(
            template_url("https://x/{version}/linux-x64/claude", "2.1.202"),
            "https://x/2.1.202/linux-x64/claude"
        );
        let doc: serde_json::Value =
            serde_json::from_str(r#"{"platforms": {"linux-x64": {"checksum": "abc123"}}}"#)
                .unwrap();
        assert_eq!(
            json_string_at_path(&doc, "platforms.linux-x64.checksum").unwrap(),
            "abc123"
        );
        assert!(json_string_at_path(&doc, "platforms.nope.checksum").is_err());
    }

    #[test]
    fn expand_tilde_only_touches_leading_tilde() {
        let home = crate::paths::home_dir();
        assert_eq!(expand_tilde("~/.local/bin"), home.join(".local/bin"));
        assert_eq!(
            expand_tilde("/opt/tool"),
            std::path::PathBuf::from("/opt/tool")
        );
    }
}
