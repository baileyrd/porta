//! Filesystem layout. Everything porta touches lives under `porta_home()`, a
//! single per-user directory — never a system path, never anything requiring
//! elevated privileges. The directory can live anywhere the user chooses
//! (`porta init --home C:\tools\porta`) and be relocated later
//! (`porta move`).

use std::path::PathBuf;

/// How the porta home directory was determined — shown by `porta doctor`
/// so a surprising resolution is explainable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeSource {
    /// `$PORTA_HOME` environment variable.
    Env,
    /// Derived from the running executable's own location
    /// (`<home>/bin/porta` next to a `<home>/state.json`).
    Executable,
    /// The platform default (`~/.porta`, `%LOCALAPPDATA%\porta`).
    Default,
}

impl HomeSource {
    pub fn describe(self) -> &'static str {
        match self {
            HomeSource::Env => "from $PORTA_HOME",
            HomeSource::Executable => "located from the porta executable",
            HomeSource::Default => "platform default",
        }
    }
}

/// Root directory for all porta state. Resolution order:
///
/// 1. `$PORTA_HOME` — explicit override, always wins.
/// 2. **Self-location**: if this executable runs from `<dir>/bin/` and
///    `<dir>/state.json` exists, `<dir>` is the home. This is what lets the
///    whole folder live anywhere (`C:\tools\porta`, a USB stick, ...) and
///    be moved freely — the binary inside it finds its own home with no
///    environment setup. `state.json` acts as the marker so a porta binary
///    copied into an unrelated `bin/` directory (e.g. `/usr/local/bin`)
///    never mistakes it for a porta home.
/// 3. The platform default: `%LOCALAPPDATA%\porta` on Windows, `~/.porta`
///    elsewhere.
pub fn porta_home() -> PathBuf {
    porta_home_with_source().0
}

pub fn porta_home_with_source() -> (PathBuf, HomeSource) {
    if let Ok(custom) = std::env::var("PORTA_HOME") {
        if !custom.is_empty() {
            return (PathBuf::from(custom), HomeSource::Env);
        }
    }

    if let Some(home) = self_located_home() {
        return (home, HomeSource::Executable);
    }

    (default_home(), HomeSource::Default)
}

fn default_home() -> PathBuf {
    if cfg!(windows) {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("porta");
        }
    }
    home_dir().join(".porta")
}

fn self_located_home() -> Option<PathBuf> {
    // canonicalize so a symlinked `porta` resolves to where it really lives.
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let bin = exe.parent()?;
    if bin.file_name()?.to_str()? != "bin" {
        return None;
    }
    let home = bin.parent()?;
    if home.join("state.json").is_file() {
        Some(home.to_path_buf())
    } else {
        None
    }
}

/// The user's home directory, resolved without any third-party crate.
pub fn home_dir() -> PathBuf {
    if cfg!(windows) {
        if let Ok(profile) = std::env::var("USERPROFILE") {
            return PathBuf::from(profile);
        }
        if let (Ok(drive), Ok(path)) = (std::env::var("HOMEDRIVE"), std::env::var("HOMEPATH")) {
            return PathBuf::from(format!("{drive}{path}"));
        }
    }
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Where installed tool binaries (and shims) live. This is the one directory
/// porta asks to be put on `PATH`.
pub fn bin_dir() -> PathBuf {
    porta_home().join("bin")
}

/// Scratch/staging area for source builds and extracted archives.
pub fn tools_dir() -> PathBuf {
    porta_home().join("tools")
}

/// Download cache (archives, cloned repos are cleaned up; this is for raw
/// downloaded files so re-installs of the same version can skip the network).
pub fn cache_dir() -> PathBuf {
    porta_home().join("cache")
}

/// JSON registry of what porta has installed.
pub fn state_file() -> PathBuf {
    porta_home().join("state.json")
}

/// Optional user-authored manifest that extends/overrides the built-in tool
/// list. Not required to exist.
pub fn user_manifest_file() -> PathBuf {
    porta_home().join("tools.toml")
}

/// Ensure the core directories exist. Safe to call repeatedly.
pub fn ensure_layout() -> std::io::Result<()> {
    for dir in [bin_dir(), tools_dir(), cache_dir()] {
        std::fs::create_dir_all(dir)?;
    }
    Ok(())
}
