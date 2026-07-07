//! The `binary` strategy: download a prebuilt archive (or raw binary) for
//! the current OS/arch, optionally verify its SHA-256 against a published
//! checksum document, and place the binary in porta's `bin/`. This is the
//! strategy that keeps a tool fully inside `$PORTA_HOME`, which is what
//! makes the environment copyable to another machine.

use crate::archive;
use crate::install::{binary_file_name, make_executable, Outcome, Strategy};
use crate::manifest::{self, ArchiveKind, BinarySpec, Tool};
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

pub fn install(tool: &Tool, spec: &BinarySpec) -> Result<Outcome> {
    let target_key = manifest::current_target();
    let target = manifest::require_binary_target(spec, &target_key)?;

    let version = resolve_version(spec)?;
    let url = manifest::template_url(&target.url, &version);

    println!(
        "porta: downloading `{}` {version} for {target_key}",
        tool.label()
    );

    crate::paths::ensure_layout()?;
    let cache_dir = crate::paths::cache_dir().join(&tool.name).join(&version);
    let archive_file_name = url.rsplit('/').next().unwrap_or("download").to_string();
    let archive_path = cache_dir.join(&archive_file_name);

    if !archive_path.exists() {
        crate::download::download_to_file(&url, &archive_path)
            .with_context(|| format!("downloading {url}"))?;
    }

    // Verified even for cache hits: it's cheap, and it means a corrupted or
    // tampered cache entry can never be installed.
    if let Some(checksum) = &target.checksum {
        verify_sha256(
            &archive_path,
            &archive_file_name,
            checksum,
            &version,
            &target_key,
        )
        .with_context(|| {
            format!(
                "checksum verification failed for {}",
                archive_path.display()
            )
        })?;
    }

    let dest_bin = crate::paths::bin_dir().join(binary_file_name(tool.bin_name()));

    match target.archive {
        ArchiveKind::Raw => {
            std::fs::create_dir_all(crate::paths::bin_dir())?;
            std::fs::copy(&archive_path, &dest_bin).with_context(|| {
                format!(
                    "copying {} to {}",
                    archive_path.display(),
                    dest_bin.display()
                )
            })?;
        }
        kind => {
            let binary_path = target.binary_path.as_deref().with_context(|| {
                format!("`{}` needs `binary_path` for archive extraction", tool.name)
            })?;
            let extract_dir = cache_dir.join("extracted");
            if extract_dir.exists() {
                std::fs::remove_dir_all(&extract_dir)?;
            }
            archive::extract(&archive_path, kind, &extract_dir)
                .with_context(|| format!("extracting {}", archive_path.display()))?;
            let found = archive::locate(&extract_dir, binary_path)?;
            std::fs::create_dir_all(crate::paths::bin_dir())?;
            std::fs::copy(&found, &dest_bin).with_context(|| {
                format!("copying {} to {}", found.display(), dest_bin.display())
            })?;
        }
    }

    make_executable(&dest_bin)?;

    Ok(Outcome {
        version,
        strategy: Strategy::Binary,
        location: dest_bin.display().to_string(),
    })
}

/// `version = "latest"` + `version_url` resolves the current version over
/// the network at install time; a pinned version is used as-is (and never
/// touches the network), so a user manifest can pin for reproducibility.
fn resolve_version(spec: &BinarySpec) -> Result<String> {
    if spec.version != "latest" {
        return Ok(spec.version.clone());
    }
    let Some(version_url) = &spec.version_url else {
        // "latest" without a resolver is just a literal label.
        return Ok(spec.version.clone());
    };
    let body = crate::download::fetch_text(version_url)
        .with_context(|| format!("resolving latest version from {version_url}"))?;
    manifest::validate_version_string(&body)
}

fn verify_sha256(
    file: &std::path::Path,
    file_name: &str,
    checksum: &manifest::ChecksumSpec,
    version: &str,
    target_key: &str,
) -> Result<()> {
    let url = manifest::template_url(&checksum.url, version);
    let doc = crate::download::fetch_text(&url).with_context(|| format!("fetching {url}"))?;

    let expected = match &checksum.json_path {
        Some(path) => {
            let json: serde_json::Value = serde_json::from_str(&doc)
                .with_context(|| format!("{url} did not return valid JSON"))?;
            manifest::json_string_at_path(&json, path)?.to_ascii_lowercase()
        }
        None => digest_from_plain_doc(&doc, file_name)
            .with_context(|| format!("parsing checksum document from {url}"))?,
    };

    let bytes = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;
    let actual = hex(&Sha256::digest(&bytes));

    if actual != expected {
        bail!(
            "SHA-256 mismatch for {target_key}: expected {expected}, got {actual} \
             (delete the cached file and retry; if it persists, the download may be compromised)"
        );
    }
    Ok(())
}

/// Extract the digest for `file_name` from a plain (non-JSON) checksum
/// document. Handles both shapes in the wild:
///
/// - a single-file `.sha256` document (`<hex>` or `<hex>  <filename>`) —
///   the first token is the digest;
/// - a combined `checksums.txt` in `sha256sum` format, one
///   `<hex>  <filename>` line per release asset (gh, fd, and most goreleaser
///   projects ship these) — the line whose filename matches wins. A leading
///   `*` on the filename (sha256sum's binary-mode marker) is tolerated.
fn digest_from_plain_doc(doc: &str, file_name: &str) -> Result<String> {
    let lines: Vec<&str> = doc.lines().filter(|l| !l.trim().is_empty()).collect();

    for line in &lines {
        let mut tokens = line.split_whitespace();
        let (Some(digest), Some(name)) = (tokens.next(), tokens.next()) else {
            continue;
        };
        if name == file_name || name.strip_prefix('*') == Some(file_name) {
            return Ok(digest.to_ascii_lowercase());
        }
    }

    // No filename matched: only safe to fall back to "first token" when the
    // document plainly describes a single file.
    if lines.len() == 1 {
        let first = lines[0]
            .split_whitespace()
            .next()
            .context("empty checksum document")?;
        return Ok(first.to_ascii_lowercase());
    }

    bail!(
        "checksum document lists {} entries but none match `{file_name}`",
        lines.len()
    )
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_encodes_lowercase() {
        assert_eq!(hex(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    fn sha256_digest_matches_known_vector() {
        // sha256("abc") — FIPS 180-2 test vector.
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn plain_checksum_doc_single_file() {
        // bare digest
        assert_eq!(
            digest_from_plain_doc("ABC123\n", "tool.tar.gz").unwrap(),
            "abc123"
        );
        // `<hex>  <filename>` with a matching name
        assert_eq!(
            digest_from_plain_doc("abc123  tool.tar.gz\n", "tool.tar.gz").unwrap(),
            "abc123"
        );
        // single line whose filename doesn't match still falls back
        assert_eq!(
            digest_from_plain_doc("abc123  renamed.tar.gz\n", "tool.tar.gz").unwrap(),
            "abc123"
        );
    }

    #[test]
    fn plain_checksum_doc_combined_matches_by_filename() {
        // the gh_X_checksums.txt shape: one line per release asset
        let doc = "\
aaa111  gh_2.96.0_linux_386.tar.gz
bbb222  gh_2.96.0_linux_amd64.tar.gz
ccc333 *gh_2.96.0_macOS_arm64.zip
ddd444  gh_2.96.0_windows_amd64.zip
";
        assert_eq!(
            digest_from_plain_doc(doc, "gh_2.96.0_linux_amd64.tar.gz").unwrap(),
            "bbb222"
        );
        // sha256sum binary-mode `*` prefix is tolerated
        assert_eq!(
            digest_from_plain_doc(doc, "gh_2.96.0_macOS_arm64.zip").unwrap(),
            "ccc333"
        );
        // multi-line with no match must error, never guess
        assert!(digest_from_plain_doc(doc, "gh_2.96.0_linux_arm64.tar.gz").is_err());
    }
}
