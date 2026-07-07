//! Archive extraction, built into the porta binary via pure-Rust
//! decompressors (`flate2`/`tar`/`zip`). porta deliberately does NOT shell
//! out to host `tar`/`unzip`/`Expand-Archive`: the whole point of the
//! environment is to work on machines where nothing can be assumed
//! present, and that has to include the tools porta itself depends on.
//! Both codecs guard against path-traversal ("zip-slip") entries.

use crate::manifest::ArchiveKind;
use anyhow::{bail, Context, Result};
use std::path::Path;

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
    let file =
        std::fs::File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let decoder = flate2::read::GzDecoder::new(std::io::BufReader::new(file));
    // `unpack` refuses entries that would escape `dest`.
    tar::Archive::new(decoder)
        .unpack(dest)
        .with_context(|| format!("extracting {}", archive.display()))
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file =
        std::fs::File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .with_context(|| format!("reading {} as a zip archive", archive.display()))?;
    // `extract` skips entries whose names would escape `dest` and restores
    // unix modes where the archive carries them.
    zip.extract(dest)
        .with_context(|| format!("extracting {}", archive.display()))
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

/// The directory to treat as an extracted source tree's root: `dir` itself
/// if it directly contains files, else the single top-level directory
/// (repo tarballs from GitHub-style forges nest everything under
/// `<repo>-<ref>/`).
pub fn source_root(dir: &Path) -> Result<std::path::PathBuf> {
    let entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .collect::<std::io::Result<_>>()?;

    match entries.as_slice() {
        [single] if single.file_type()?.is_dir() => Ok(single.path()),
        [] => bail!("{} is empty after extraction", dir.display()),
        _ => Ok(dir.to_path_buf()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch_dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("porta-archive-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Build a tar.gz in pure Rust — the tests are as host-tool-free as the
    /// code under test.
    fn write_tar_gz(archive: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(archive).unwrap();
        let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        let mut builder = tar::Builder::new(encoder);
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, contents.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap();
    }

    fn write_zip(archive: &Path, entries: &[(&str, &str)]) {
        let file = std::fs::File::create(archive).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (path, contents) in entries {
            writer.start_file(*path, options).unwrap();
            writer.write_all(contents.as_bytes()).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn extract_tar_gz_roundtrip() {
        let work = scratch_dir("targz");
        let archive_path = work.join("out.tar.gz");
        write_tar_gz(&archive_path, &[("nested-1.2.3/rg", "pretend binary")]);

        let dest = work.join("extracted");
        extract(&archive_path, ArchiveKind::TarGz, &dest).unwrap();

        let found = locate(&dest, "nested-1.2.3/rg").unwrap();
        assert_eq!(std::fs::read_to_string(found).unwrap(), "pretend binary");

        std::fs::remove_dir_all(&work).ok();
    }

    #[test]
    fn extract_zip_roundtrip() {
        let work = scratch_dir("zip");
        let archive_path = work.join("out.zip");
        write_zip(
            &archive_path,
            &[("gh_9.9.9_linux_amd64/bin/gh", "pretend gh")],
        );

        let dest = work.join("extracted");
        extract(&archive_path, ArchiveKind::Zip, &dest).unwrap();

        // manifest-style lookup: `bin/gh` under an unpredictable top dir
        let found = locate(&dest, "bin/gh").unwrap();
        assert_eq!(std::fs::read_to_string(found).unwrap(), "pretend gh");

        std::fs::remove_dir_all(&work).ok();
    }

    #[test]
    fn source_root_unwraps_single_top_dir() {
        let work = scratch_dir("srcroot");
        let archive_path = work.join("src.tar.gz");
        write_tar_gz(
            &archive_path,
            &[
                ("repo-1.0.0/Cargo.toml", "[package]"),
                ("repo-1.0.0/src/main.rs", "fn main() {}"),
            ],
        );
        let dest = work.join("extracted");
        extract(&archive_path, ArchiveKind::TarGz, &dest).unwrap();

        let root = source_root(&dest).unwrap();
        assert!(root.ends_with("repo-1.0.0"));
        assert!(root.join("Cargo.toml").is_file());

        // A flat layout is its own root.
        let flat = scratch_dir("srcroot-flat");
        std::fs::write(flat.join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(flat.join("lib.rs"), "").unwrap();
        assert_eq!(source_root(&flat).unwrap(), flat);

        std::fs::remove_dir_all(&work).ok();
        std::fs::remove_dir_all(&flat).ok();
    }

    #[test]
    fn locate_errors_when_truly_missing() {
        let work = scratch_dir("missing");
        let dest = work.join("extracted");
        std::fs::create_dir_all(&dest).unwrap();

        assert!(locate(&dest, "nowhere/rg").is_err());

        std::fs::remove_dir_all(&work).ok();
    }

    #[test]
    fn tar_path_traversal_is_rejected() {
        let work = scratch_dir("slip");
        let archive_path = work.join("evil.tar.gz");

        // tar::Builder itself refuses to write `..` entries, so a malicious
        // header has to be crafted by hand to test the unpack-side guard.
        let mut header = [0u8; 512];
        let name = b"../evil.txt";
        header[..name.len()].copy_from_slice(name);
        header[100..107].copy_from_slice(b"0000644"); // mode
        header[124..135].copy_from_slice(b"00000000007"); // size = 7 octal
        header[136..147].copy_from_slice(b"00000000000"); // mtime
        header[156] = b'0'; // regular file
        header[148..156].copy_from_slice(b"        "); // chksum = spaces while summing
        let sum: u32 = header.iter().map(|&b| b as u32).sum();
        let chksum = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(chksum.as_bytes());

        let mut raw = Vec::new();
        raw.extend_from_slice(&header);
        let mut data = [0u8; 512];
        data[..7].copy_from_slice(b"escaped");
        raw.extend_from_slice(&data);
        raw.extend_from_slice(&[0u8; 1024]); // end-of-archive blocks

        let file = std::fs::File::create(&archive_path).unwrap();
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        encoder.write_all(&raw).unwrap();
        encoder.finish().unwrap();

        let dest = work.join("extracted");
        let result = extract(&archive_path, ArchiveKind::TarGz, &dest);
        // Either the unpack errors or the entry is skipped — the file must
        // not appear outside `dest`.
        let escaped = work.join("evil.txt");
        assert!(!escaped.exists(), "path traversal escaped the sandbox");
        drop(result);

        std::fs::remove_dir_all(&work).ok();
    }
}
