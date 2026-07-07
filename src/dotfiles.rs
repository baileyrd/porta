//! Dotfile management: keep configuration files *inside* the portable
//! environment and link them into `$HOME`, so `.gitconfig`, `.bashrc`,
//! `.config/nvim/`, etc. travel with `~/.porta` instead of being left
//! behind on the old machine.
//!
//! Model (deliberately small — think "stow", not "chezmoi"):
//! - `porta dotfiles add <path>` MOVES the real file into
//!   `$PORTA_HOME/dotfiles/<home-relative-path>` and leaves a symlink at
//!   the original location.
//! - the tracked list lives in `state.json` (which itself travels), so on
//!   a new machine `porta dotfiles link` — run automatically by
//!   `porta init` — recreates every symlink.
//! - a real file already present at a link target is preserved as
//!   `<name>.porta-backup`, never overwritten.
//! - on Windows, creating symlinks without admin requires Developer Mode;
//!   when that fails porta falls back to copying the file into place (and
//!   says so — edits then need `porta dotfiles add` again to re-capture).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub fn store_dir() -> PathBuf {
    crate::paths::porta_home().join("dotfiles")
}

/// `$HOME`-relative form of `path` (e.g. `.config/git/config`), used both
/// as the store key and the `state.json` entry.
fn home_relative(path: &Path) -> Result<PathBuf> {
    let abs = std::path::absolute(path).with_context(|| format!("resolving {}", path.display()))?;
    let home = crate::paths::home_dir();
    let rel = abs.strip_prefix(&home).map_err(|_| {
        anyhow::anyhow!(
            "{} is not under your home directory ({}) — only home dotfiles can be tracked",
            abs.display(),
            home.display()
        )
    })?;
    if rel.as_os_str().is_empty() {
        bail!("cannot track the home directory itself");
    }
    let porta_home = crate::paths::porta_home();
    if abs.starts_with(&porta_home) {
        bail!(
            "{} is inside $PORTA_HOME — it already travels with the environment",
            abs.display()
        );
    }
    Ok(rel.to_path_buf())
}

pub fn add(paths: &[PathBuf]) -> Result<()> {
    let mut state = crate::state::State::load()?;

    for path in paths {
        let rel = home_relative(path)?;
        let rel_str = rel.display().to_string();
        let home_path = crate::paths::home_dir().join(&rel);
        let store_path = store_dir().join(&rel);

        let meta = std::fs::symlink_metadata(&home_path)
            .with_context(|| format!("{} does not exist", home_path.display()))?;
        if meta.is_symlink() {
            if std::fs::read_link(&home_path)
                .map(|t| t == store_path)
                .unwrap_or(false)
            {
                println!("porta: `{rel_str}` is already tracked");
                continue;
            }
            bail!(
                "{} is a symlink (to somewhere other than porta's store) — refusing to move it",
                home_path.display()
            );
        }

        if store_path.exists() {
            bail!(
                "{} already exists in the dotfiles store — `porta dotfiles remove {rel_str}` first",
                store_path.display()
            );
        }

        if let Some(parent) = store_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        move_into_store(&home_path, &store_path)?;
        link_one(&rel, true)?;

        if !state.dotfiles.contains(&rel_str) {
            state.dotfiles.push(rel_str.clone());
            state.dotfiles.sort();
        }
        println!(
            "porta: tracking `{rel_str}` (stored in {})",
            store_dir().display()
        );
    }

    state.save()
}

/// (Re)create the `$HOME` links for every tracked entry. Returns how many
/// entries were processed.
pub fn link_all() -> Result<usize> {
    let state = crate::state::State::load()?;
    for rel_str in &state.dotfiles {
        link_one(Path::new(rel_str), false)?;
    }
    Ok(state.dotfiles.len())
}

fn link_one(rel: &Path, fresh_add: bool) -> Result<()> {
    let home_path = crate::paths::home_dir().join(rel);
    let store_path = store_dir().join(rel);

    if !store_path.exists() {
        bail!(
            "tracked dotfile `{}` is missing from the store at {}",
            rel.display(),
            store_path.display()
        );
    }

    if let Ok(meta) = std::fs::symlink_metadata(&home_path) {
        if meta.is_symlink() {
            if std::fs::read_link(&home_path)
                .map(|t| t == store_path)
                .unwrap_or(false)
            {
                return Ok(()); // already correct
            }
            // A porta link from a previous home layout (or elsewhere):
            // replace it.
            std::fs::remove_file(&home_path)
                .with_context(|| format!("removing stale symlink {}", home_path.display()))?;
        } else if !fresh_add {
            // A real file the new machine already had: keep it, don't
            // clobber.
            let backup = backup_path(&home_path);
            if backup.exists() {
                bail!(
                    "both {} and its backup {} exist — resolve manually",
                    home_path.display(),
                    backup.display()
                );
            }
            std::fs::rename(&home_path, &backup).with_context(|| {
                format!("backing up {} to {}", home_path.display(), backup.display())
            })?;
            println!(
                "porta: existing {} preserved as {}",
                home_path.display(),
                backup.display()
            );
        }
    }

    if let Some(parent) = home_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    make_link(&store_path, &home_path)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".porta-backup");
    PathBuf::from(s)
}

pub fn list() -> Result<()> {
    let state = crate::state::State::load()?;
    if state.dotfiles.is_empty() {
        println!("no dotfiles tracked yet — try `porta dotfiles add ~/.gitconfig`");
        return Ok(());
    }
    for rel_str in &state.dotfiles {
        let rel = Path::new(rel_str);
        let home_path = crate::paths::home_dir().join(rel);
        let store_path = store_dir().join(rel);

        let status = match std::fs::symlink_metadata(&home_path) {
            Ok(meta) if meta.is_symlink() => {
                if std::fs::read_link(&home_path)
                    .map(|t| t == store_path)
                    .unwrap_or(false)
                {
                    "linked"
                } else {
                    "symlink elsewhere (run `porta dotfiles link`)"
                }
            }
            Ok(_) => "present but not linked (run `porta dotfiles link`)",
            Err(_) => "not linked (run `porta dotfiles link`)",
        };
        println!("{rel_str:<40} {status}");
    }
    Ok(())
}

/// Stop tracking: put a real copy of the content back at `$HOME/<rel>` and
/// delete the store entry.
pub fn remove(path: &Path) -> Result<()> {
    let mut state = crate::state::State::load()?;

    // Accept either a home path (~/.gitconfig) or the tracked relative form.
    let rel = match home_relative(path) {
        Ok(rel) => rel,
        Err(_) => path.to_path_buf(),
    };
    let rel_str = rel.display().to_string();

    let Some(idx) = state.dotfiles.iter().position(|e| *e == rel_str) else {
        bail!("`{rel_str}` is not a tracked dotfile (see `porta dotfiles list`)");
    };

    let home_path = crate::paths::home_dir().join(&rel);
    let store_path = store_dir().join(&rel);

    if std::fs::symlink_metadata(&home_path)
        .map(|m| m.is_symlink())
        .unwrap_or(false)
    {
        std::fs::remove_file(&home_path)
            .with_context(|| format!("removing symlink {}", home_path.display()))?;
    }
    if store_path.exists() {
        move_into_store(&store_path, &home_path)
            .with_context(|| format!("restoring {}", home_path.display()))?;
    }
    // Prune now-empty directories left behind in the store.
    let mut dir = store_path.parent().map(Path::to_path_buf);
    while let Some(d) = dir {
        if d == store_dir() || std::fs::remove_dir(&d).is_err() {
            break;
        }
        dir = d.parent().map(Path::to_path_buf);
    }

    state.dotfiles.remove(idx);
    state.save()?;
    println!(
        "porta: `{rel_str}` restored to {} and untracked",
        home_path.display()
    );
    Ok(())
}

/// Move a file or directory, falling back to copy+delete for cross-device
/// renames.
fn move_into_store(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_recursively(from, to)?;
    if from.is_dir() {
        std::fs::remove_dir_all(from)?;
    } else {
        std::fs::remove_file(from)?;
    }
    Ok(())
}

pub(crate) fn copy_recursively(from: &Path, to: &Path) -> Result<()> {
    let meta =
        std::fs::symlink_metadata(from).with_context(|| format!("reading {}", from.display()))?;
    if meta.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursively(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to)
            .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
    }
    Ok(())
}

#[cfg(unix)]
fn make_link(store_path: &Path, home_path: &Path) -> Result<()> {
    std::os::unix::fs::symlink(store_path, home_path).with_context(|| {
        format!(
            "symlinking {} -> {}",
            home_path.display(),
            store_path.display()
        )
    })
}

#[cfg(windows)]
fn make_link(store_path: &Path, home_path: &Path) -> Result<()> {
    let result = if store_path.is_dir() {
        std::os::windows::fs::symlink_dir(store_path, home_path)
    } else {
        std::os::windows::fs::symlink_file(store_path, home_path)
    };
    if result.is_ok() {
        return Ok(());
    }
    // Symlink creation without admin needs Developer Mode; degrade to a
    // copy so the dotfile is still usable, and be explicit that it's a copy.
    copy_recursively(store_path, home_path)?;
    eprintln!(
        "porta: note: couldn't create a symlink for {} (enable Windows Developer Mode for \
         symlinks); placed a COPY instead — re-run `porta dotfiles add` after editing it \
         to capture changes",
        home_path.display()
    );
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn make_link(store_path: &Path, home_path: &Path) -> Result<()> {
    copy_recursively(store_path, home_path)
}

// The lifecycle test for this module lives in `tests/dotfiles_cli.rs`: it
// drives the real binary with a scratch HOME/PORTA_HOME per child process,
// because in-process tests can't set those env vars without racing other
// unit tests that read them.
