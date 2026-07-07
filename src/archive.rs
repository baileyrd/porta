//! Archive extraction. Deliberately shells out to platform-native tools
//! (`tar`, `unzip`, PowerShell's `Expand-Archive`) instead of vendoring a
//! decompressor: every target platform already ships one, and none of them
//! need admin rights to invoke.

use crate::manifest::ArchiveKind;
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

pub fn extract(archive: &Path, kind: ArchiveKind, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)
        .with_context(|| format!("creating extraction directory {}", dest.display()))?;

    match kind {
        ArchiveKind::Raw => {
            bail!("ArchiveKind::Raw should be copied directly, not extracted");
        }
        ArchiveKind::TarGz => extract_tar_gz(archive, dest),
        ArchiveKind::Zip => extract_zip(archive, dest),
    }
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> Result<()> {
    run(
        Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(dest),
        "tar -xzf",
    )
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    // `tar` extracts zip files fine on the bsdtar (libarchive) builds shipped
    // by default on macOS and Windows 10 1803+; try it first since it's a
    // single, uniform code path across platforms.
    let tar_result = Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status();
    if let Ok(status) = tar_result {
        if status.success() {
            return Ok(());
        }
    }

    if cfg!(windows) {
        run(
            Command::new("powershell").args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                &format!(
                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                    archive.display(),
                    dest.display()
                ),
            ]),
            "Expand-Archive",
        )
    } else {
        run(
            Command::new("unzip")
                .arg("-o")
                .arg(archive)
                .arg("-d")
                .arg(dest),
            "unzip",
        )
    }
}

fn run(cmd: &mut Command, what: &str) -> Result<()> {
    let status = cmd
        .status()
        .with_context(|| format!("failed to launch `{what}` (is it installed?)"))?;
    if !status.success() {
        bail!("`{what}` exited with {status}");
    }
    Ok(())
}

/// Find a file within `root` whose path (relative to `root`) matches
/// `relative`, tolerating a single top-level directory whose name porta
/// couldn't predict (some archives nest under `<pkg>-<version>-<target>/`,
/// others don't). Tries the exact path first, then a one-level search.
pub fn locate(root: &Path, relative: &str) -> Result<std::path::PathBuf> {
    let exact = root.join(relative);
    if exact.exists() {
        return Ok(exact);
    }

    let file_name = Path::new(relative)
        .file_name()
        .context("binary_path in manifest has no file name")?;

    for entry in std::fs::read_dir(root).with_context(|| format!("reading {}", root.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let candidate = entry.path().join(relative);
            if candidate.exists() {
                return Ok(candidate);
            }
            let candidate = entry.path().join(file_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    bail!(
        "could not find `{relative}` anywhere under {} after extraction",
        root.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("porta-archive-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn extract_tar_gz_roundtrip() {
        let work = scratch_dir("targz");
        let payload_dir = work.join("payload");
        std::fs::create_dir_all(payload_dir.join("nested-1.2.3")).unwrap();
        std::fs::write(payload_dir.join("nested-1.2.3/rg"), b"pretend binary").unwrap();

        let archive_path = work.join("out.tar.gz");
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive_path)
            .arg("-C")
            .arg(&payload_dir)
            .arg("nested-1.2.3")
            .status()
            .unwrap();
        assert!(status.success());

        let dest = work.join("extracted");
        extract(&archive_path, ArchiveKind::TarGz, &dest).unwrap();

        let found = locate(&dest, "nested-1.2.3/rg").unwrap();
        assert_eq!(std::fs::read_to_string(found).unwrap(), "pretend binary");

        std::fs::remove_dir_all(&work).ok();
    }

    #[test]
    fn locate_tolerates_unpredictable_top_level_dir() {
        let work = scratch_dir("locate");
        let dest = work.join("extracted");
        std::fs::create_dir_all(dest.join("some-unpredictable-dir-name")).unwrap();
        std::fs::write(dest.join("some-unpredictable-dir-name/rg"), b"x").unwrap();

        // `locate` is asked for a binary_path whose directory component
        // doesn't match what's actually on disk (a differently-named or
        // differently-versioned top-level dir) — it should still find the
        // binary by file name inside whatever directory is there.
        let found = locate(&dest, "some-other-dir/rg").unwrap();
        assert_eq!(found.file_name().unwrap(), "rg");

        std::fs::remove_dir_all(&work).ok();
    }

    #[test]
    fn locate_errors_when_truly_missing() {
        let work = scratch_dir("missing");
        let dest = work.join("extracted");
        std::fs::create_dir_all(&dest).unwrap();

        assert!(locate(&dest, "nowhere/rg").is_err());

        std::fs::remove_dir_all(&work).ok();
    }
}
