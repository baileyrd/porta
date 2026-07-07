//! Filesystem layout. Everything porta touches lives under `porta_home()`, a
//! single per-user directory — never a system path, never anything requiring
//! elevated privileges.

use std::path::PathBuf;

/// Root directory for all porta state: `$PORTA_HOME`, else
/// `%LOCALAPPDATA%\porta` on Windows, else `~/.porta` on Unix.
pub fn porta_home() -> PathBuf {
    if let Ok(custom) = std::env::var("PORTA_HOME") {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }

    if cfg!(windows) {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local_app_data).join("porta");
        }
    }

    home_dir().join(".porta")
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
